// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use fsl_core::recursion;
use fsl_core::{
    FslValue, KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr,
    KernelModel, Pattern, TypeDef, TypeRef,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::value::TypedBindings as Bindings;
use crate::value::{
    SymbolicState, SymbolicValue, TypedBinding, bool_term, coerce, collection_contains,
    collection_entries, collection_key_equal, concrete_value, distinct_entries, i64_index,
    int_term, is_collection, ite_value, literal_key, logical_equal, select_finite, solver_sort,
};

type BinderCandidates<T> = Vec<(String, SymbolicValue<T>)>;
type SymbolicPair<T> = (SymbolicValue<T>, SymbolicValue<T>);

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
    let value = eval_raw(solver, model, expr, state, bindings, old_state)?;
    if scalar_literal(&value) {
        let ty = model
            .expression_type(expr, &binding_types(bindings), None)?
            .ok_or_else(|| VerifyError::new("collection literal requires an expected type"))?;
        materialize(solver, model, value, &ty, false)
    } else {
        Ok(value)
    }
}

fn binding_types<T>(bindings: &Bindings<T>) -> Vec<(String, TypeRef)> {
    bindings
        .types()
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect()
}

pub(crate) fn eval_binding<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<TypedBinding<S::Term>, VerifyError> {
    let ty = model
        .expression_type(expr, &binding_types(bindings), None)?
        .ok_or_else(|| VerifyError::new("binding requires an inferred type"))?;
    let value = eval(solver, model, expr, state, bindings, None)?;
    Ok(TypedBinding(value, Some(ty)))
}

fn scalar_literal<T>(value: &SymbolicValue<T>) -> bool {
    matches!(value, SymbolicValue::SetLiteral(items) | SymbolicValue::SeqLiteral(items)
        if items.iter().all(|item| matches!(item, SymbolicValue::Scalar { .. })))
}

/// The only literal materialization boundary, for inferred and explicit contexts.
fn materialize<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: SymbolicValue<S::Term>,
    ty: &TypeRef,
    contextual: bool,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if contextual || scalar_literal(&value) || value.ty().is_some() {
        coerce(solver, model, value, ty)
    } else {
        Ok(value)
    }
}

fn eval_raw<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    recursion::guard(|| eval_inner(solver, model, expr, state, bindings, old_state))
}

