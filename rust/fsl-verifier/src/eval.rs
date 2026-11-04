// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use fsl_core::binder_range::{
    InclusiveI64, RangeBinding, RangePlan, RangePrerequisite, RangeUnknown,
};
use fsl_core::{
    FslValue, KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr,
    KernelModel, Pattern, TypeDef, TypeRef,
};
use fsl_core::{pattern_binding_flows, recursion};
use fsl_solver::{ModelValue, SmtSolver};

use crate::VerifyError;
use crate::bmc::{CheckedStateCarriers, DischargedIntegerRange};
use crate::value::TypedBindings as Bindings;
use crate::value::{
    BindingRecord, RangeFact, SymbolicState, SymbolicValue, TypedBinding, bool_term, coerce,
    collection_contains, collection_entries, collection_key_equal, concrete_value,
    distinct_entries, i64_index, int_term, is_collection, ite_value, literal_key, logical_equal,
    ordered_collection_entries, select_finite, solver_sort,
};

type BinderCandidates<T> = Vec<(String, SymbolicValue<T>)>;
type SymbolicPair<T> = (SymbolicValue<T>, SymbolicValue<T>);

#[cfg(test)]
mod range_metadata_tests;
#[cfg(test)]
mod scope_tests;

/// Symbolic evaluation of one kernel expression, and the cycle entry this
/// module's stack guard belongs on (`recursion::guard`): `eval_binary`,
/// `eval_equality_operands`, `eval_method`, `eval_quantified`, and
/// `eval_aggregate` all re-enter `eval` for their operands, so guarding this
/// one function guards the cycle.
///
/// Depth here follows the spec's structure -- a refinement `map` substituted
/// into an obligation grows the ITE tree, and an equality multiplies it on both
/// sides -- and costs far more per level than parsing the same expression.
///
/// Measured in #620 by sampling `refine` on `examples/agentic_rag`: 491 `eval`
/// frames under 101 `eval_binary`, 62 `eval_equality_operands`, and 59
/// `ite_value`. That 19-stage mapping overflows a 1 MiB stack here while the
/// parser survives the identical file, which is why per-level cost, not chain
/// length alone, decides which site aborts first.
pub(crate) fn eval<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    eval_in(solver, model, expr, state, bindings, old_state, None)
}

pub(crate) fn eval_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    in_closed_scope(expr, bindings, |scope| {
        eval_with_bindings_in(solver, model, expr, state, scope, old_state, ctx)
    })
}

/// Only matches and conjunctions export to a true continuation. All other
/// constructors close their children, including raw/contextual evaluation.
fn in_closed_scope<T: Clone, R>(
    expr: &Expr,
    bindings: &mut Bindings<T>,
    evaluate: impl FnOnce(&mut Bindings<T>) -> R,
) -> R {
    if matches!(expr, Expr::Is { .. })
        || matches!(expr, Expr::Binary { op, .. } if pattern_binding_flows(op, false))
    {
        evaluate(&mut bindings.clone())
    } else {
        evaluate(bindings)
    }
}

pub(crate) fn eval_with_bindings<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    eval_with_bindings_in(solver, model, expr, state, bindings, old_state, None)
}

pub(crate) fn eval_with_bindings_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    recursion::guard(|| eval_inner(solver, model, expr, state, bindings, old_state, ctx))
}

/// Reaching an RHS (notably through `or`) does not imply binding visibility.
pub(crate) fn confine_binary_rhs<T: Clone>(op: &str, outer: &Bindings<T>, local: &mut Bindings<T>) {
    if !pattern_binding_flows(op, true) {
        local.clone_from(outer);
    }
}

fn binding_types<T>(bindings: &Bindings<T>) -> Vec<(String, TypeRef)> {
    bindings
        .types()
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect()
}

/// Enumeration fixes a literal candidate's value independently of membership.
/// Keep its semantic type, and do not infer facts for opaque terms or enum ordinals.
/// The record for one binder candidate: a literal value, or a member of a state
/// Set whose carrier this read frame certified, which is the prerequisite
/// `fsl_core`'s admission rule states for such a binder. Without the certificate a
/// symbolic candidate has no fact, because an unchecked member may escape the type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn binder_candidate_binding<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    candidate: TypedBinding<S::Term>,
    state: &SymbolicState<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    bindings: &Bindings<S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> BindingRecord<S::Term> {
    let record = enumerated_binder_binding(solver, model, candidate);
    if record.range_fact.is_some() {
        return record;
    }
    match certified_set_element_envelope(model, binder, state, old_state, bindings, ctx) {
        Some(envelope) => {
            BindingRecord::with_range_fact(record.binding, RangeFact::certified(envelope))
        }
        None => record,
    }
}

fn certified_set_element_envelope<T>(
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<T>,
    old_state: Option<&SymbolicState<T>>,
    bindings: &Bindings<T>,
    ctx: Option<&CheckedStateCarriers<'_, T>>,
) -> Option<InclusiveI64> {
    let Binder::Collection { collection, .. } = binder else {
        return None;
    };
    let (frame, name) = fsl_core::binder_range::state_set_source(collection)?;
    if bindings.records().any(|(bound, _)| bound == name) {
        return None;
    }
    let frame = match frame {
        fsl_core::binder_range::SetFrame::Current => state,
        fsl_core::binder_range::SetFrame::Old => old_state?,
    };
    if !ctx?.contains(frame, name) {
        return None;
    }
    fsl_core::binder_range::state_set_element_envelope(model, name)
}

pub(crate) fn enumerated_binder_binding<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binding: TypedBinding<S::Term>,
) -> BindingRecord<S::Term> {
    let integer = match binding.1.as_ref() {
        Some(TypeRef::Int | TypeRef::Range(_, _)) => true,
        Some(TypeRef::Named(name)) => matches!(model.types.get(name), Some(TypeDef::Domain { .. })),
        _ => false,
    };
    let literal = integer
        .then(|| int_term(&binding.0).ok())
        .flatten()
        .and_then(|term| solver.literal_value(term));
    match literal {
        Some(ModelValue::Int(value)) => {
            BindingRecord::with_range_fact(binding, RangeFact::parameter(value))
        }
        _ => binding.into(),
    }
}

#[cfg(test)]
pub(crate) fn eval_binding<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    rhs_defined: Option<&S::Term>,
) -> Result<BindingRecord<S::Term>, VerifyError> {
    eval_binding_in(solver, model, expr, state, bindings, rhs_defined, None)
}

pub(crate) fn eval_binding_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    rhs_defined: Option<&S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<BindingRecord<S::Term>, VerifyError> {
    let ty = model
        .expression_type(expr, &binding_types(bindings), None)?
        .ok_or_else(|| VerifyError::new("binding requires an inferred type"))?;
    let enclosing = bindings.clone();
    let value = eval_in(solver, model, expr, state, bindings, None, ctx)?;
    let binding = TypedBinding(value, Some(ty));
    Ok(
        match capture_let_range_fact(solver, model, expr, state, &enclosing, rhs_defined) {
            Some(fact) => BindingRecord::with_range_fact(binding, fact),
            None => binding.into(),
        },
    )
}

