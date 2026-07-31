// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::recursion;
use fsl_core::{
    FslValue, KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr,
    KernelModel, Pattern, TypeDef, TypeRef,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::value::{
    Bindings, SymbolicState, SymbolicValue, bool_term, coerce, concrete_value, i64_index, int_term,
    ite_value, logical_equal, select_finite,
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
        Expr::EnumMember { type_name, member } => concrete_value(
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
            let condition = eval(solver, model, condition, state, bindings, old_state)?;
            let then_value = eval(solver, model, then_expr, state, bindings, old_state)?;
            let else_value = eval(solver, model, else_expr, state, bindings, old_state)?;
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
            eval_pattern(solver, value, pattern, bindings)
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
                let SymbolicValue::Relation { entries, .. } = value else {
                    return Err(VerifyError::new(format!("{name}() requires a relation")));
                };
                let same_key = |left: &(FslValue, FslValue), right: &(FslValue, FslValue)| {
                    if name == "rel_functional" {
                        left.0 == right.0
                    } else {
                        left.1 == right.1
                    }
                };
                let mut clauses = Vec::new();
                for (index, (key, present)) in entries.iter().enumerate() {
                    for (other_key, other_present) in entries.iter().skip(index + 1) {
                        if same_key(key, other_key) {
                            clauses
                                .push(solver.not(
                                    &solver.and(&[present.clone(), other_present.clone()])?,
                                )?);
                        }
                    }
                }
                Ok(bool_value(solver, solver.and(&clauses)?))
            }
            "rel_domain" | "rel_range" => {
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                let SymbolicValue::Relation { ty, entries } = value else {
                    return Err(VerifyError::new(format!("{name}() requires a relation")));
                };
                let TypeRef::Relation(source_ty, target_ty) = &ty else {
                    unreachable!();
                };
                let (element_ty, values) = if name == "rel_domain" {
                    (source_ty.as_ref().clone(), model.domain_values(source_ty)?)
                } else {
                    (target_ty.as_ref().clone(), model.domain_values(target_ty)?)
                };
                let mut element_entries = Vec::with_capacity(values.len());
                for element in values {
                    let present_terms = entries
                        .iter()
                        .filter(|((source, target), _)| {
                            if name == "rel_domain" {
                                source == &element
                            } else {
                                target == &element
                            }
                        })
                        .map(|(_, present)| present.clone())
                        .collect::<Vec<_>>();
                    element_entries.push((element, solver.or(&present_terms)?));
                }
                Ok(SymbolicValue::Set {
                    ty: TypeRef::Set(Box::new(element_ty)),
                    entries: element_entries,
                })
            }
            "rel_acyclic" => {
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                let SymbolicValue::Relation { ty, entries } = value else {
                    return Err(VerifyError::new("acyclic() requires a relation"));
                };
                let TypeRef::Relation(source_ty, target_ty) = &ty else {
                    unreachable!();
                };
                if source_ty != target_ty {
                    return Err(VerifyError::new(
                        "acyclic() requires a self-relation (relation T -> T)",
                    ));
                }
                let reach = relation_reachability_table(solver, model, &entries, source_ty)?;
                let mut cyclic_terms = Vec::new();
                for ((source, target), present) in &entries {
                    let back = reach
                        .get(&(target.clone(), source.clone()))
                        .ok_or_else(|| VerifyError::new("relation reachability table gap"))?;
                    cyclic_terms.push(solver.and(&[present.clone(), back.clone()])?);
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
            let SymbolicValue::Relation { ty, entries } = relation else {
                return Err(VerifyError::new("reachable() requires a relation"));
            };
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
            let reach = relation_reachability_table(solver, model, &entries, source_ty)?;
            let mut terms = Vec::with_capacity(reach.len());
            for ((from, to), reachable) in &reach {
                let from_term = concrete_value(solver, model, source_ty, from)?;
                let to_term = concrete_value(solver, model, target_ty, to)?;
                let same_source = logical_equal(solver, model, &source, &from_term)?;
                let same_target = logical_equal(solver, model, &target, &to_term)?;
                terms.push(solver.and(&[same_source, same_target, reachable.clone()])?);
            }
            Ok(bool_value(solver, solver.or(&terms)?))
        }
        Expr::TernaryNamed { name, .. } => Err(VerifyError::new(format!(
            "unsupported ternary expression '{name}'"
        ))),
    }
}

/// Bounded-hop symbolic reachability closure over a relation's finite
/// (source, target) domain grid, memoized by iterative relaxation (hop bound
/// = domain size, sound for both `reachable()` and `acyclic()`'s cycle
/// check). Ports the frozen Python symbolic reference's
/// `bmc.py::_relation_reachable_expr` (path of at least one edge -- see the
/// base-case note below for why this intentionally does not match the
/// concrete Monitor's own trivial-self-reachability convention).
fn relation_reachability_table<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    entries: &[((FslValue, FslValue), S::Term)],
    domain_ty: &TypeRef,
) -> Result<BTreeMap<(FslValue, FslValue), S::Term>, VerifyError> {
    let values = model.domain_values(domain_ty)?;
    let direct: BTreeMap<(FslValue, FslValue), S::Term> = entries.iter().cloned().collect();
    // Base case: a path of exactly one hop (a direct edge). Unlike the
    // concrete Monitor's BFS oracle (which treats a node as trivially
    // "reachable" from itself with zero hops), the frozen Python symbolic
    // reference (`bmc.py::_relation_reachable_expr`) requires at least one
    // edge -- `reachable(r, a, a)` is only true when `a` has an actual path
    // back to itself (e.g. a self-loop or a cycle through it). Matching the
    // symbolic reference here (not the concrete one) is what this port's
    // `--engine bmc`/`induction` verdicts are graded against.
    let mut reach: BTreeMap<(FslValue, FslValue), S::Term> = direct.clone();
    for _ in 0..values.len() {
        let mut next = BTreeMap::new();
        for a in &values {
            for b in &values {
                let mut terms = vec![reach[&(a.clone(), b.clone())].clone()];
                for c in &values {
                    let via_c = solver.and(&[
                        reach[&(a.clone(), c.clone())].clone(),
                        direct[&(c.clone(), b.clone())].clone(),
                    ])?;
                    terms.push(via_c);
                }
                next.insert((a.clone(), b.clone()), solver.or(&terms)?);
            }
        }
        reach = next;
    }
    Ok(reach)
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

pub(crate) fn expression_has_partial_operation_candidate(expr: &Expr) -> bool {
    match expr {
        Expr::Index(_, _) => true,
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            matches!(name.as_str(), "head" | "pop" | "at")
                || expression_has_partial_operation_candidate(receiver)
                || args.iter().any(expression_has_partial_operation_candidate)
        }
        Expr::Binary { op, left, right } => {
            matches!(op.as_str(), "/" | "%")
                || expression_has_partial_operation_candidate(left)
                || expression_has_partial_operation_candidate(right)
        }
        Expr::Some(inner)
        | Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::Field(inner, _)
        | Expr::Stage { entity: inner, .. }
        | Expr::UnaryNamed { expr: inner, .. }
        | Expr::Is { expr: inner, .. } => expression_has_partial_operation_candidate(inner),
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            items.iter().any(expression_has_partial_operation_candidate)
        }
        Expr::Struct { fields, .. } => fields
            .iter()
            .any(|(_, value)| expression_has_partial_operation_candidate(value)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            expression_has_partial_operation_candidate(condition)
                || expression_has_partial_operation_candidate(then_expr)
                || expression_has_partial_operation_candidate(else_expr)
        }
        Expr::Quantified { binder, body, .. } => {
            binder_has_partial_operation_candidate(binder)
                || expression_has_partial_operation_candidate(body)
        }
        Expr::Aggregate { binder, value, .. } => {
            binder_has_partial_operation_candidate(binder)
                || value
                    .as_deref()
                    .is_some_and(expression_has_partial_operation_candidate)
        }
        Expr::BinaryNamed { left, right, .. } => {
            expression_has_partial_operation_candidate(left)
                || expression_has_partial_operation_candidate(right)
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            expression_has_partial_operation_candidate(first)
                || expression_has_partial_operation_candidate(second)
                || expression_has_partial_operation_candidate(third)
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => false,
    }
}