#[allow(clippy::too_many_lines)]
fn eval_inner<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
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
        Expr::Some(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            let inner_ty = value
                .ty()
                .cloned()
                .ok_or_else(|| VerifyError::new("some() requires a typed value"))?;
            Ok(SymbolicValue::Option {
                ty: TypeRef::Option(Box::new(inner_ty)),
                present: solver.bool_value(true),
                value: Box::new(value),
            })
        }
        Expr::Set(items) => Ok(SymbolicValue::SetLiteral(
            items
                .iter()
                .map(|item| eval(solver, model, item, state, bindings, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Seq(items) => Ok(SymbolicValue::SeqLiteral(
            items
                .iter()
                .map(|item| eval(solver, model, item, state, bindings, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Struct { name, fields } => {
            eval_struct_literal(solver, model, name, fields, state, bindings, old_state)
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
            let base = eval(solver, model, base, state, bindings, old_state)?;
            let index = eval(solver, model, index, state, bindings, old_state)?;
            eval_index(solver, model, &base, &index)
        }
        Expr::Field(base, field) => {
            let base = eval(solver, model, base, state, bindings, old_state)?;
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
            let receiver = eval(solver, model, receiver, state, bindings, old_state)?;
            eval_method(
                solver, model, receiver, name, args, state, bindings, old_state,
            )
        }
        Expr::Binary { op, left, right } => {
            eval_binary(solver, model, op, left, right, state, bindings, old_state)
        }
        Expr::Neg(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            Ok(SymbolicValue::Scalar {
                ty: TypeRef::Int,
                term: solver.neg(int_term(&value)?)?,
            })
        }
        Expr::Not(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            Ok(bool_value(solver, solver.not(bool_term(&value)?)?))
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if let Some(ty) = model.expression_type(expr, &binding_types(bindings), None)?
                && matches!(&ty, TypeRef::Set(item) | TypeRef::Seq(item, _)
                    if solver_sort(model, item).is_ok())
            {
                return eval_in_context(
                    solver, model, expr, &ty, state, bindings, old_state, false,
                );
            }
            let condition = eval(solver, model, condition, state, bindings, old_state)?;
            let then_value = eval_raw(solver, model, then_expr, state, bindings, old_state)?;
            let else_value = eval_raw(solver, model, else_expr, state, bindings, old_state)?;
            ite_value(
                solver,
                model,
                bool_term(&condition)?,
                &then_value,
                &else_value,
            )
        }
        Expr::Is { expr, pattern } => {
            let value = eval(solver, model, expr, state, bindings, old_state)?;
            eval_pattern(solver, model, value, pattern, state, bindings)
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => eval_quantified(
            solver, model, quantifier, binder, body, state, bindings, old_state,
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
        ),
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => eval(
                solver,
                model,
                expr,
                old_state.ok_or_else(|| VerifyError::new("old() used without old state"))?,
                bindings,
                None,
            ),
            "abs" => {
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                let term = int_term(&value)?;
                let nonnegative = solver.ge(term, &solver.int_value(0))?;
                Ok(int_value(
                    solver,
                    solver.ite(&nonnegative, term, &solver.neg(term)?)?,
                ))
            }
            "rel_functional" | "rel_injective" => {
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                let (_, entries) = collection_entries(solver, model, &value)?;
                let (shared, distinct) = if name == "rel_functional" {
                    (0, 1)
                } else {
                    (1, 0)
                };
                let mut clauses = Vec::new();
                for (index, (key, present)) in entries.iter().enumerate() {
                    for (other_key, other_present) in entries.iter().skip(index + 1) {
                        let same = solver.equal(&key[shared], &other_key[shared])?;
                        let different =
                            solver.not(&solver.equal(&key[distinct], &other_key[distinct])?)?;
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
                let value = eval(solver, model, expr, state, bindings, old_state)?;
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
                let value = eval(solver, model, expr, state, bindings, old_state)?;
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
            let left = eval(solver, model, left, state, bindings, old_state)?;
            let right = eval(solver, model, right, state, bindings, old_state)?;
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
            let relation = eval(solver, model, first, state, bindings, old_state)?;
            let (ty, entries) = collection_entries(solver, model, &relation)?;
            let TypeRef::Relation(source_ty, target_ty) = &ty else {
                unreachable!();
            };
            if source_ty != target_ty {
                return Err(VerifyError::new(
                    "reachable() requires a self-relation (relation T -> T)",
                ));
            }
            let source = eval(solver, model, second, state, bindings, old_state)?;
            let target = eval(solver, model, third, state, bindings, old_state)?;
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
    entries: &[(Vec<S::Term>, S::Term)],
) -> Result<RelationReachability<S::Term>, VerifyError> {
    let values = model.domain_values(domain_ty)?;
    let mut vertices: Vec<_> = values
        .iter()
        .map(|value| {
            let SymbolicValue::Scalar { term, .. } =
                concrete_value(solver, model, domain_ty, value)?
            else {
                return Err(VerifyError::new("relation endpoints must be scalar"));
            };
            Ok(term)
        })
        .collect::<Result<_, VerifyError>>()?;
    // Every relation keeps the finite pair grid prefix. Only appended slots
    // introduce candidate vertices; duplicating all grid endpoints is quadratic.
    vertices.extend(
        entries
            .iter()
            .skip(values.len() * values.len())
            .flat_map(|(key, _)| key.iter().cloned()),
    );
    let mut reach = vertices
        .iter()
        .map(|source| {
            vertices
                .iter()
                .map(|target| {
                    collection_contains(solver, entries, &[source.clone(), target.clone()])
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

#[derive(Clone)]
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
    )
}

/// [`property_evaluation_status`] (property context: `/` and `%` are total)
/// with `first_partial` classifying `class`.
pub(crate) fn classified_property_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    class: FailureClass,
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
    evaluation_status_with_policy(
        solver,
        model,
        expr,
        state,
        bindings,
        old_state,
        PROPERTY_EVALUATION,
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    recursion::guard(|| {
        evaluation_status_with_policy_inner(solver, model, expr, state, bindings, old_state, policy)
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
        ),
        Expr::Neg(inner) => {
            let mut local = bindings.clone();
            let inner_status = evaluation_status_with_policy(
                solver, model, inner, state, &local, old_state, policy,
            )?;
            let inner = eval(solver, model, inner, state, &mut local, old_state)?;
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
                solver, model, inner, state, &local, old_state, policy,
            )?;
            let inner = eval(solver, model, inner, state, &mut local, old_state)?;
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
        | Expr::UnaryNamed { expr: inner, .. } => {
            evaluation_status_with_policy(solver, model, inner, state, bindings, old_state, policy)
        }
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            ordered_evaluation_status(solver, model, items, state, bindings, old_state, policy)
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
                solver, model, &items, state, bindings, old_state, policy,
            )
        }
        Expr::Index(base, index) => {
            let mut local = bindings.clone();
            let base_status = evaluation_status_with_policy(
                solver, model, base, state, &local, old_state, policy,
            )?;
            let base_value = eval(solver, model, base, state, &mut local, old_state)?;
            let index_status = evaluation_status_with_policy(
                solver, model, index, state, &local, old_state, policy,
            )?;
            let index_value = eval(solver, model, index, state, &mut local, old_state)?;
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
                solver, model, receiver, state, &local, old_state, policy,
            )?;
            let receiver_value = eval(solver, model, receiver, state, &mut local, old_state)?;
            let arguments_status =
                ordered_evaluation_status(solver, model, args, state, &local, old_state, policy)?;
            let argument_values = args
                .iter()
                .map(|argument| eval(solver, model, argument, state, &mut local, old_state))
                .collect::<Result<Vec<_>, _>>()?;
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
                solver, model, left, state, &local, old_state, policy,
            )?;
            let reached_right = match op.as_str() {
                "and" | "=>" => {
                    let left_value = eval(solver, model, left, state, &mut local, old_state)?;
                    bool_term(&left_value)?.clone()
                }
                "or" => {
                    let left_value = eval(solver, model, left, state, &mut local, old_state)?;
                    solver.not(bool_term(&left_value)?)?
                }
                _ => solver.bool_value(true),
            };
            let right_status = evaluation_status_with_policy(
                solver, model, right, state, &local, old_state, policy,
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
                let left_value = eval(solver, model, left, state, &mut local, old_state)?;
                let right_value = eval(solver, model, right, state, &mut local, old_state)?;
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
                let left_value = eval(solver, model, left, state, &mut local, old_state)?;
                let right_value = eval(solver, model, right, state, &mut local, old_state)?;
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
                solver, model, condition, state, &local, old_state, policy,
            )?;
            let condition_value = eval(solver, model, condition, state, &mut local, old_state)?;
            let then_status = evaluation_status_with_policy(
                solver, model, then_expr, state, &local, old_state, policy,
            )?;
            let else_status = evaluation_status_with_policy(
                solver, model, else_expr, state, &local, old_state, policy,
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
            solver, model, quantifier, binder, body, state, bindings, old_state, policy,
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
        ),
        Expr::BinaryNamed { left, right, .. } => ordered_evaluation_status_refs(
            solver,
            model,
            &[left.as_ref(), right.as_ref()],
            state,
            bindings,
            old_state,
            policy,
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
        ),
    }
}

fn ordered_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[Expr],
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
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
    )
}

fn ordered_evaluation_status_refs<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[&Expr],
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    policy: EvaluationPolicy,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let local = bindings.clone();
    let mut statuses = Vec::new();
    for expression in expressions {
        statuses.push(evaluation_status_with_policy(
            solver, model, expression, state, &local, old_state, policy,
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status =
        binder_source_evaluation_status(solver, model, binder, state, bindings, old_state, policy)?;
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    for (name, value, membership) in
        binder_values(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, value);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| {
                evaluation_status_with_policy(
                    solver, model, where_expr, state, &local, old_state, policy,
                )
            },
        )?;
        let where_reached = solver.and(&[active.clone(), membership.clone()])?;
        has_partial_operation |= where_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[where_reached.clone(), where_status.first_partial])?,
        ])?;
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?
            .unwrap_or_else(|| solver.bool_value(true));
        let body_reached = solver.and(&[
            where_reached.clone(),
            where_status.fully_defined.clone(),
            where_term.clone(),
        ])?;
        let body_status =
            evaluation_status_with_policy(solver, model, body, state, &local, old_state, policy)?;
        has_partial_operation |= body_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[body_reached.clone(), body_status.first_partial])?,
        ])?;
        let where_ok = solver.implies(&where_reached, &where_status.fully_defined)?;
        let body_ok = solver.implies(&body_reached, &body_status.fully_defined)?;
        fully_defined = solver.and(&[fully_defined, where_ok, body_ok])?;

        let body_value = eval(solver, model, body, state, &mut local, old_state)?;
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status =
        binder_source_evaluation_status(solver, model, binder, state, bindings, old_state, policy)?;
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    let mut sum = solver.int_value(0);
    for (name, candidate, membership) in
        binder_values(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, candidate);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| {
                evaluation_status_with_policy(
                    solver, model, where_expr, state, &local, old_state, policy,
                )
            },
        )?;
        let where_reached = solver.and(&[active.clone(), membership])?;
        has_partial_operation |= where_status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[where_reached.clone(), where_status.first_partial])?,
        ])?;
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?
            .unwrap_or_else(|| solver.bool_value(true));
        let mut iteration_ok = solver.implies(&where_reached, &where_status.fully_defined)?;
        if let Some(value) = value {
            let value_reached =
                solver.and(&[where_reached, where_status.fully_defined, where_term])?;
            let value_status = evaluation_status_with_policy(
                solver, model, value, state, &local, old_state, policy,
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
                let value = eval(solver, model, value, state, &mut local, old_state)?;
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    ordered_evaluation_status_refs(
        solver,
        model,
        &binder_source_expressions(binder),
        state,
        bindings,
        old_state,
        policy,
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
                let value = eval_expected(solver, model, expr, ty, state, bindings, old_state)?;
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
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let values = args
        .iter()
        .map(|arg| eval(solver, model, arg, state, bindings, old_state))
        .collect::<Result<Vec<_>, _>>()?;
    if is_collection(&receiver) {
        let (ty, mut entries) = collection_entries(solver, model, &receiver)?;
        if name == "size" && values.is_empty() {
            let terms = entries
                .iter()
                .map(|(_, present)| solver.ite(present, &solver.int_value(1), &solver.int_value(0)))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(int_value(solver, sum_terms(solver, &terms)?));
        }
        let key = values
            .iter()
            .map(|value| match value {
                SymbolicValue::Scalar { term, .. } => Ok(term.clone()),
                _ => Err(VerifyError::new("collection members must be scalar")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let grid = matches!(
            receiver,
            SymbolicValue::Set { .. } | SymbolicValue::Relation { .. }
        );
        // A nominal bounded type alone is not proof: a state value or a
        // collection binder can escape it when that bound is unselected.
        let known = literal_key(solver, &key);
        let inside = name == "remove"
            || known.as_ref().is_some_and(|known| {
                entries.iter().any(|(candidate, _)| {
                    candidate
                        .iter()
                        .zip(known)
                        .all(|(term, value)| solver.literal_value(term).as_ref() == Some(value))
                })
            });
        // Known in-carrier adds and grid removals need no new candidate slot.
        // Unknown/outside values still take the complete retention path below.
        if grid && matches!(name, "add" | "remove") && inside {
            return match receiver {
                SymbolicValue::Set { ty, entries: grid } => Ok(SymbolicValue::Set {
                    ty,
                    entries: update_grid(solver, &key, grid, &entries, name == "add")?,
                }),
                SymbolicValue::Relation { ty, entries: grid } => Ok(SymbolicValue::Relation {
                    ty,
                    entries: update_grid(solver, &key, grid, &entries, name == "add")?,
                }),
                _ => unreachable!(),
            };
        }
        let contains = collection_contains(solver, &entries, &key)?;
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
                        solver.not(&collection_key_equal(solver, candidate, &key)?)?,
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
    key: &[S::Term],
    grid: Vec<(K, S::Term)>,
    symbolic: &[(Vec<S::Term>, S::Term)],
    added: bool,
) -> Result<Vec<(K, S::Term)>, VerifyError> {
    grid.into_iter()
        .zip(symbolic)
        .map(|((key_value, present), (candidate, _))| {
            let same = collection_key_equal(solver, candidate, key)?;
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
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let (left, right) = if matches!(op, "==" | "!=") {
        eval_equality_operands(solver, model, left, right, state, bindings, old_state)?
    } else {
        (
            eval(solver, model, left, state, bindings, old_state)?,
            eval(solver, model, right, state, bindings, old_state)?,
        )
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
) -> Result<SymbolicPair<S::Term>, VerifyError> {
    let types = binding_types(bindings);
    if model.expression_type(left, &types, None)?.is_none() {
        let ty = model.equality_operand_type(left, right, &types)?;
        return Ok((
            eval_expected(solver, model, left, &ty, state, bindings, old_state)?,
            eval_expected(solver, model, right, &ty, state, bindings, old_state)?,
        ));
    }
    match eval_raw(solver, model, left, state, bindings, old_state) {
        Ok(left_value) => {
            if let Some(left_ty) = left_value.ty() {
                let right_value =
                    eval_expected(solver, model, right, left_ty, state, bindings, old_state)?;
                Ok((left_value, right_value))
            } else {
                let right_value = eval_raw(solver, model, right, state, bindings, old_state)?;
                if right_value.ty().is_none()
                    && scalar_literal(&left_value)
                    && scalar_literal(&right_value)
                {
                    let ty = model.equality_operand_type(left, right, &types)?;
                    return Ok((
                        materialize(solver, model, left_value, &ty, false)?,
                        materialize(solver, model, right_value, &ty, false)?,
                    ));
                }
                let right_ty = right_value
                    .ty()
                    .ok_or_else(|| VerifyError::new("equality requires a typed operand"))?;
                let left_value =
                    eval_expected(solver, model, left, right_ty, state, bindings, old_state)?;
                Ok((left_value, right_value))
            }
        }
        Err(error) => Err(error),
    }
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
    eval_in_context(
        solver, model, expr, expected, state, bindings, old_state, true,
    )
}

#[allow(clippy::too_many_arguments)]
fn eval_in_context<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    expected: &TypeRef,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
    contextual: bool,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
        ..
    } = expr
    {
        let condition = eval(solver, model, condition, state, bindings, old_state)?;
        let then_value = eval_in_context(
            solver, model, then_expr, expected, state, bindings, old_state, contextual,
        )?;
        let else_value = eval_in_context(
            solver, model, else_expr, expected, state, bindings, old_state, contextual,
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
            value: Box::new(eval_in_context(
                solver, model, inner, inner_ty, state, bindings, old_state, contextual,
            )?),
        });
    }
    let value = eval_raw(solver, model, expr, state, bindings, old_state)?;
    materialize(solver, model, value, expected, contextual)
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
            // Never overwrite a name already in scope (#1153); see the concrete
            // evaluator. A new name still reaches the action body.
            if !bindings.contains_key(name) {
                // The concrete evaluator binds only when the operand is
                // present (#1259): where it is absent, a name that also
                // resolves to a state variable, constant or enum member keeps
                // that meaning. `check` rejects such a collision, so this only
                // keeps the engines agreeing on a model that bypassed it.
                let bound = match lookup(solver, model, name, state, bindings) {
                    Ok(outer) if outer.ty() == value.ty() => {
                        ite_value(solver, model, &present, &value, &outer)?
                    }
                    _ => *value,
                };
                bindings.insert(name.clone(), bound);
            }
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
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let candidates = binder_values(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let body = eval(solver, model, body, state, &mut local, old_state)?;
        let body = bool_term(&body)?.clone();
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
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
) -> Result<Vec<S::Term>, VerifyError> {
    let candidates = binder_values(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
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
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if kind != AggregateKind::Sum {
        let conditions = binder_conditions(solver, model, binder, state, bindings, old_state)?;
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
        binder_values(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, candidate);
        let value = eval(solver, model, value, state, &mut local, old_state)?;
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        let condition = binder_condition(solver, membership, where_term)?;
        terms.push(solver.ite(&condition, int_term(&value)?, &solver.int_value(0))?);
    }
    Ok(int_value(solver, sum_terms(solver, &terms)?))
}

type ConditionalBinderCandidates<T> = Vec<(String, SymbolicValue<T>, Option<T>)>;
type TypedBinderCandidates<T> = Vec<(String, TypedBinding<T>, Option<T>)>;

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

pub(crate) fn binder_values<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<TypedBinderCandidates<S::Term>, VerifyError> {
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
    let collection = eval(solver, model, collection, state, &mut local, old_state)?;
    let candidates: ConditionalBinderCandidates<S::Term> = match collection {
        SymbolicValue::Collection { ty, entries } => {
            let TypeRef::Set(element_ty) = ty else {
                return Err(VerifyError::new("collection binder requires a Set"));
            };
            ordered_set_candidates(
                solver,
                model,
                name,
                entries
                    .into_iter()
                    .map(|(key, present)| {
                        (
                            SymbolicValue::Scalar {
                                ty: element_ty.as_ref().clone(),
                                term: key[0].clone(),
                            },
                            present,
                        )
                    })
                    .collect(),
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
    let mut ordered = items
        .into_iter()
        .map(|(value, present)| Ok((set_order_key(solver, model, &value)?, value, present)))
        .collect::<Result<Vec<_>, VerifyError>>()?;
    // A fixed insertion network orders symbolic values even when their
    // relative order depends on the current state or action arguments.
    for end in 1..ordered.len() {
        for index in (1..=end).rev() {
            let (left_key, left, left_present) = &ordered[index - 1];
            let (right_key, right, right_present) = &ordered[index];
            let swap = solver.gt(left_key, right_key)?;
            let lower = (
                solver.ite(&swap, right_key, left_key)?,
                ite_value(solver, model, &swap, right, left)?,
                solver.ite(&swap, right_present, left_present)?,
            );
            let upper = (
                solver.ite(&swap, left_key, right_key)?,
                ite_value(solver, model, &swap, left, right)?,
                solver.ite(&swap, left_present, right_present)?,
            );
            ordered[index - 1] = lower;
            ordered[index] = upper;
        }
    }
    ordered
        .iter()
        .enumerate()
        .map(|(index, (_, value, present))| {
            let mut membership = present.clone();
            for (_, previous, previous_present) in &ordered[..index] {
                let duplicate = solver.and(&[
                    previous_present.clone(),
                    logical_equal(solver, model, value, previous)?,
                ])?;
                membership = solver.and(&[membership, solver.not(&duplicate)?])?;
            }
            Ok((name.to_owned(), value.clone(), Some(membership)))
        })
        .collect()
}

/// Scalar keys follow concrete Value order, including enum member spelling.
/// Composite literal elements remain unsupported rather than guessed.
fn set_order_key<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &SymbolicValue<S::Term>,
) -> Result<S::Term, VerifyError> {
    let SymbolicValue::Scalar { ty, term } = value else {
        return Err(VerifyError::new(
            "Set literal binder requires scalar elements",
        ));
    };
    match ty {
        TypeRef::Bool => Ok(solver.ite(term, &solver.int_value(1), &solver.int_value(0))?),
        TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Enum { .. })) => {
            let mut values = model.domain_values(ty)?;
            values.sort();
            let mut rank = solver.int_value(0);
            for (index, value) in values.iter().enumerate() {
                let value = concrete_value(solver, model, ty, value)?;
                let matches = solver.equal(term, int_term(&value)?)?;
                rank = solver.ite(&matches, &solver.int_value(i64_index(index)?), &rank)?;
            }
            Ok(rank)
        }
        _ => Ok(term.clone()),
    }
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
            let ty = TypeRef::Named(type_name);
            model
                .domain_values(&ty)?
                .into_iter()
                .map(|value| Ok((name.clone(), concrete_value(solver, model, &ty, &value)?)))
                .collect()
        }
        Binder::Range { name, lo, hi, .. } => {
            let lo = static_int(lo, model)?;
            let hi = static_int(hi, model)?;
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
    binder_where_expression(binder)
        .map(|expr| {
            let value = eval(solver, model, expr, state, bindings, old_state)?;
            Ok(bool_term(&value)?.clone())
        })
        .transpose()
}

fn static_int(expr: &Expr, model: &KernelModel) -> Result<i64, VerifyError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(FslValue::Int(value)) => Ok(*value),
            _ => Err(VerifyError::new(format!(
                "'{name}' is not an integer const"
            ))),
        },
        Expr::Neg(inner) => static_int(inner, model)?
            .checked_neg()
            .ok_or_else(|| VerifyError::new("integer overflow in static negation")),
        Expr::Binary { op, left, right } => {
            let left = static_int(left, model)?;
            let right = static_int(right, model)?;
            match op.as_str() {
                "+" => left.checked_add(right),
                "-" => left.checked_sub(right),
                "*" => left.checked_mul(right),
                _ => None,
            }
            .ok_or_else(|| VerifyError::new("invalid static integer expression"))
        }
        _ => Err(VerifyError::new("binder bound is not a static integer")),
    }
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