/// Capture eligibility against the original read frame, never assert or consume it.
fn capture_let_range_fact<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    enclosing: &Bindings<S::Term>,
    rhs_defined: Option<&S::Term>,
) -> Option<RangeFact<S::Term>> {
    let records: Vec<_> = enclosing.records().collect();
    let lexical = records
        .iter()
        .map(|(name, record)| {
            Some(RangeBinding {
                name,
                semantic_ty: record.binding.1.as_ref()?,
                fact: record.range_fact.as_ref().map(RangeFact::envelope),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let (envelope, prerequisites) = match model.plan_integer_range(expr, expr, &lexical) {
        RangePlan::Static { lo, hi } => (InclusiveI64::new(lo, hi)?, Vec::new()),
        RangePlan::Finite {
            candidates,
            prerequisites,
        } => (candidates, prerequisites),
        RangePlan::Empty { .. } | RangePlan::Unknown(_) => return None,
    };
    // Required classified status was already computed by the caller. Only the
    // ordinary path adds optional status work, after successful value planning.
    let defined = match rhs_defined {
        Some(defined) => defined.clone(),
        None => {
            evaluation_status(solver, model, expr, state, enclosing, None)
                .ok()?
                .fully_defined
        }
    };
    let mut conditions = vec![defined];
    for prerequisite in prerequisites {
        match prerequisite {
            RangePrerequisite::StateCarrier { name, envelope } => {
                let term = int_term(state.get(&name)?).ok()?;
                conditions.push(
                    solver
                        .and(&[
                            solver.ge(term, &solver.int_value(envelope.lo())).ok()?,
                            solver.le(term, &solver.int_value(envelope.hi())).ok()?,
                        ])
                        .ok()?,
                );
            }
            RangePrerequisite::LexicalFact { input_index } => {
                let fact = records.get(input_index)?.1.range_fact.as_ref()?;
                if let Some(valid_when) = fact.valid_when() {
                    conditions.push(valid_when.clone());
                }
            }
        }
    }
    Some(RangeFact::captured(envelope, solver.and(&conditions).ok()?))
}

fn eval_raw_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    in_closed_scope(expr, bindings, |scope| {
        recursion::guard(|| eval_inner(solver, model, expr, state, scope, old_state, ctx))
    })
}

#[allow(clippy::too_many_lines)]
fn eval_inner<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match expr {
        Expr::Num(value) => Ok(SymbolicValue::Scalar {
            ty: TypeRef::Int,
            term: solver.int_value(*value),
        }),
        Expr::Bool(value) => Ok(SymbolicValue::Scalar {
            ty: TypeRef::Bool,
            term: solver.bool_value(*value),
        }),
        Expr::None => Ok(SymbolicValue::None),
        Expr::Some(_) | Expr::Set(_) | Expr::Seq(_) | Expr::Conditional { .. } => {
            let ty = model
                .expression_type(expr, &binding_types(bindings), None)?
                .ok_or_else(|| VerifyError::new("expression requires an expected type"))?;
            eval_expected_in(solver, model, expr, &ty, state, bindings, old_state, ctx)
        }
        Expr::Struct { name, fields } => {
            eval_struct_literal(solver, model, name, fields, state, bindings, old_state, ctx)
        }
        Expr::Var(name) => lookup(solver, model, name, state, bindings),
        Expr::EnumMember {
            type_name, member, ..
        } => concrete_value(
            solver,
            model,
            &TypeRef::Named(type_name.clone()),
            &FslValue::Enum {
                type_name: type_name.clone(),
                member: member.clone(),
            },
        ),
        Expr::Call { name, .. } => Err(VerifyError::new(format!(
            "unexpanded predicate call '{name}'"
        ))),
        Expr::Stage { .. } => Err(VerifyError::new("unlowered stage access")),
        Expr::Index(base, index) => {
            let base = eval_in(solver, model, base, state, bindings, old_state, ctx)?;
            let index = eval_in(solver, model, index, state, bindings, old_state, ctx)?;
            eval_index(solver, model, &base, &index)
        }
        Expr::Field(base, field) => {
            let base = eval_in(solver, model, base, state, bindings, old_state, ctx)?;
            let SymbolicValue::Struct { fields, .. } = base else {
                return Err(VerifyError::new("field access requires a struct"));
            };
            fields
                .get(field)
                .cloned()
                .ok_or_else(|| VerifyError::new(format!("unknown struct field '{field}'")))
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver = eval_in(solver, model, receiver, state, bindings, old_state, ctx)?;
            eval_method(
                solver, model, receiver, name, args, state, bindings, old_state, ctx,
            )
        }
        Expr::Binary { op, left, right } => eval_binary(
            solver, model, op, left, right, state, bindings, old_state, ctx,
        ),
        Expr::Neg(inner) => {
            let value = eval_in(solver, model, inner, state, bindings, old_state, ctx)?;
            Ok(SymbolicValue::Scalar {
                ty: TypeRef::Int,
                term: solver.neg(int_term(&value)?)?,
            })
        }
        Expr::Not(inner) => {
            let value = eval_in(solver, model, inner, state, bindings, old_state, ctx)?;
            Ok(bool_value(solver, solver.not(bool_term(&value)?)?))
        }
        Expr::Is { expr, pattern } => {
            let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
            eval_pattern(solver, model, value, pattern, state, bindings)
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => eval_quantified(
            solver, model, quantifier, binder, body, state, bindings, old_state, None, ctx,
        ),
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => eval_aggregate(
            solver,
            model,
            *kind,
            binder,
            value.as_deref(),
            state,
            bindings,
            old_state,
            ctx,
        ),
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => eval_in(
                solver,
                model,
                expr,
                old_state.ok_or_else(|| VerifyError::new("old() used without old state"))?,
                bindings,
                None,
                ctx,
            ),
            "abs" => {
                let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
                let term = int_term(&value)?;
                let nonnegative = solver.ge(term, &solver.int_value(0))?;
                Ok(int_value(
                    solver,
                    solver.ite(&nonnegative, term, &solver.neg(term)?)?,
                ))
            }
            "rel_functional" | "rel_injective" => {
                let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
                let (_, entries) = collection_entries(solver, model, &value)?;
                let (shared, distinct) = if name == "rel_functional" {
                    (0, 1)
                } else {
                    (1, 0)
                };
                let mut clauses = Vec::new();
                for (index, (key, present)) in entries.iter().enumerate() {
                    for (other_key, other_present) in entries.iter().skip(index + 1) {
                        let same = logical_equal(solver, model, &key[shared], &other_key[shared])?;
                        let different = solver.not(&logical_equal(
                            solver,
                            model,
                            &key[distinct],
                            &other_key[distinct],
                        )?)?;
                        clauses.push(solver.not(&solver.and(&[
                            same,
                            different,
                            present.clone(),
                            other_present.clone(),
                        ])?)?);
                    }
                }
                Ok(bool_value(solver, solver.and(&clauses)?))
            }
            "rel_domain" | "rel_range" => {
                let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
                let (ty, entries) = collection_entries(solver, model, &value)?;
                let TypeRef::Relation(source_ty, target_ty) = &ty else {
                    unreachable!();
                };
                let (element_ty, endpoint) = if name == "rel_domain" {
                    (source_ty.as_ref().clone(), 0)
                } else {
                    (target_ty.as_ref().clone(), 1)
                };
                let entries = distinct_entries(
                    solver,
                    model,
                    entries
                        .into_iter()
                        .map(|(key, present)| (vec![key[endpoint].clone()], present))
                        .collect(),
                )?;
                Ok(SymbolicValue::Collection {
                    ty: TypeRef::Set(Box::new(element_ty)),
                    entries,
                })
            }
            "rel_acyclic" => {
                let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
                let (ty, entries) = collection_entries(solver, model, &value)?;
                let TypeRef::Relation(source_ty, target_ty) = &ty else {
                    unreachable!();
                };
                if source_ty != target_ty {
                    return Err(VerifyError::new(
                        "acyclic() requires a self-relation (relation T -> T)",
                    ));
                }
                let reach = relation_reachability_table(solver, model, source_ty, &entries)?;
                let mut cyclic_terms = Vec::new();
                for (index, _) in reach.vertices.iter().enumerate() {
                    cyclic_terms.push(reach.paths[index][index].clone());
                }
                let cyclic = solver.or(&cyclic_terms)?;
                Ok(bool_value(solver, solver.not(&cyclic)?))
            }
            _ => Err(VerifyError::new(format!(
                "unsupported unary expression '{name}'"
            ))),
        },
        Expr::BinaryNamed { name, left, right } => {
            let left = eval_in(solver, model, left, state, bindings, old_state, ctx)?;
            let right = eval_in(solver, model, right, state, bindings, old_state, ctx)?;
            let condition = match name.as_str() {
                "min" => solver.le(int_term(&left)?, int_term(&right)?)?,
                "max" => solver.ge(int_term(&left)?, int_term(&right)?)?,
                _ => {
                    return Err(VerifyError::new(format!(
                        "unsupported binary expression '{name}'"
                    )));
                }
            };
            Ok(int_value(
                solver,
                solver.ite(&condition, int_term(&left)?, int_term(&right)?)?,
            ))
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } if name == "rel_reachable" => {
            let relation = eval_in(solver, model, first, state, bindings, old_state, ctx)?;
            let (ty, entries) = collection_entries(solver, model, &relation)?;
            let TypeRef::Relation(source_ty, target_ty) = &ty else {
                unreachable!();
            };
            if source_ty != target_ty {
                return Err(VerifyError::new(
                    "reachable() requires a self-relation (relation T -> T)",
                ));
            }
            let source = eval_in(solver, model, second, state, bindings, old_state, ctx)?;
            let target = eval_in(solver, model, third, state, bindings, old_state, ctx)?;
            let reach = relation_reachability_table(solver, model, source_ty, &entries)?;
            let mut terms = Vec::new();
            for (from_index, from) in reach.vertices.iter().enumerate() {
                for (to_index, to) in reach.vertices.iter().enumerate() {
                    let from = SymbolicValue::Scalar {
                        ty: source_ty.as_ref().clone(),
                        term: from.clone(),
                    };
                    let to = SymbolicValue::Scalar {
                        ty: target_ty.as_ref().clone(),
                        term: to.clone(),
                    };
                    let same_source = logical_equal(solver, model, &source, &from)?;
                    let same_target = logical_equal(solver, model, &target, &to)?;
                    terms.push(solver.and(&[
                        same_source,
                        same_target,
                        reach.paths[from_index][to_index].clone(),
                    ])?);
                }
            }
            Ok(bool_value(solver, solver.or(&terms)?))
        }
        Expr::TernaryNamed { name, .. } => Err(VerifyError::new(format!(
            "unsupported ternary expression '{name}'"
        ))),
    }
}

struct RelationReachability<T> {
    vertices: Vec<T>,
    paths: Vec<Vec<T>>,
}

/// Positive-length path closure, including actual out-of-carrier endpoints.
fn relation_reachability_table<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    domain_ty: &TypeRef,
    entries: &crate::value::CollectionEntries<S::Term>,
) -> Result<RelationReachability<S::Term>, VerifyError> {
    let values = model.domain_values(domain_ty)?;
    let endpoint = |value| match value {
        SymbolicValue::Scalar { term, .. } => Ok(term),
        _ => Err(VerifyError::new("relation endpoints must be scalar")),
    };
    let mut vertices: Vec<_> = values
        .iter()
        .map(|value| endpoint(concrete_value(solver, model, domain_ty, value)?))
        .collect::<Result<_, VerifyError>>()?;
    // Every relation keeps the finite pair grid prefix. Only appended slots
    // introduce candidate vertices; duplicating all grid endpoints is quadratic.
    vertices.extend(
        entries
            .iter()
            .skip(values.len() * values.len())
            .flat_map(|(key, _)| key.iter().cloned())
            .map(endpoint)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut reach = vertices
        .iter()
        .map(|source| {
            vertices
                .iter()
                .map(|target| {
                    let endpoint = |term: &S::Term| SymbolicValue::Scalar {
                        ty: domain_ty.clone(),
                        term: term.clone(),
                    };
                    collection_contains(
                        solver,
                        model,
                        entries,
                        &[endpoint(source), endpoint(target)],
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Warshall closure over actual candidate endpoints, not just the carrier.
    // As before, reachability requires at least one edge.
    for via in 0..vertices.len() {
        for source in 0..vertices.len() {
            for target in 0..vertices.len() {
                reach[source][target] = solver.or(&[
                    reach[source][target].clone(),
                    solver.and(&[reach[source][via].clone(), reach[via][target].clone()])?,
                ])?;
            }
        }
    }
    Ok(RelationReachability {
        vertices,
        paths: reach,
    })
}

/// Build the condition under which concrete evaluation of `expr` completes
/// without a partial operation, checked integer overflow, or invalid finite
/// lookup. Unlike [`eval`], this preserves native short-circuit and conditional
/// evaluation paths.
pub(crate) fn definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<S::Term, VerifyError> {
    Ok(evaluation_status(solver, model, expr, state, bindings, old_state)?.fully_defined)
}

#[derive(Clone, Debug)]
pub(crate) struct EvaluationStatus<T> {
    pub fully_defined: T,
    pub first_partial: T,
    pub has_partial_operation: bool,
}

/// Which failure an [`EvaluationStatus`]'s `first_partial` classifies: the
/// condition that evaluation completes (`fully_defined`) is the same for
/// every class, but `first_partial` (and `has_partial_operation`) answer
/// "is the first reached failure one of this class?".
///
/// The six partial operations of LANGUAGE.md §6 are what BMC and the explicit
/// engine report as `partial_op`, and what every caller asked before #1221.
/// A finite `Map` key outside its domain is the class the k-induction step
/// obligation asks separately (#1221). Checked i64 overflow, the remaining
/// non-partial failure, is deliberately not a class here: the induction step
/// obligation does not ask it (#1263, `docs/design/DESIGN-induction.md`
/// §2.6); a class for it would be added next to `KeyDomain`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailureClass {
    /// LANGUAGE.md §6: `head`/`pop`/`at`, a `Seq` index, `/` and `%` (the
    /// last two in action context only).
    PartialOperation,
    /// Attribute a reached partial failure without changing evaluation order
    /// or the definedness of operations outside this selected kind.
    PartialOperationKind(fsl_core::PartialOperation),
    /// A `Map` index (read or indexed assignment) outside the map's finite
    /// key domain.
    KeyDomain,
}

impl FailureClass {
    fn classifies_partial(self, operation: Option<fsl_core::PartialOperation>) -> bool {
        self == Self::PartialOperation
            || matches!(self, Self::PartialOperationKind(selected) if Some(selected) == operation)
    }
}

#[derive(Clone, Copy)]
struct EvaluationPolicy {
    total_division: bool,
    classify: FailureClass,
}

const ACTION_EVALUATION: EvaluationPolicy = EvaluationPolicy {
    total_division: false,
    classify: FailureClass::PartialOperation,
};
const PROPERTY_EVALUATION: EvaluationPolicy = EvaluationPolicy {
    total_division: true,
    classify: FailureClass::PartialOperation,
};

pub(crate) fn safe_status<S: SmtSolver>(solver: &S) -> EvaluationStatus<S::Term> {
    EvaluationStatus {
        fully_defined: solver.bool_value(true),
        first_partial: solver.bool_value(false),
        has_partial_operation: false,
    }
}

pub(crate) fn sequence_statuses<S: SmtSolver>(
    solver: &S,
    statuses: impl IntoIterator<Item = EvaluationStatus<S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let mut fully_defined = solver.bool_value(true);
    let mut first_partial = solver.bool_value(false);
    let mut has_partial_operation = false;
    for status in statuses {
        has_partial_operation |= status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[fully_defined.clone(), status.first_partial])?,
        ])?;
        fully_defined = solver.and(&[fully_defined, status.fully_defined])?;
    }
    Ok(EvaluationStatus {
        fully_defined,
        first_partial,
        has_partial_operation,
    })
}

/// Build the path-sensitive conditions that concrete evaluation completes and
/// that its first reached failure is one of LANGUAGE.md §6's six classified
/// partial operations.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    evaluation_status_with_policy(
        solver,
        model,
        expr,
        state,
        bindings,
        old_state,
        ACTION_EVALUATION,
        None,
    )
}

/// [`evaluation_status`] (action context: `/` and `%` are partial) with
/// `first_partial` classifying `class` instead of the partial operations.
pub(crate) fn classified_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    class: FailureClass,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    classified_evaluation_status_in(solver, model, expr, state, bindings, old_state, class, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn classified_evaluation_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    evaluation_status_with_policy(
        solver,
        model,
        expr,
        state,
        bindings,
        old_state,
        EvaluationPolicy {
            classify: class,
            ..ACTION_EVALUATION
        },
        ctx,
    )
}

/// [`property_evaluation_status`] (property context: `/` and `%` are total)
/// with `first_partial` classifying `class`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn classified_property_evaluation_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    evaluation_status_with_policy(
        solver,
        model,
        expr,
        state,
        bindings,
        old_state,
        EvaluationPolicy {
            classify: class,
            ..PROPERTY_EVALUATION
        },
        ctx,
    )
}