pub(crate) fn binder_has_partial_operation_candidate(binder: &Binder) -> bool {
    match binder {
        Binder::Typed { where_expr, .. } => where_expr
            .as_deref()
            .is_some_and(expression_has_partial_operation_candidate),
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            expression_has_partial_operation_candidate(lo)
                || expression_has_partial_operation_candidate(hi)
                || where_expr
                    .as_deref()
                    .is_some_and(expression_has_partial_operation_candidate)
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            expression_has_partial_operation_candidate(collection)
                || where_expr
                    .as_deref()
                    .is_some_and(expression_has_partial_operation_candidate)
        }
    }
}

fn safe_status<S: SmtSolver>(solver: &S) -> EvaluationStatus<S::Term> {
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
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {
            Ok(safe_status(solver))
        }
        Expr::UnaryNamed {
            name, expr: inner, ..
        } if name == "old" => evaluation_status(
            solver,
            model,
            inner,
            old_state.ok_or_else(|| VerifyError::new("old() used without old state"))?,
            bindings,
            None,
        ),
        Expr::Neg(inner) => {
            let mut local = bindings.clone();
            let inner_status = evaluation_status(solver, model, inner, state, &local, old_state)?;
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
            let inner_status = evaluation_status(solver, model, inner, state, &local, old_state)?;
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
            evaluation_status(solver, model, inner, state, bindings, old_state)
        }
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            ordered_evaluation_status(solver, model, items, state, bindings, old_state)
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
            ordered_evaluation_status_refs(solver, model, &items, state, bindings, old_state)
        }
        Expr::Index(base, index) => {
            let mut local = bindings.clone();
            let base_status = evaluation_status(solver, model, base, state, &local, old_state)?;
            let base_value = eval(solver, model, base, state, &mut local, old_state)?;
            let index_status = evaluation_status(solver, model, index, state, &local, old_state)?;
            let index_value = eval(solver, model, index, state, &mut local, old_state)?;
            let fully_accessible = index_accessible(solver, model, &base_value, &index_value)?;
            let partial_accessible =
                partial_operation_index_accessible(solver, &base_value, &index_value)?;
            sequence_statuses(
                solver,
                [
                    base_status,
                    index_status,
                    EvaluationStatus {
                        fully_defined: fully_accessible,
                        first_partial: solver.not(&partial_accessible)?,
                        has_partial_operation: matches!(base_value, SymbolicValue::Seq { .. }),
                    },
                ],
            )
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let mut local = bindings.clone();
            let receiver_status =
                evaluation_status(solver, model, receiver, state, &local, old_state)?;
            let receiver_value = eval(solver, model, receiver, state, &mut local, old_state)?;
            let arguments_status =
                ordered_evaluation_status(solver, model, args, state, &local, old_state)?;
            let argument_values = args
                .iter()
                .map(|argument| eval(solver, model, argument, state, &mut local, old_state))
                .collect::<Result<Vec<_>, _>>()?;
            let operation_defined =
                method_definedness(solver, name, &receiver_value, argument_values.as_slice())?;
            sequence_statuses(
                solver,
                [
                    receiver_status,
                    arguments_status,
                    EvaluationStatus {
                        fully_defined: operation_defined.clone(),
                        first_partial: solver.not(&operation_defined)?,
                        has_partial_operation: matches!(
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
            let left_status = evaluation_status(solver, model, left, state, &local, old_state)?;
            let left_value = eval(solver, model, left, state, &mut local, old_state)?;
            let right_status = evaluation_status(solver, model, right, state, &local, old_state)?;
            let right_value = eval(solver, model, right, state, &mut local, old_state)?;
            let reached_right = match op.as_str() {
                "and" | "=>" => bool_term(&left_value)?.clone(),
                "or" => solver.not(bool_term(&left_value)?)?,
                _ => solver.bool_value(true),
            };
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
                let zero = solver.equal(int_term(&right_value)?, &solver.int_value(0))?;
                parts.push(solver.not(&zero)?);
                let overflow = solver.and(&[
                    solver.equal(int_term(&left_value)?, &solver.int_value(i64::MIN))?,
                    solver.equal(int_term(&right_value)?, &solver.int_value(-1))?,
                ])?;
                parts.push(solver.not(&overflow)?);
                operation_partial = solver.and(&[operands_defined, zero])?;
            } else if matches!(op.as_str(), "+" | "-" | "*") {
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
                    || matches!(op.as_str(), "/" | "%"),
            })
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            let mut local = bindings.clone();
            let condition_status =
                evaluation_status(solver, model, condition, state, &local, old_state)?;
            let condition_value = eval(solver, model, condition, state, &mut local, old_state)?;
            let then_status =
                evaluation_status(solver, model, then_expr, state, &local, old_state)?;
            let else_status =
                evaluation_status(solver, model, else_expr, state, &local, old_state)?;
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
            solver, model, quantifier, binder, body, state, bindings, old_state,
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
        ),
        Expr::BinaryNamed { left, right, .. } => ordered_evaluation_status_refs(
            solver,
            model,
            &[left.as_ref(), right.as_ref()],
            state,
            bindings,
            old_state,
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let expressions = expressions.iter().collect::<Vec<_>>();
    ordered_evaluation_status_refs(solver, model, &expressions, state, bindings, old_state)
}

fn ordered_evaluation_status_refs<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[&Expr],
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let mut local = bindings.clone();
    let mut statuses = Vec::new();
    for expression in expressions {
        statuses.push(evaluation_status(
            solver, model, expression, state, &local, old_state,
        )?);
        let _ = eval(solver, model, expression, state, &mut local, old_state)?;
    }
    sequence_statuses(solver, statuses)
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

fn method_definedness<S: SmtSolver>(
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status =
        binder_source_evaluation_status(solver, model, binder, state, bindings, old_state)?;
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    for (name, value, membership) in
        binder_candidates(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, value);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| evaluation_status(solver, model, where_expr, state, &local, old_state),
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
        let body_status = evaluation_status(solver, model, body, state, &local, old_state)?;
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let source_status =
        binder_source_evaluation_status(solver, model, binder, state, bindings, old_state)?;
    let mut fully_defined = source_status.fully_defined.clone();
    let mut first_partial = source_status.first_partial;
    let mut has_partial_operation = source_status.has_partial_operation;
    let mut active = source_status.fully_defined;
    let mut sum = solver.int_value(0);
    for (name, candidate, membership) in
        binder_candidates(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, candidate);
        let membership = membership.unwrap_or_else(|| solver.bool_value(true));
        let where_status = binder_where_expression(binder).map_or_else(
            || Ok(safe_status(solver)),
            |where_expr| evaluation_status(solver, model, where_expr, state, &local, old_state),
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
            let value_status = evaluation_status(solver, model, value, state, &local, old_state)?;
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

fn i64_term_is_in_range<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<S::Term, VerifyError> {
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
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match binder {
        Binder::Typed { .. } => Ok(safe_status(solver)),
        Binder::Range { lo, hi, .. } => ordered_evaluation_status_refs(
            solver,
            model,
            &[lo.as_ref(), hi.as_ref()],
            state,
            bindings,
            old_state,
        ),
        Binder::Collection { collection, .. } => {
            evaluation_status(solver, model, collection, state, bindings, old_state)
        }
    }
}

fn binder_where_expression(binder: &Binder) -> Option<&Expr> {
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
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                Ok((field.clone(), coerce(solver, model, value, ty)?))
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
        SymbolicValue::Seq { slots, .. } => {
            let entries = slots
                .iter()
                .enumerate()
                .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
                .collect::<Result<Vec<_>, VerifyError>>()?;
            select_finite(solver, model, &entries, index, &TypeRef::Int)
        }
        _ => Err(VerifyError::new("indexing requires a map or sequence")),
    }
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
    match receiver {
        SymbolicValue::Set { ty, entries } => {
            let TypeRef::Set(element_ty) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("contains", [value]) => {
                    let terms = entries
                        .iter()
                        .map(|(element, present)| {
                            let element = concrete_value(solver, model, element_ty, element)?;
                            let same = logical_equal(solver, model, value, &element)?;
                            Ok(solver.and(&[same, present.clone()])?)
                        })
                        .collect::<Result<Vec<_>, VerifyError>>()?;
                    Ok(bool_value(solver, solver.or(&terms)?))
                }
                ("add" | "remove", [value]) => {
                    let added = name == "add";
                    let entries = entries
                        .into_iter()
                        .map(|(element, present)| {
                            let symbolic = concrete_value(solver, model, element_ty, &element)?;
                            let same = logical_equal(solver, model, value, &symbolic)?;
                            Ok((
                                element,
                                solver.ite(&same, &solver.bool_value(added), &present)?,
                            ))
                        })
                        .collect::<Result<_, VerifyError>>()?;
                    Ok(SymbolicValue::Set { ty, entries })
                }
                ("size", []) => {
                    let terms = entries
                        .iter()
                        .map(|(_, present)| {
                            solver
                                .ite(present, &solver.int_value(1), &solver.int_value(0))
                                .map_err(VerifyError::from)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(int_value(solver, sum_terms(solver, &terms)?))
                }
                _ => Err(VerifyError::new(format!("invalid Set method '{name}'"))),
            }
        }
        SymbolicValue::Seq { ty, slots, len } => {
            let TypeRef::Seq(element_ty, capacity) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("push", [value]) => {
                    let value = coerce(solver, model, value.clone(), element_ty)?;
                    let slots = slots
                        .iter()
                        .enumerate()
                        .map(|(index, old)| {
                            let matches =
                                solver.equal(&len, &solver.int_value(i64_index(index)?))?;
                            ite_value(solver, model, &matches, &value, old)
                        })
                        .collect::<Result<_, VerifyError>>()?;
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
                ("head", []) => slots
                    .first()
                    .cloned()
                    .ok_or_else(|| VerifyError::new("head() on zero-capacity sequence")),
                ("at", [index]) => {
                    let entries = slots
                        .iter()
                        .enumerate()
                        .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
                        .collect::<Result<Vec<_>, VerifyError>>()?;
                    select_finite(solver, model, &entries, index, &TypeRef::Int)
                }
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
        SymbolicValue::Relation { ty, entries } => {
            let TypeRef::Relation(source_ty, target_ty) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("contains", [source, target]) => {
                    let mut terms = Vec::with_capacity(entries.len());
                    for ((entry_source, entry_target), present) in &entries {
                        let source_term = concrete_value(solver, model, source_ty, entry_source)?;
                        let target_term = concrete_value(solver, model, target_ty, entry_target)?;
                        let same_source = logical_equal(solver, model, source, &source_term)?;
                        let same_target = logical_equal(solver, model, target, &target_term)?;
                        terms.push(solver.and(&[same_source, same_target, present.clone()])?);
                    }
                    Ok(bool_value(solver, solver.or(&terms)?))
                }
                ("add" | "remove", [source, target]) => {
                    let added = name == "add";
                    let entries = entries
                        .into_iter()
                        .map(|((entry_source, entry_target), present)| {
                            let source_term =
                                concrete_value(solver, model, source_ty, &entry_source)?;
                            let target_term =
                                concrete_value(solver, model, target_ty, &entry_target)?;
                            let same_source = logical_equal(solver, model, source, &source_term)?;
                            let same_target = logical_equal(solver, model, target, &target_term)?;
                            let matches = solver.and(&[same_source, same_target])?;
                            Ok((
                                (entry_source, entry_target),
                                solver.ite(&matches, &solver.bool_value(added), &present)?,
                            ))
                        })
                        .collect::<Result<_, VerifyError>>()?;
                    Ok(SymbolicValue::Relation { ty, entries })
                }
                _ => Err(VerifyError::new(format!(
                    "invalid relation method '{name}'"
                ))),
            }
        }
        _ => Err(VerifyError::new(
            "method receiver has no collection methods",
        )),
    }
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
    match eval(solver, model, left, state, bindings, old_state) {
        Ok(left_value) => {
            if let Some(left_ty) = left_value.ty() {
                let right_value =
                    eval_expected(solver, model, right, left_ty, state, bindings, old_state)?;
                Ok((left_value, right_value))
            } else {
                let right_value = eval(solver, model, right, state, bindings, old_state)?;
                let right_ty = right_value
                    .ty()
                    .ok_or_else(|| VerifyError::new("equality requires a typed operand"))?;
                let left_value =
                    eval_expected(solver, model, left, right_ty, state, bindings, old_state)?;
                Ok((left_value, right_value))
            }
        }
        Err(_) if requires_expected_type(left) => {
            let right_value = eval(solver, model, right, state, bindings, old_state)?;
            let right_ty = right_value
                .ty()
                .ok_or_else(|| VerifyError::new("equality requires a typed operand"))?;
            let left_value =
                eval_expected(solver, model, left, right_ty, state, bindings, old_state)?;
            Ok((left_value, right_value))
        }
        Err(error) => Err(error),
    }
}

fn requires_expected_type(expr: &Expr) -> bool {
    match expr {
        Expr::None => true,
        Expr::Some(inner) => requires_expected_type(inner),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_expected<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    expected: &TypeRef,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if let (Expr::Some(inner), TypeRef::Option(inner_ty)) = (expr, expected) {
        return Ok(SymbolicValue::Option {
            ty: expected.clone(),
            present: solver.bool_value(true),
            value: Box::new(eval_expected(
                solver, model, inner, inner_ty, state, bindings, old_state,
            )?),
        });
    }
    let value = eval(solver, model, expr, state, bindings, old_state)?;
    coerce(solver, model, value, expected)
}

fn eval_pattern<S: SmtSolver>(
    solver: &S,
    value: SymbolicValue<S::Term>,
    pattern: &Pattern,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match (value, pattern) {
        (SymbolicValue::None, Pattern::None) => Ok(bool_value(solver, solver.bool_value(true))),
        (SymbolicValue::None, Pattern::Some(_)) => Ok(bool_value(solver, solver.bool_value(false))),
        (SymbolicValue::Option { present, .. }, Pattern::None) => {
            Ok(bool_value(solver, solver.not(&present)?))
        }
        (SymbolicValue::Option { present, value, .. }, Pattern::Some(name)) => {
            bindings.insert(name.clone(), *value);
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
    let candidates = binder_candidates(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let body = eval(solver, model, body, state, &mut local, old_state)?;
        let body = bool_term(&body)?.clone();
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        let condition = match (membership, where_term) {
            (Some(membership), Some(where_term)) => Some(solver.and(&[membership, where_term])?),
            (Some(condition), None) | (None, Some(condition)) => Some(condition),
            (None, None) => None,
        };
        terms.push(if let Some(condition) = condition {
            if quantifier == "forall" {
                solver.implies(&condition, &body)?
            } else {
                solver.and(&[condition, body])?
            }
        } else {
            body
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
    let candidates = binder_candidates(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        terms.push(match (membership, where_term) {
            (Some(membership), Some(where_term)) => solver.and(&[membership, where_term])?,
            (Some(condition), None) | (None, Some(condition)) => condition,
            (None, None) => solver.bool_value(true),
        });
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
        binder_candidates(solver, model, binder, state, bindings, old_state)?
    {
        let mut local = bindings.clone();
        local.insert(name, candidate);
        let value = eval(solver, model, value, state, &mut local, old_state)?;
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        let condition = match (membership, where_term) {
            (Some(membership), Some(where_term)) => Some(solver.and(&[membership, where_term])?),
            (Some(condition), None) | (None, Some(condition)) => Some(condition),
            (None, None) => None,
        };
        terms.push(if let Some(condition) = condition {
            solver.ite(&condition, int_term(&value)?, &solver.int_value(0))?
        } else {
            int_term(&value)?.clone()
        });
    }
    Ok(int_value(solver, sum_terms(solver, &terms)?))
}

type ConditionalBinderCandidates<T> = Vec<(String, SymbolicValue<T>, Option<T>)>;

fn binder_candidates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<ConditionalBinderCandidates<S::Term>, VerifyError> {
    let Binder::Collection {
        name, collection, ..
    } = binder
    else {
        return binder_values(solver, model, binder).map(|values| {
            values
                .into_iter()
                .map(|(name, value)| (name, value, None))
                .collect()
        });
    };
    let mut local = bindings.clone();
    let collection = eval(solver, model, collection, state, &mut local, old_state)?;
    match collection {
        SymbolicValue::Set { ty, entries } => {
            let TypeRef::Set(element_ty) = ty else {
                unreachable!();
            };
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
        _ => Err(VerifyError::new(
            "collection binder expects a Set or Seq expression",
        )),
    }
}

pub(crate) fn binder_values<S: SmtSolver>(
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
    let where_expr = match binder {
        Binder::Typed { where_expr, .. }
        | Binder::Range { where_expr, .. }
        | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
    };
    where_expr
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