pub(crate) fn property_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    property_evaluation_status_in(solver, model, expr, state, bindings, old_state, None)
}

pub(crate) fn property_evaluation_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    evaluation_status_with_policy(
        solver,
        model,
        expr,
        state,
        bindings,
        old_state,
        PROPERTY_EVALUATION,
        ctx,
    )
}

/// Definedness analysis of one kernel expression, and a second cycle entry
/// for this module's stack guard (#1164): every arm below, and the
/// `ordered_evaluation_status_refs` / binder helpers, re-enter here for their
/// operands. It walks the same user-controlled tree as [`eval`] but is not
/// dominated by it -- `eval`'s guard runs inside the calls this function makes,
/// not around its own recursion -- so it needs its own guard.
///
/// Crash-witnessed by `verify` (bmc and induction) on a right-nested `if`
/// chain inside an `invariant`: unguarded, a debug aarch64 build survives
/// N=300 and aborts at N=320, and at N=500 all ~320 frames on the stack are
/// this function (~25 KiB per level), reached through
/// `property_evaluation_status` from `bmc::check_state_properties`.
#[allow(clippy::too_many_arguments)]
fn evaluation_status_with_policy<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    recursion::guard(|| {
        evaluation_status_with_policy_inner(
            solver, model, expr, state, bindings, old_state, policy, ctx,
        )
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn evaluation_status_with_policy_inner<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {
            Ok(safe_status(solver))
        }
        Expr::UnaryNamed {
            name, expr: inner, ..
        } if name == "old" => evaluation_status_with_policy(
            solver,
            model,
            inner,
            old_state.ok_or_else(|| VerifyError::new("old() used without old state"))?,
            bindings,
            None,
            policy,
            ctx,
        ),
        Expr::Neg(inner) => {
            let mut local = bindings.clone();
            let inner_status = evaluation_status_with_policy(
                solver, model, inner, state, &local, old_state, policy, ctx,
            )?;
            let inner = eval_in(solver, model, inner, state, &mut local, old_state, ctx)?;
            sequence_statuses(
                solver,
                [
                    inner_status,
                    EvaluationStatus {
                        fully_defined: solver
                            .not(&solver.equal(int_term(&inner)?, &solver.int_value(i64::MIN))?)?,
                        first_partial: solver.bool_value(false),
                        has_partial_operation: false,
                    },
                ],
            )
        }
        Expr::UnaryNamed {
            name, expr: inner, ..
        } if name == "abs" => {
            let mut local = bindings.clone();
            let inner_status = evaluation_status_with_policy(
                solver, model, inner, state, &local, old_state, policy, ctx,
            )?;
            let inner = eval_in(solver, model, inner, state, &mut local, old_state, ctx)?;
            sequence_statuses(
                solver,
                [
                    inner_status,
                    EvaluationStatus {
                        fully_defined: solver
                            .not(&solver.equal(int_term(&inner)?, &solver.int_value(i64::MIN))?)?,
                        first_partial: solver.bool_value(false),
                        has_partial_operation: false,
                    },
                ],
            )
        }
        Expr::Some(inner)
        | Expr::Not(inner)
        | Expr::Field(inner, _)
        | Expr::Is { expr: inner, .. }
        | Expr::Stage { entity: inner, .. }
        | Expr::UnaryNamed { expr: inner, .. } => evaluation_status_with_policy(
            solver, model, inner, state, bindings, old_state, policy, ctx,
        ),
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            ordered_evaluation_status(
                solver, model, items, state, bindings, old_state, policy, ctx,
            )
        }
        Expr::Struct { name, fields } => {
            let TypeDef::Struct {
                fields: expected, ..
            } = model
                .types
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("unknown struct type '{name}'")))?
            else {
                return Err(VerifyError::new(format!("'{name}' is not a struct")));
            };
            let expressions = fields
                .iter()
                .map(|(field, expression)| (field.as_str(), expression))
                .collect::<BTreeMap<_, _>>();
            let items = expected
                .iter()
                .map(|(field, _)| {
                    expressions
                        .get(field.as_str())
                        .copied()
                        .ok_or_else(|| VerifyError::new(format!("missing struct field '{field}'")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if expressions.len() != expected.len() {
                return Err(VerifyError::new(format!(
                    "struct '{name}' has the wrong number of fields"
                )));
            }
            ordered_evaluation_status_refs(
                solver, model, &items, state, bindings, old_state, policy, ctx,
            )
        }
        Expr::Index(base, index) => {
            let mut local = bindings.clone();
            let base_status = evaluation_status_with_policy(
                solver, model, base, state, &local, old_state, policy, ctx,
            )?;
            let base_value = eval_in(solver, model, base, state, &mut local, old_state, ctx)?;
            let index_status = evaluation_status_with_policy(
                solver, model, index, state, &local, old_state, policy, ctx,
            )?;
            let index_value = eval_in(solver, model, index, state, &mut local, old_state, ctx)?;
            let fully_accessible = index_accessible(solver, model, &base_value, &index_value)?;
            let access = index_access_status(
                solver,
                &base_value,
                &index_value,
                fully_accessible,
                policy.classify,
            )?;
            sequence_statuses(solver, [base_status, index_status, access])
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let mut local = bindings.clone();
            let receiver_status = evaluation_status_with_policy(
                solver, model, receiver, state, &local, old_state, policy, ctx,
            )?;
            let receiver_value =
                eval_in(solver, model, receiver, state, &mut local, old_state, ctx)?;
            let arguments_status = ordered_evaluation_status(
                solver, model, args, state, &local, old_state, policy, ctx,
            )?;
            let argument_values = eval_method_arguments(
                solver,
                model,
                &receiver_value,
                name,
                args,
                state,
                &mut local,
                old_state,
                ctx,
            )?;
            let operation_defined =
                method_definedness(solver, name, &receiver_value, argument_values.as_slice())?;
            let classified = policy
                .classify
                .classifies_partial(fsl_core::PartialOperation::candidate(expr));
            sequence_statuses(
                solver,
                [
                    receiver_status,
                    arguments_status,
                    EvaluationStatus {
                        fully_defined: operation_defined.clone(),
                        first_partial: if classified {
                            solver.not(&operation_defined)?
                        } else {
                            solver.bool_value(false)
                        },
                        has_partial_operation: classified
                            && matches!(
                                (&receiver_value, name.as_str(), argument_values.as_slice()),
                                (SymbolicValue::Seq { .. }, "head" | "pop", [])
                                    | (SymbolicValue::Seq { .. }, "at", [_])
                            ),
                    },
                ],
            )
        }
        Expr::Binary { op, left, right } => {
            let mut local = bindings.clone();
            let left_status = evaluation_status_with_policy(
                solver, model, left, state, &local, old_state, policy, ctx,
            )?;
            let reached_right = match op.as_str() {
                "and" | "=>" => {
                    let left_value = eval_with_bindings_in(
                        solver, model, left, state, &mut local, old_state, ctx,
                    )?;
                    bool_term(&left_value)?.clone()
                }
                "or" => {
                    let left_value = eval_with_bindings_in(
                        solver, model, left, state, &mut local, old_state, ctx,
                    )?;
                    solver.not(bool_term(&left_value)?)?
                }
                _ => solver.bool_value(true),
            };
            confine_binary_rhs(op, bindings, &mut local);
            let right_status = evaluation_status_with_policy(
                solver, model, right, state, &local, old_state, policy, ctx,
            )?;
            let first_partial = solver.or(&[
                left_status.first_partial,
                solver.and(&[
                    left_status.fully_defined.clone(),
                    reached_right.clone(),
                    right_status.first_partial,
                ])?,
            ])?;
            let operands_defined = solver.and(&[
                left_status.fully_defined,
                solver.implies(&reached_right, &right_status.fully_defined)?,
            ])?;
            let mut parts = vec![operands_defined.clone()];
            let mut operation_partial = solver.bool_value(false);
            if matches!(op.as_str(), "/" | "%") {
                let left_value = eval_in(solver, model, left, state, &mut local, old_state, ctx)?;
                let right_value = eval_in(solver, model, right, state, &mut local, old_state, ctx)?;
                let zero = solver.equal(int_term(&right_value)?, &solver.int_value(0))?;
                if !policy.total_division {
                    parts.push(solver.not(&zero)?);
                    if policy
                        .classify
                        .classifies_partial(fsl_core::PartialOperation::candidate(expr))
                    {
                        operation_partial = solver.and(&[operands_defined.clone(), zero])?;
                    }
                }
                let overflow = solver.and(&[
                    solver.equal(int_term(&left_value)?, &solver.int_value(i64::MIN))?,
                    solver.equal(int_term(&right_value)?, &solver.int_value(-1))?,
                ])?;
                parts.push(solver.not(&overflow)?);
            } else if matches!(op.as_str(), "+" | "-" | "*") {
                let left_value = eval_in(solver, model, left, state, &mut local, old_state, ctx)?;
                let right_value = eval_in(solver, model, right, state, &mut local, old_state, ctx)?;
                let result = match op.as_str() {
                    "+" => solver.add(int_term(&left_value)?, int_term(&right_value)?)?,
                    "-" => solver.sub(int_term(&left_value)?, int_term(&right_value)?)?,
                    "*" => solver.mul(int_term(&left_value)?, int_term(&right_value)?)?,
                    _ => unreachable!(),
                };
                parts.push(i64_term_is_in_range(solver, &result)?);
            }
            Ok(EvaluationStatus {
                fully_defined: solver.and(&parts)?,
                first_partial: solver.or(&[first_partial, operation_partial])?,
                has_partial_operation: left_status.has_partial_operation
                    || right_status.has_partial_operation
                    || (matches!(op.as_str(), "/" | "%")
                        && !policy.total_division
                        && policy
                            .classify
                            .classifies_partial(fsl_core::PartialOperation::candidate(expr))),
            })
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            let mut local = bindings.clone();
            let condition_status = evaluation_status_with_policy(
                solver, model, condition, state, &local, old_state, policy, ctx,
            )?;
            let condition_value =
                eval_in(solver, model, condition, state, &mut local, old_state, ctx)?;
            let then_status = evaluation_status_with_policy(
                solver, model, then_expr, state, &local, old_state, policy, ctx,
            )?;
            let else_status = evaluation_status_with_policy(
                solver, model, else_expr, state, &local, old_state, policy, ctx,
            )?;
            let branch_defined = solver.ite(
                bool_term(&condition_value)?,
                &then_status.fully_defined,
                &else_status.fully_defined,
            )?;
            let branch_partial = solver.ite(
                bool_term(&condition_value)?,
                &then_status.first_partial,
                &else_status.first_partial,
            )?;
            Ok(EvaluationStatus {
                fully_defined: solver
                    .and(&[condition_status.fully_defined.clone(), branch_defined])?,
                first_partial: solver.or(&[
                    condition_status.first_partial,
                    solver.and(&[condition_status.fully_defined, branch_partial])?,
                ])?,
                has_partial_operation: condition_status.has_partial_operation
                    || then_status.has_partial_operation
                    || else_status.has_partial_operation,
            })
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => quantified_evaluation_status(
            solver, model, quantifier, binder, body, state, bindings, old_state, policy, None, ctx,
        ),
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => aggregate_evaluation_status(
            solver,
            model,
            *kind,
            binder,
            value.as_deref(),
            state,
            bindings,
            old_state,
            policy,
            ctx,
        ),
        Expr::BinaryNamed { left, right, .. } => ordered_evaluation_status_refs(
            solver,
            model,
            &[left.as_ref(), right.as_ref()],
            state,
            bindings,
            old_state,
            policy,
            ctx,
        ),
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => ordered_evaluation_status_refs(
            solver,
            model,
            &[first.as_ref(), second.as_ref(), third.as_ref()],
            state,
            bindings,
            old_state,
            policy,
            ctx,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn ordered_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[Expr],
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let expressions = expressions.iter().collect::<Vec<_>>();
    ordered_evaluation_status_refs(
        solver,
        model,
        &expressions,
        state,
        bindings,
        old_state,
        policy,
        ctx,
    )
}

#[allow(clippy::too_many_arguments)]
fn ordered_evaluation_status_refs<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[&Expr],
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let local = bindings.clone();
    let mut statuses = Vec::new();
    for expression in expressions {
        statuses.push(evaluation_status_with_policy(
            solver, model, expression, state, &local, old_state, policy, ctx,
        )?);
    }
    sequence_statuses(solver, statuses)
}

/// The status of one index access (`base[index]`, read or indexed
/// assignment) whose completion condition is `fully_accessible`
/// ([`index_accessible`]), with `first_partial` classifying `class`: a `Seq`
/// index outside the live prefix is a partial operation, a `Map` index
/// outside the finite key domain is a key-domain miss.
pub(crate) fn index_access_status<S: SmtSolver>(
    solver: &S,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
    fully_accessible: S::Term,
    class: FailureClass,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    Ok(match (class, base) {
        (FailureClass::PartialOperation | FailureClass::PartialOperationKind(_), _) => {
            let partial_accessible = partial_operation_index_accessible(solver, base, index)?;
            let classified = class.classifies_partial(Some(fsl_core::PartialOperation::Index));
            EvaluationStatus {
                fully_defined: fully_accessible,
                first_partial: if classified {
                    solver.not(&partial_accessible)?
                } else {
                    solver.bool_value(false)
                },
                has_partial_operation: classified && matches!(base, SymbolicValue::Seq { .. }),
            }
        }
        (FailureClass::KeyDomain, SymbolicValue::Map { .. }) => EvaluationStatus {
            first_partial: solver.not(&fully_accessible)?,
            fully_defined: fully_accessible,
            has_partial_operation: true,
        },
        (FailureClass::KeyDomain, _) => EvaluationStatus {
            fully_defined: fully_accessible,
            first_partial: solver.bool_value(false),
            has_partial_operation: false,
        },
    })
}

pub(crate) fn partial_operation_index_accessible<S: SmtSolver>(
    solver: &S,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
) -> Result<S::Term, VerifyError> {
    match base {
        SymbolicValue::Seq { len, .. } => Ok(solver.and(&[
            solver.ge(int_term(index)?, &solver.int_value(0))?,
            solver.lt(int_term(index)?, len)?,
        ])?),
        _ => Ok(solver.bool_value(true)),
    }
}

pub(crate) fn index_accessible<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
) -> Result<S::Term, VerifyError> {
    match base {
        SymbolicValue::Map { ty, entries } => {
            let TypeRef::Map(key_ty, _) = ty else {
                unreachable!();
            };
            let terms = entries
                .iter()
                .map(|(key, _)| {
                    let key = concrete_value(solver, model, key_ty, key)?;
                    logical_equal(solver, model, index, &key)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(solver.or(&terms)?)
        }
        SymbolicValue::Seq { len, .. } => Ok(solver.and(&[
            solver.ge(int_term(index)?, &solver.int_value(0))?,
            solver.lt(int_term(index)?, len)?,
        ])?),
        _ => Err(VerifyError::new("indexing requires a map or sequence")),
    }
}

pub(crate) fn method_definedness<S: SmtSolver>(
    solver: &S,
    name: &str,
    receiver: &SymbolicValue<S::Term>,
    arguments: &[SymbolicValue<S::Term>],
) -> Result<S::Term, VerifyError> {
    let SymbolicValue::Seq { len, .. } = receiver else {
        return Ok(solver.bool_value(true));
    };
    match (name, arguments) {
        ("head" | "pop", []) => solver
            .gt(len, &solver.int_value(0))
            .map_err(VerifyError::from),
        ("at", [index]) => Ok(solver.and(&[
            solver.ge(int_term(index)?, &solver.int_value(0))?,
            solver.lt(int_term(index)?, len)?,
        ])?),
        _ => Ok(solver.bool_value(true)),
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn quantified_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    binder: &Binder,
    body: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    prepared: Option<&DischargedIntegerRange<'_, S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status = match prepared {
        Some(prepared) => prepared.draft().source_status.clone(),
        None => binder_source_evaluation_status(
            solver, model, binder, state, bindings, old_state, policy, ctx,
        )?,
    };
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    let candidates = if prepared.is_none() {
        binder_values_in(solver, model, binder, state, bindings, old_state, ctx)?
    } else {
        Vec::new()
    };
    for (name, value, membership) in
        candidates
            .into_iter()
            .chain(prepared.into_iter().flat_map(|p| {
                p.candidates()
                    .iter()
                    .map(|(n, v, m)| (n.clone(), v.clone(), Some(m.clone())))
            }))
    {
        let mut local = bindings.clone();
        let record = binder_candidate_binding(
            solver, model, binder, value, state, old_state, bindings, ctx,
        );
        local.insert(name, record);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| {
                evaluation_status_with_policy(
                    solver, model, where_expr, state, &local, old_state, policy, ctx,
                )
            },
        )?;
        let where_reached = solver.and(&[active.clone(), membership.clone()])?;
        has_partial_operation |= where_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[where_reached.clone(), where_status.first_partial])?,
        ])?;
        let where_term = binder_where_in(solver, model, binder, state, &mut local, old_state, ctx)?
            .unwrap_or_else(|| solver.bool_value(true));
        let body_reached = solver.and(&[
            where_reached.clone(),
            where_status.fully_defined.clone(),
            where_term.clone(),
        ])?;
        let body_status = evaluation_status_with_policy(
            solver, model, body, state, &local, old_state, policy, ctx,
        )?;
        has_partial_operation |= body_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[body_reached.clone(), body_status.first_partial])?,
        ])?;
        let where_ok = solver.implies(&where_reached, &where_status.fully_defined)?;
        let body_ok = solver.implies(&body_reached, &body_status.fully_defined)?;
        fully_defined = solver.and(&[fully_defined, where_ok, body_ok])?;

        let body_value = eval_in(solver, model, body, state, &mut local, old_state, ctx)?;
        let body_continues = if quantifier == "forall" {
            bool_term(&body_value)?.clone()
        } else {
            solver.not(bool_term(&body_value)?)?
        };
        let candidate_continues = solver.or(&[
            solver.not(&membership)?,
            solver.and(&[
                where_status.fully_defined,
                solver.or(&[
                    solver.not(&where_term)?,
                    solver.and(&[body_status.fully_defined, body_continues])?,
                ])?,
            ])?,
        ])?;
        active = solver.and(&[active, candidate_continues])?;
    }
    Ok(EvaluationStatus {
        fully_defined,
        first_partial,
        has_partial_operation,
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn aggregate_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    kind: AggregateKind,
    binder: &Binder,
    value: Option<&Expr>,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status = binder_source_evaluation_status(
        solver, model, binder, state, bindings, old_state, policy, ctx,
    )?;
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    let mut sum = solver.int_value(0);
    for (name, candidate, membership) in
        binder_values_in(solver, model, binder, state, bindings, old_state, ctx)?
    {
        let mut local = bindings.clone();
        let record = binder_candidate_binding(
            solver, model, binder, candidate, state, old_state, bindings, ctx,
        );
        local.insert(name, record);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| {
                evaluation_status_with_policy(
                    solver, model, where_expr, state, &local, old_state, policy, ctx,
                )
            },
        )?;
        let where_reached = solver.and(&[active.clone(), membership])?;
        has_partial_operation |= where_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[where_reached.clone(), where_status.first_partial])?,
        ])?;
        let where_term = binder_where_in(solver, model, binder, state, &mut local, old_state, ctx)?
            .unwrap_or_else(|| solver.bool_value(true));
        let mut iteration_ok = solver.implies(&where_reached, &where_status.fully_defined)?;
        if let Some(value) = value {
            let value_reached =
                solver.and(&[where_reached, where_status.fully_defined, where_term])?;
            let value_status = evaluation_status_with_policy(
                solver, model, value, state, &local, old_state, policy, ctx,
            )?;
            has_partial_operation |= value_status.has_partial_operation;
            first_partial = solver.or(&[
                first_partial,
                solver.and(&[value_reached.clone(), value_status.first_partial])?,
            ])?;
            iteration_ok = solver.and(&[
                iteration_ok,
                solver.implies(&value_reached, &value_status.fully_defined)?,
            ])?;
            if kind == AggregateKind::Sum {
                let value = eval_in(solver, model, value, state, &mut local, old_state, ctx)?;
                let next_sum = solver.add(&sum, int_term(&value)?)?;
                let sum_reached =
                    solver.and(&[value_reached.clone(), value_status.fully_defined])?;
                iteration_ok = solver.and(&[
                    iteration_ok,
                    solver.implies(&sum_reached, &i64_term_is_in_range(solver, &next_sum)?)?,
                ])?;
                sum = solver.ite(&value_reached, &next_sum, &sum)?;
            }
        }
        fully_defined = solver.and(&[fully_defined, iteration_ok.clone()])?;
        active = solver.and(&[active, iteration_ok])?;
    }
    Ok(EvaluationStatus {
        fully_defined,
        first_partial,
        has_partial_operation,
    })
}

pub(crate) fn i64_term_is_in_range<S: SmtSolver>(
    solver: &S,
    term: &S::Term,
) -> Result<S::Term, VerifyError> {
    Ok(solver.and(&[
        solver.ge(term, &solver.int_value(i64::MIN))?,
        solver.le(term, &solver.int_value(i64::MAX))?,
    ])?)
}

#[allow(clippy::too_many_arguments)]
fn binder_source_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    ordered_evaluation_status_refs(
        solver,
        model,
        &binder_source_expressions(binder),
        state,
        bindings,
        old_state,
        policy,
        ctx,
    )
}

pub(crate) fn binder_source_expressions(binder: &Binder) -> Vec<&Expr> {
    match binder {
        Binder::Typed { .. } => Vec::new(),
        Binder::Range { lo, hi, .. } => vec![lo, hi],
        Binder::Collection { collection, .. } => vec![collection],
    }
}

pub(crate) fn binder_where_expression(binder: &Binder) -> Option<&Expr> {
    match binder {
        Binder::Typed { where_expr, .. }
        | Binder::Range { where_expr, .. }
        | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_struct_literal<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    fields: &[(String, Expr)],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let Some(TypeDef::Struct { fields: expected }) = model.types.get(name) else {
        return Err(VerifyError::new(format!("unknown struct type '{name}'")));
    };
    let expressions = fields.iter().cloned().collect::<BTreeMap<_, _>>();
    Ok(SymbolicValue::Struct {
        ty: TypeRef::Named(name.to_owned()),
        fields: expected
            .iter()
            .map(|(field, ty)| {
                let expr = expressions
                    .get(field)
                    .ok_or_else(|| VerifyError::new(format!("missing struct field '{field}'")))?;
                let value =
                    eval_expected_in(solver, model, expr, ty, state, bindings, old_state, ctx)?;
                Ok((field.clone(), value))
            })
            .collect::<Result<_, VerifyError>>()?,
    })
}

fn lookup<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if let Some(value) = bindings.get(name).or_else(|| state.get(name)) {
        return Ok(value.clone());
    }
    if let Some(value) = model.consts.get(name) {
        let ty = match value {
            FslValue::Int(_) => TypeRef::Int,
            FslValue::Bool(_) => TypeRef::Bool,
            _ => return Err(VerifyError::new(format!("unsupported const '{name}'"))),
        };
        return concrete_value(solver, model, &ty, value);
    }
    if let Some(value @ FslValue::Enum { type_name, .. }) = model.enum_members.get(name) {
        return concrete_value(solver, model, &TypeRef::Named(type_name.clone()), value);
    }
    Err(VerifyError::new(format!("unknown identifier '{name}'")))
}

fn eval_index<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match base {
        SymbolicValue::Map { ty, entries } => {
            let TypeRef::Map(key_ty, _) = ty else {
                unreachable!();
            };
            select_finite(solver, model, entries, index, key_ty)
        }
        SymbolicValue::Seq { ty, slots, .. } => {
            eval_seq_read(solver, model, ty, slots, Some(index))
        }
        _ => Err(VerifyError::new("indexing requires a map or sequence")),
    }
}

fn eval_seq_read<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
    slots: &[SymbolicValue<S::Term>],
    index: Option<&SymbolicValue<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    static NEXT_SLOT: AtomicU64 = AtomicU64::new(0);
    if slots.is_empty() {
        let TypeRef::Seq(element_ty, _) = ty else {
            unreachable!();
        };
        // A capacity-zero Seq still needs an unobservable read term, just like
        // an empty live prefix at positive capacity. Materialize it lazily so
        // stored slots, length bounds, and rendered state keep capacity zero.
        // Definedness, not this unconstrained value, decides partial_op.
        let id = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
        return Ok(SymbolicValue::Scalar {
            ty: element_ty.as_ref().clone(),
            term: solver.constant(
                &format!("@seq_unobserved_slot@{id}"),
                &solver_sort(model, element_ty)?,
            )?,
        });
    }
    let Some(index) = index else {
        return Ok(slots[0].clone());
    };
    let entries = slots
        .iter()
        .enumerate()
        .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
        .collect::<Result<Vec<_>, VerifyError>>()?;
    select_finite(solver, model, &entries, index, &TypeRef::Int)
}

/// Checked method argument construction is identical for values and status.
#[allow(clippy::too_many_arguments)]
fn eval_method_arguments<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    receiver: &SymbolicValue<S::Term>,
    name: &str,
    args: &[Expr],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<Vec<SymbolicValue<S::Term>>, VerifyError> {
    let argument_types = match receiver.ty() {
        Some(TypeRef::Set(element)) => vec![element.as_ref()],
        Some(TypeRef::Relation(source, target)) => vec![source.as_ref(), target.as_ref()],
        Some(TypeRef::Seq(element, _)) if matches!(name, "contains" | "push") => {
            vec![element.as_ref()]
        }
        _ => Vec::new(),
    };
    args.iter()
        .enumerate()
        .map(|(index, arg)| match argument_types.get(index) {
            Some(ty) => eval_expected_in(solver, model, arg, ty, state, bindings, old_state, ctx),
            None => eval_in(solver, model, arg, state, bindings, old_state, ctx),
        })
        .collect()
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn eval_method<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    receiver: SymbolicValue<S::Term>,
    name: &str,
    args: &[Expr],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let values = eval_method_arguments(
        solver, model, &receiver, name, args, state, bindings, old_state, ctx,
    )?;
    if is_collection(&receiver) {
        let (ty, mut entries) = collection_entries(solver, model, &receiver)?;
        if name == "size" && values.is_empty() {
            let terms = entries
                .iter()
                .map(|(_, present)| solver.ite(present, &solver.int_value(1), &solver.int_value(0)))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(int_value(solver, sum_terms(solver, &terms)?));
        }
        let key = values;
        let grid = matches!(
            receiver,
            SymbolicValue::Set { .. } | SymbolicValue::Relation { .. }
        );
        // A nominal bounded type alone is not proof: a state value or a
        // collection binder can escape it when that bound is unselected.
        let known = literal_key(solver, &key);
        let inside = name == "remove"
            || known.as_ref().is_some_and(|known| {
                entries
                    .iter()
                    .any(|(candidate, _)| literal_key(solver, candidate).as_ref() == Some(known))
            });
        // Known in-carrier adds and grid removals need no new candidate slot.
        // Unknown/outside values still take the complete retention path below.
        if grid && matches!(name, "add" | "remove") && inside {
            return match receiver {
                SymbolicValue::Set { ty, entries: grid } => Ok(SymbolicValue::Set {
                    ty,
                    entries: update_grid(solver, model, &key, grid, &entries, name == "add")?,
                }),
                SymbolicValue::Relation { ty, entries: grid } => Ok(SymbolicValue::Relation {
                    ty,
                    entries: update_grid(solver, model, &key, grid, &entries, name == "add")?,
                }),
                _ => unreachable!(),
            };
        }
        let contains = collection_contains(solver, model, &entries, &key)?;
        return match name {
            "contains" => Ok(bool_value(solver, contains)),
            "add" => {
                if let Some(known) = &known
                    && let Some((_, present)) = entries.iter_mut().find(|(candidate, _)| {
                        literal_key(solver, candidate).as_ref() == Some(known)
                    })
                {
                    *present = solver.or(&[present.clone(), solver.not(&contains)?])?;
                    return Ok(SymbolicValue::Collection { ty, entries });
                }
                entries.push((key, solver.not(&contains)?));
                Ok(SymbolicValue::Collection { ty, entries })
            }
            "remove" => {
                for (candidate, present) in &mut entries {
                    *present = solver.and(&[
                        present.clone(),
                        solver.not(&collection_key_equal(solver, model, candidate, &key)?)?,
                    ])?;
                }
                Ok(SymbolicValue::Collection { ty, entries })
            }
            _ => Err(VerifyError::new(format!(
                "invalid collection method '{name}'"
            ))),
        };
    }
    match receiver {
        SymbolicValue::Seq { ty, slots, len } => {
            let TypeRef::Seq(element_ty, capacity) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("push", [value]) => {
                    let value = coerce(solver, model, value.clone(), element_ty)?;
                    let mut slots: Vec<_> = slots
                        .iter()
                        .enumerate()
                        .map(|(index, old)| {
                            let matches =
                                solver.equal(&len, &solver.int_value(i64_index(index)?))?;
                            ite_value(solver, model, &matches, &value, old)
                        })
                        .collect::<Result<_, VerifyError>>()?;
                    // A temporary may grow beyond the declared state capacity.
                    // When len < the old slot count, this extra slot is not live.
                    slots.push(value);
                    Ok(SymbolicValue::Seq {
                        ty,
                        slots,
                        len: solver.add(&len, &solver.int_value(1))?,
                    })
                }
                ("pop", []) => {
                    let mut shifted = slots.iter().skip(1).cloned().collect::<Vec<_>>();
                    if let Some(last) = slots.last() {
                        shifted.push(last.clone());
                    }
                    Ok(SymbolicValue::Seq {
                        ty,
                        slots: shifted,
                        len: solver.sub(&len, &solver.int_value(1))?,
                    })
                }
                ("head", []) => eval_seq_read(solver, model, &ty, &slots, None),
                ("at", [index]) => eval_seq_read(solver, model, &ty, &slots, Some(index)),
                ("contains", [value]) => {
                    let mut terms = Vec::new();
                    for (index, slot) in slots.iter().enumerate() {
                        let active = solver.lt(&solver.int_value(i64_index(index)?), &len)?;
                        let same = logical_equal(solver, model, slot, value)?;
                        terms.push(solver.and(&[active, same])?);
                    }
                    Ok(bool_value(solver, solver.or(&terms)?))
                }
                ("size", []) => Ok(int_value(solver, len)),
                _ => Err(VerifyError::new(format!(
                    "invalid Seq<{element_ty:?}, {capacity}> method '{name}'"
                ))),
            }
        }
        _ => Err(VerifyError::new(
            "method receiver has no collection methods",
        )),
    }
}

fn update_grid<S: SmtSolver, K>(
    solver: &S,
    model: &KernelModel,
    key: &[SymbolicValue<S::Term>],
    grid: Vec<(K, S::Term)>,
    symbolic: &crate::value::CollectionEntries<S::Term>,
    added: bool,
) -> Result<Vec<(K, S::Term)>, VerifyError> {
    grid.into_iter()
        .zip(symbolic)
        .map(|((key_value, present), (candidate, _))| {
            let same = collection_key_equal(solver, model, candidate, key)?;
            Ok((
                key_value,
                solver.ite(&same, &solver.bool_value(added), &present)?,
            ))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn eval_binary<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    op: &str,
    left: &Expr,
    right: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let (left, right) = if matches!(op, "==" | "!=") {
        eval_equality_operands(solver, model, left, right, state, bindings, old_state, ctx)?
    } else {
        let mut local = bindings.clone();
        let left = eval_with_bindings_in(solver, model, left, state, &mut local, old_state, ctx)?;
        confine_binary_rhs(op, bindings, &mut local);
        let right = eval_with_bindings_in(solver, model, right, state, &mut local, old_state, ctx)?;
        if pattern_binding_flows(op, false) {
            *bindings = local;
        }
        (left, right)
    };
    match op {
        "and" => Ok(bool_value(
            solver,
            solver.and(&[bool_term(&left)?.clone(), bool_term(&right)?.clone()])?,
        )),
        "or" => Ok(bool_value(
            solver,
            solver.or(&[bool_term(&left)?.clone(), bool_term(&right)?.clone()])?,
        )),
        "=>" => Ok(bool_value(
            solver,
            solver.implies(bool_term(&left)?, bool_term(&right)?)?,
        )),
        "==" | "!=" => {
            let equal = logical_equal(solver, model, &left, &right)?;
            Ok(bool_value(
                solver,
                if op == "==" {
                    equal
                } else {
                    solver.not(&equal)?
                },
            ))
        }
        "+" | "-" | "*" | "/" | "%" => {
            let term = match op {
                "+" => solver.add(int_term(&left)?, int_term(&right)?)?,
                "-" => solver.sub(int_term(&left)?, int_term(&right)?)?,
                "*" => solver.mul(int_term(&left)?, int_term(&right)?)?,
                "/" => solver.div(int_term(&left)?, int_term(&right)?)?,
                "%" => solver.modulo(int_term(&left)?, int_term(&right)?)?,
                _ => unreachable!(),
            };
            Ok(int_value(solver, term))
        }
        "<" | "<=" | ">" | ">=" => {
            let term = match op {
                "<" => solver.lt(int_term(&left)?, int_term(&right)?)?,
                "<=" => solver.le(int_term(&left)?, int_term(&right)?)?,
                ">" => solver.gt(int_term(&left)?, int_term(&right)?)?,
                ">=" => solver.ge(int_term(&left)?, int_term(&right)?)?,
                _ => unreachable!(),
            };
            Ok(bool_value(solver, term))
        }
        _ => Err(VerifyError::new(format!("unknown operator '{op}'"))),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_equality_operands<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    left: &Expr,
    right: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicPair<S::Term>, VerifyError> {
    let ty = model.equality_operand_type(left, right, &binding_types(bindings))?;
    Ok((
        eval_expected_in(solver, model, left, &ty, state, bindings, old_state, ctx)?,
        eval_expected_in(solver, model, right, &ty, state, bindings, old_state, ctx)?,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_expected<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    expected: &TypeRef,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    eval_expected_in(
        solver, model, expr, expected, state, bindings, old_state, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_expected_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    expected: &TypeRef,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
        ..
    } = expr
    {
        let condition = eval_in(solver, model, condition, state, bindings, old_state, ctx)?;
        let then_value = eval_expected_in(
            solver, model, then_expr, expected, state, bindings, old_state, ctx,
        )?;
        let else_value = eval_expected_in(
            solver, model, else_expr, expected, state, bindings, old_state, ctx,
        )?;
        return ite_value(
            solver,
            model,
            bool_term(&condition)?,
            &then_value,
            &else_value,
        );
    }
    if let (Expr::Some(inner), TypeRef::Option(inner_ty)) = (expr, expected) {
        return Ok(SymbolicValue::Option {
            ty: expected.clone(),
            present: solver.bool_value(true),
            value: Box::new(eval_expected_in(
                solver, model, inner, inner_ty, state, bindings, old_state, ctx,
            )?),
        });
    }
    let value = match (expr, expected) {
        (Expr::Set(items), TypeRef::Set(element))
        | (Expr::Seq(items), TypeRef::Seq(element, _)) => {
            let values = items
                .iter()
                .map(|item| {
                    eval_expected_in(
                        solver, model, item, element, state, bindings, old_state, ctx,
                    )
                })
                .collect::<Result<_, _>>()?;
            if matches!(expr, Expr::Set(_)) {
                SymbolicValue::SetLiteral(values)
            } else {
                SymbolicValue::SeqLiteral(values)
            }
        }
        // The empty relation initializer has no element expressions.
        (Expr::Set(items), TypeRef::Relation(_, _)) if items.is_empty() => {
            SymbolicValue::SetLiteral(Vec::new())
        }
        _ => eval_raw_in(solver, model, expr, state, bindings, old_state, ctx)?,
    };
    coerce(solver, model, value, expected)
}

fn eval_pattern<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: SymbolicValue<S::Term>,
    pattern: &Pattern,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match (value, pattern) {
        (SymbolicValue::None, Pattern::None) => Ok(bool_value(solver, solver.bool_value(true))),
        (SymbolicValue::None, Pattern::Some(_)) => Ok(bool_value(solver, solver.bool_value(false))),
        (SymbolicValue::Option { present, .. }, Pattern::None) => {
            Ok(bool_value(solver, solver.not(&present)?))
        }
        (SymbolicValue::Option { present, value, .. }, Pattern::Some(name)) => {
            // Rewritten matches shadow locally; closed evaluation restores the
            // complete outer typed binding. Absence retains its outer meaning.
            let bound = match lookup(solver, model, name, state, bindings) {
                Ok(outer) if outer.ty() == value.ty() => {
                    ite_value(solver, model, &present, &value, &outer)?
                }
                _ => *value,
            };
            bindings.insert(name.clone(), bound);
            Ok(bool_value(solver, present))
        }
        _ => Err(VerifyError::new("is pattern requires an Option value")),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_quantified<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    binder: &Binder,
    body: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    prepared: Option<&DischargedIntegerRange<'_, S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let candidates = if prepared.is_none() {
        binder_values_in(solver, model, binder, state, bindings, old_state, ctx)?
    } else {
        Vec::new()
    };
    let mut terms = Vec::new();
    for (name, value, membership) in
        candidates
            .into_iter()
            .chain(prepared.into_iter().flat_map(|p| {
                p.candidates()
                    .iter()
                    .map(|(n, v, m)| (n.clone(), v.clone(), Some(m.clone())))
            }))
    {
        let mut local = bindings.clone();
        let record = binder_candidate_binding(
            solver, model, binder, value, state, old_state, bindings, ctx,
        );
        local.insert(name, record);
        let body = eval_in(solver, model, body, state, &mut local, old_state, ctx)?;
        let body = bool_term(&body)?.clone();
        let where_term = binder_where_in(solver, model, binder, state, &mut local, old_state, ctx)?;
        let condition = binder_condition(solver, membership, where_term)?;
        terms.push(if quantifier == "forall" {
            solver.implies(&condition, &body)?
        } else {
            solver.and(&[condition, body])?
        });
    }
    let term = if quantifier == "forall" {
        solver.and(&terms)?
    } else {
        solver.or(&terms)?
    };
    Ok(bool_value(solver, term))
}

fn binder_conditions<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<Vec<S::Term>, VerifyError> {
    let candidates = binder_values_in(solver, model, binder, state, bindings, old_state, ctx)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        let record = binder_candidate_binding(
            solver, model, binder, value, state, old_state, bindings, ctx,
        );
        local.insert(name, record);
        let where_term = binder_where_in(solver, model, binder, state, &mut local, old_state, ctx)?;
        terms.push(binder_condition(solver, membership, where_term)?);
    }
    Ok(terms)
}

#[allow(clippy::too_many_arguments)]
fn eval_aggregate<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    kind: AggregateKind,
    binder: &Binder,
    value: Option<&Expr>,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if kind != AggregateKind::Sum {
        let conditions = binder_conditions(solver, model, binder, state, bindings, old_state, ctx)?;
        let counts = conditions
            .iter()
            .map(|condition| {
                solver
                    .ite(condition, &solver.int_value(1), &solver.int_value(0))
                    .map_err(VerifyError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let count = sum_terms(solver, &counts)?;
        return Ok(match kind {
            AggregateKind::Count => int_value(solver, count),
            AggregateKind::Unique => bool_value(solver, solver.le(&count, &solver.int_value(1))?),
            AggregateKind::ExactlyOne => {
                bool_value(solver, solver.equal(&count, &solver.int_value(1))?)
            }
            AggregateKind::Sum => unreachable!(),
        });
    }

    let value = value.ok_or_else(|| VerifyError::new("sum aggregate requires a value"))?;
    let mut terms = Vec::new();
    for (name, candidate, membership) in
        binder_values_in(solver, model, binder, state, bindings, old_state, ctx)?
    {
        let mut local = bindings.clone();
        let record = binder_candidate_binding(
            solver, model, binder, candidate, state, old_state, bindings, ctx,
        );
        local.insert(name, record);
        let value = eval_in(solver, model, value, state, &mut local, old_state, ctx)?;
        let where_term = binder_where_in(solver, model, binder, state, &mut local, old_state, ctx)?;
        let condition = binder_condition(solver, membership, where_term)?;
        terms.push(solver.ite(&condition, int_term(&value)?, &solver.int_value(0))?);
    }
    Ok(int_value(solver, sum_terms(solver, &terms)?))
}

type ConditionalBinderCandidates<T> = Vec<(String, SymbolicValue<T>, Option<T>)>;
type TypedBinderCandidates<T> = Vec<(String, TypedBinding<T>, Option<T>)>;
type IntegerRangeCandidates<T> = Vec<(String, TypedBinding<T>, T)>;

pub(crate) struct IntegerRangeDraft<'f, T> {
    pub(crate) binder: &'f Binder,
    pub(crate) read_state: &'f SymbolicState<T>,
    pub(crate) enclosing: &'f Bindings<T>,
    pub(crate) actual: (T, T),
    pub(crate) source_status: EvaluationStatus<T>,
    pub(crate) plan: RangePlan,
    pub(crate) captured_terms: Vec<(usize, &'f RangeFact<T>)>,
}

pub(crate) fn integer_range_plan<T>(
    model: &KernelModel,
    binder: &Binder,
    enclosing: &Bindings<T>,
) -> RangePlan {
    let Binder::Range { lo, hi, .. } = binder else {
        return RangePlan::Unknown(RangeUnknown::UnsupportedExpression);
    };
    let lexical: Option<Vec<_>> = enclosing
        .records()
        .map(|(name, record)| {
            Some(RangeBinding {
                name,
                semantic_ty: record.binding.1.as_ref()?,
                fact: record.range_fact.as_ref().map(RangeFact::envelope),
            })
        })
        .collect();
    lexical.map_or(
        RangePlan::Unknown(RangeUnknown::MissingFiniteFact),
        |lexical| model.plan_integer_range(lo, hi, &lexical),
    )
}

pub(crate) fn integer_range_error(plan: &RangePlan) -> VerifyError {
    let RangePlan::Unknown(reason) = plan else {
        return VerifyError::new(
            "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope",
        );
    };
    let reason = match reason {
        RangeUnknown::MissingFiniteFact => "missing-finite-fact",
        RangeUnknown::UnsupportedExpression => "unsupported-expression",
        RangeUnknown::EnvelopeUnrepresentable => "envelope-unrepresentable",
        RangeUnknown::ConstantFoldFailed => "constant-fold-failed",
    };
    VerifyError::new(format!(
        "unsupported integer range execution [planner]: {reason}"
    ))
}

pub(crate) fn property_range_source_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    property_range_source_status_in(solver, model, binder, state, bindings, None)
}

pub(crate) fn property_range_source_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    binder_source_evaluation_status(
        solver,
        model,
        binder,
        state,
        bindings,
        None,
        PROPERTY_EVALUATION,
        ctx,
    )
}

pub(crate) fn integer_range_draft<'f, S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &'f Binder,
    read_state: &'f SymbolicState<S::Term>,
    enclosing: &'f Bindings<S::Term>,
    planned_source: (RangePlan, EvaluationStatus<S::Term>),
) -> Result<IntegerRangeDraft<'f, S::Term>, VerifyError> {
    integer_range_draft_in(
        solver,
        model,
        binder,
        read_state,
        enclosing,
        planned_source,
        None,
    )
}

pub(crate) fn integer_range_draft_in<'f, S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &'f Binder,
    read_state: &'f SymbolicState<S::Term>,
    enclosing: &'f Bindings<S::Term>,
    planned_source: (RangePlan, EvaluationStatus<S::Term>),
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<IntegerRangeDraft<'f, S::Term>, VerifyError> {
    let Binder::Range { lo, hi, .. } = binder else {
        return Err(integer_range_error(&planned_source.0));
    };
    let mut local = enclosing.clone();
    let lo = eval_in(solver, model, lo, read_state, &mut local, None, ctx)?;
    let hi = eval_in(solver, model, hi, read_state, &mut local, None, ctx)?;
    let captured_terms = enclosing
        .records()
        .enumerate()
        .filter_map(|(i, (_, r))| r.range_fact.as_ref().map(|fact| (i, fact)))
        .collect();
    Ok(IntegerRangeDraft {
        binder,
        read_state,
        enclosing,
        actual: (int_term(&lo)?.clone(), int_term(&hi)?.clone()),
        plan: planned_source.0,
        source_status: planned_source.1,
        captured_terms,
    })
}

pub(crate) fn materialize_integer_range<S: SmtSolver>(
    solver: &S,
    draft: &IntegerRangeDraft<'_, S::Term>,
) -> Result<IntegerRangeCandidates<S::Term>, VerifyError> {
    let envelope = match &draft.plan {
        RangePlan::Empty { .. } => return Ok(Vec::new()),
        RangePlan::Finite { candidates, .. } => candidates,
        _ => return Err(integer_range_error(&draft.plan)),
    };
    let Binder::Range { name, .. } = draft.binder else {
        return Err(integer_range_error(&draft.plan));
    };
    // There is no size cap, but a list this long cannot be allocated: refuse rather than
    // abort. Running out while allocating solver terms is the native CLI's refusal.
    let width = usize::try_from(i128::from(envelope.hi()) - i128::from(envelope.lo()) + 1);
    let mut candidates = Vec::new();
    width
        .ok()
        .and_then(|width| candidates.try_reserve_exact(width).ok())
        .ok_or_else(|| {
            VerifyError::new(
                "unsupported integer range execution [resource]: not enough memory to \
                 enumerate this range's candidates",
            )
        })?;
    for z in envelope.lo()..=envelope.hi() {
        let term = solver.int_value(z);
        let membership = solver.and(&[
            solver.le(&draft.actual.0, &term)?,
            solver.le(&term, &draft.actual.1)?,
        ])?;
        candidates.push((
            name.clone(),
            TypedBinding(int_value(solver, term), Some(TypeRef::Int)),
            membership,
        ));
    }
    Ok(candidates)
}

pub(crate) fn prepared_quantified_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    body: &Expr,
    prepared: &DischargedIntegerRange<'_, S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    prepared_quantified_status_in(solver, model, quantifier, body, prepared, None)
}

pub(crate) fn prepared_quantified_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    body: &Expr,
    prepared: &DischargedIntegerRange<'_, S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let d = prepared.draft();
    quantified_evaluation_status(
        solver,
        model,
        quantifier,
        d.binder,
        body,
        d.read_state,
        d.enclosing,
        None,
        PROPERTY_EVALUATION,
        Some(prepared),
        ctx,
    )
}

pub(crate) fn prepared_quantified_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    body: &Expr,
    prepared: &DischargedIntegerRange<'_, S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    prepared_quantified_value_in(solver, model, quantifier, body, prepared, None)
}

pub(crate) fn prepared_quantified_value_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    body: &Expr,
    prepared: &DischargedIntegerRange<'_, S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let d = prepared.draft();
    eval_quantified(
        solver,
        model,
        quantifier,
        d.binder,
        body,
        d.read_state,
        d.enclosing,
        None,
        Some(prepared),
        ctx,
    )
}

/// An unfiltered binder has condition true, the identity for conjunction.
pub(crate) fn binder_condition<S: SmtSolver>(
    solver: &S,
    membership: Option<S::Term>,
    where_term: Option<S::Term>,
) -> Result<S::Term, VerifyError> {
    Ok(match (membership, where_term) {
        (Some(membership), Some(where_term)) => solver.and(&[membership, where_term])?,
        (Some(condition), None) | (None, Some(condition)) => condition,
        (None, None) => solver.bool_value(true),
    })
}

fn parameter_range_values<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    plan: RangePlan,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<TypedBinderCandidates<S::Term>, VerifyError> {
    let (RangePlan::Finite { prerequisites, .. } | RangePlan::Empty { prerequisites }) = &plan
    else {
        return Err(integer_range_error(&plan));
    };
    let eligible = plan.is_executable(|p| match p {
        RangePrerequisite::LexicalFact { input_index } => bindings
            .records()
            .nth(*input_index)
            .and_then(|(_, r)| r.range_fact.as_ref())
            .is_some_and(|f| f.valid_when().is_none()),
        RangePrerequisite::StateCarrier { name, .. } => {
            ctx.is_some_and(|checked| checked.contains(state, name))
        }
    });
    if !eligible {
        // Captured lexical values remain unsupported without their own discharge.
        // Preserve their legacy bound diagnostic without approving any candidates
        // or replacing a state-carrier eligibility refusal.
        if prerequisites
            .iter()
            .all(|p| matches!(p, RangePrerequisite::LexicalFact { .. }))
            && let Binder::Range { lo, hi, .. } = binder
        {
            static_int(lo, model)?;
            static_int(hi, model)?;
        }
        return Err(integer_range_error(&plan));
    }
    let status = property_range_source_status_in(solver, model, binder, state, bindings, ctx)?;
    let draft =
        integer_range_draft_in(solver, model, binder, state, bindings, (plan, status), ctx)?;
    Ok(materialize_integer_range(solver, &draft)?
        .into_iter()
        .map(|(n, v, m)| (n, v, Some(m)))
        .collect())
}

pub(crate) fn binder_values<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<TypedBinderCandidates<S::Term>, VerifyError> {
    binder_values_in(solver, model, binder, state, bindings, old_state, None)
}

pub(crate) fn binder_values_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<TypedBinderCandidates<S::Term>, VerifyError> {
    if let Binder::Range { lo, hi, .. } = binder {
        let plan = integer_range_plan(model, binder, bindings);
        match &plan {
            RangePlan::Static { .. } => {}
            RangePlan::Unknown(RangeUnknown::ConstantFoldFailed) => {
                static_int(lo, model)?;
                static_int(hi, model)?;
                return Err(integer_range_error(&plan));
            }
            RangePlan::Unknown(_) => return Err(integer_range_error(&plan)),
            RangePlan::Finite { .. } | RangePlan::Empty { .. } => {
                return parameter_range_values(solver, model, binder, state, bindings, plan, ctx);
            }
        }
    }
    let Binder::Collection {
        name, collection, ..
    } = binder
    else {
        return static_binder_values(solver, model, binder).map(|values| {
            values
                .into_iter()
                .map(|(name, value)| (name, value.into(), None))
                .collect()
        });
    };
    let ty = model
        .expression_type(collection, &binding_types(bindings), None)?
        .ok_or_else(|| VerifyError::new("collection binder requires an inferred type"))?;
    let (TypeRef::Set(element) | TypeRef::Seq(element, _)) = ty else {
        return Err(VerifyError::new("collection binder requires a Set or Seq"));
    };
    let mut local = bindings.clone();
    let collection = eval_in(solver, model, collection, state, &mut local, old_state, ctx)?;
    let candidates: ConditionalBinderCandidates<S::Term> = match collection {
        SymbolicValue::Collection { ty, entries } => {
            let TypeRef::Set(_) = ty else {
                return Err(VerifyError::new("collection binder requires a Set"));
            };
            ordered_set_candidates(
                solver,
                model,
                name,
                entries
                    .into_iter()
                    .map(|(key, present)| {
                        Ok((coerce(solver, model, key[0].clone(), &element)?, present))
                    })
                    .collect::<Result<_, VerifyError>>()?,
            )
        }
        SymbolicValue::Set { ty, mut entries } => {
            let TypeRef::Set(element_ty) = ty else {
                unreachable!();
            };
            // Concrete Sets use BTreeSet<Value>, not enum declaration order.
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            entries
                .into_iter()
                .map(|(element, present)| {
                    Ok((
                        name.clone(),
                        concrete_value(solver, model, &element_ty, &element)?,
                        Some(present),
                    ))
                })
                .collect()
        }
        SymbolicValue::Seq { slots, len, .. } => slots
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Ok((
                    name.clone(),
                    value,
                    Some(solver.lt(&solver.int_value(i64_index(index)?), &len)?),
                ))
            })
            .collect(),
        SymbolicValue::SeqLiteral(items) => Ok(items
            .into_iter()
            .map(|value| (name.clone(), value, None))
            .collect()),
        SymbolicValue::SetLiteral(items) => set_literal_candidates(solver, model, name, items),
        _ => Err(VerifyError::new(
            "collection binder expects a Set or Seq expression",
        )),
    }?;
    Ok(candidates
        .into_iter()
        .map(|(name, value, membership)| {
            let binding = TypedBinding(value, Some(element.as_ref().clone()));
            (name, binding, membership)
        })
        .collect())
}

fn set_literal_candidates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    items: Vec<SymbolicValue<S::Term>>,
) -> Result<ConditionalBinderCandidates<S::Term>, VerifyError> {
    ordered_set_candidates(
        solver,
        model,
        name,
        items
            .into_iter()
            .map(|value| (value, solver.bool_value(true)))
            .collect(),
    )
}

fn ordered_set_candidates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    items: Vec<(SymbolicValue<S::Term>, S::Term)>,
) -> Result<ConditionalBinderCandidates<S::Term>, VerifyError> {
    let entries: Vec<_> = items
        .into_iter()
        .map(|(value, present)| (vec![value], present))
        .collect();
    // Known integers need no solver terms to order, so a candidate stays the literal it is.
    // Membership gates every candidate, so absent keys need not sort behind present ones.
    let known: Option<Vec<i64>> = entries
        .iter()
        .map(
            |(key, _)| match (key.as_slice(), literal_key(solver, key)?.as_slice()) {
                ([SymbolicValue::Scalar { ty, .. }], [ModelValue::Int(value)])
                    if !matches!(ty, TypeRef::Named(name)
                    if matches!(model.types.get(name), Some(TypeDef::Enum { .. }))) =>
                {
                    Some(*value)
                }
                _ => None,
            },
        )
        .collect();
    let ordered = if let Some(known) = known {
        let mut keyed: Vec<_> = known
            .into_iter()
            .zip(distinct_entries(solver, model, entries)?)
            .collect();
        keyed.sort_by_key(|(value, _)| *value);
        keyed.into_iter().map(|(_, entry)| entry).collect()
    } else {
        ordered_collection_entries(solver, model, entries)?
    };
    Ok(ordered
        .into_iter()
        .map(|(key, present)| (name.to_owned(), key[0].clone(), Some(present)))
        .collect())
}

pub(crate) fn static_binder_values<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
) -> Result<BinderCandidates<S::Term>, VerifyError> {
    match binder {
        Binder::Typed {
            name, type_name, ..
        } => {
            let type_name = qualified_type(type_name.namespace.as_deref(), &type_name.name)?;
            let ty = model.typed_binder_type(&type_name);
            model
                .domain_values(&ty)?
                .into_iter()
                .map(|value| Ok((name.clone(), concrete_value(solver, model, &ty, &value)?)))
                .collect()
        }
        Binder::Range { name, lo, hi, .. } => {
            let plan = integer_range_plan(model, binder, &Bindings::<S::Term>::new());
            if !matches!(
                plan,
                RangePlan::Static { .. } | RangePlan::Unknown(RangeUnknown::ConstantFoldFailed)
            ) {
                return Err(integer_range_error(&plan));
            }
            let lo = static_int(lo, model)?;
            let hi = static_int(hi, model)?;
            if !matches!(plan, RangePlan::Static { .. }) {
                return Err(integer_range_error(&plan));
            }
            (lo..=hi)
                .map(|value| {
                    Ok((
                        name.clone(),
                        SymbolicValue::Scalar {
                            ty: TypeRef::Int,
                            term: solver.int_value(value),
                        },
                    ))
                })
                .collect()
        }
        Binder::Collection { .. } => Err(VerifyError::new(
            "collection binders are not implemented in the current verifier slice",
        )),
    }
}

pub(crate) fn binder_where<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<Option<S::Term>, VerifyError> {
    binder_where_in(solver, model, binder, state, bindings, old_state, None)
}

pub(crate) fn binder_where_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<Option<S::Term>, VerifyError> {
    binder_where_expression(binder)
        .map(|expr| {
            let value = eval_in(solver, model, expr, state, bindings, old_state, ctx)?;
            Ok(bool_term(&value)?.clone())
        })
        .transpose()
}

pub(crate) fn static_int(expr: &Expr, model: &KernelModel) -> Result<i64, VerifyError> {
    model
        .static_binder_bound(expr, &mut str::to_owned)
        .map_err(VerifyError::new)
}

fn qualified_type(namespace: Option<&str>, name: &str) -> Result<String, VerifyError> {
    if namespace.is_some() {
        Err(VerifyError::new(
            "qualified type remained after kernel lowering",
        ))
    } else {
        Ok(name.to_owned())
    }
}

fn sum_terms<S: SmtSolver>(solver: &S, terms: &[S::Term]) -> Result<S::Term, VerifyError> {
    let mut sum = solver.int_value(0);
    for term in terms {
        sum = solver.add(&sum, term)?;
    }
    Ok(sum)
}

fn bool_value<S: SmtSolver>(solver: &S, term: S::Term) -> SymbolicValue<S::Term> {
    let _ = solver;
    SymbolicValue::Scalar {
        ty: TypeRef::Bool,
        term,
    }
}

fn int_value<S: SmtSolver>(solver: &S, term: S::Term) -> SymbolicValue<S::Term> {
    let _ = solver;
    SymbolicValue::Scalar {
        ty: TypeRef::Int,
        term,
    }
}

#[cfg(test)]
mod method_context_tests {
    use super::*;
    use fsl_core::{TraceAction, TraceChange, TraceStep};
    use fsl_solver_z3::Z3Solver;

    const SOURCE: &str = "spec MethodContext {\n  type K = 0..1\n  state { c: Bool, oo: Option<Option<K>> }\n  init { c = true oo = some(some(0)) }\n  action go() { c = false oo = some(none) }\n  invariant Capture { c or $P }\n}\n";

    fn ready<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        match std::pin::pin!(future)
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("native solver yielded"),
        }
    }

    fn exercise(predicate: &str, negative: bool, partial: bool) {
        let predicate = if negative {
            format!("not ({predicate})")
        } else {
            predicate.to_owned()
        };
        let source = SOURCE.replace("$P", &predicate);
        let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
            .expect("reviewed contextual method source parses");
        let model = fsl_core::build_model(kernel).expect("contextual method source is admitted");
        let before = BTreeMap::from([
            ("c".into(), FslValue::Bool(true)),
            (
                "oo".into(),
                FslValue::Some(Box::new(FslValue::Some(Box::new(FslValue::Int(0))))),
            ),
        ]);
        let after = BTreeMap::from([
            ("c".into(), FslValue::Bool(false)),
            ("oo".into(), FslValue::Some(Box::new(FslValue::None))),
        ]);
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).unwrap();
        assert_eq!(monitor.state, before);
        let action = monitor
            .enabled()
            .unwrap()
            .into_iter()
            .find(|a| a.action == "go")
            .unwrap();
        let concrete = monitor.step(&action).unwrap();
        let mut solver = Z3Solver::new().unwrap();
        let result = ready(crate::verify_bounded(&model, &mut solver, 2))
            .expect("value and definedness must share checked argument contexts");
        if !negative && !partial {
            assert!(concrete.violation.is_none());
            assert_eq!(concrete.state, after);
            assert!(result.violation.is_none(), "{source}: {result:?}");
            return;
        }
        let kind = if partial { "partial_op" } else { "invariant" };
        let name = if partial {
            "_partial_property_Capture"
        } else {
            "Capture"
        };
        let concrete_violation = concrete.violation.unwrap();
        assert_eq!(concrete_violation.kind, kind);
        assert_eq!(concrete_violation.name, name);
        assert_eq!(concrete_violation.step, 1);
        assert_eq!(concrete.attempted_state, Some(after.clone()));
        let expected_trace = vec![
            TraceStep {
                step: 0,
                state: before.clone(),
                action: None,
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 1,
                state: after.clone(),
                action: Some(TraceAction {
                    name: "go".into(),
                    params: BTreeMap::new(),
                }),
                changes: before
                    .into_iter()
                    .map(|(name, from)| {
                        let to = after[&name].clone();
                        (name, TraceChange { from, to })
                    })
                    .collect(),
            },
        ];
        let observed = result
            .violation
            .expect("literal step-one negative must reject");
        assert_eq!(
            (
                observed.kind.as_str(),
                observed.name.as_str(),
                observed.step
            ),
            (kind, name, 1)
        );
        assert_eq!(observed.last_action.as_deref(), Some("go"));
        assert_eq!(observed.trace, expected_trace);
        assert!(observed.leads_to.is_none());
    }

    #[test]
    fn contextual_method_values_and_status_share_literal_reachable_truth() {
        for (predicate, partial) in [
            ("Seq {oo}.contains(some(none))", false),
            ("Set {oo}.contains(some(none))", false),
            ("Set {oo}.add(some(none)).size() == 1", false),
            ("Set {oo}.remove(some(none)).size() == 0", false),
            ("Seq {oo}.push(some(none)).at(1) == oo", false),
            ("Seq {Seq {oo}.pop()}.contains(Seq {})", false),
            ("Seq {oo}.contains(if c then oo else some(none))", false),
            (
                "Seq {oo}.contains(if c then some(some(Seq {1}.pop().head())) else some(none))",
                false,
            ),
            (
                "Seq {oo}.contains(if c then some(none) else some(some(Seq {1}.pop().head())))",
                true,
            ),
        ] {
            for negative in [false, true] {
                exercise(predicate, negative, partial);
            }
        }
    }
}
