// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{
    ActionDef, ActionGuard, FslValue, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, ParamDef, TypeRef,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::eval::{
    EvaluationStatus, binder_has_partial_operation_candidate, binder_values, binder_where, eval,
    eval_expected, evaluation_status, expression_has_partial_operation_candidate, index_accessible,
    partial_operation_index_accessible, sequence_statuses,
};
use crate::value::{
    Bindings, SymbolicState, SymbolicValue, bool_term, coerce, i64_index, ite_value, logical_equal,
    select_finite, store_finite,
};

type GuardEvaluation<T> = (Vec<T>, Bindings<T>);

#[derive(Clone, Debug)]
pub(crate) struct ActionInstance<T> {
    pub action_index: usize,
    pub action: String,
    pub params: Bindings<T>,
    /// Concrete parameter values in declaration order, alongside `params`'
    /// solver terms, so host-side code (leadsTo `helpful` matching) can
    /// compare instances without a solver round-trip.
    pub concrete_params: BTreeMap<String, FslValue>,
}

pub(crate) struct ActionGuardDefinedness<T> {
    pub enabled: T,
    pub defined: T,
    pub first_partial: T,
    pub has_partial_operation: bool,
    pub bindings: Bindings<T>,
}

pub(crate) fn action_has_partial_operation_candidate(action: &ActionDef) -> bool {
    action.guards.iter().any(|guard| match guard {
        ActionGuard::Let(_, expr) | ActionGuard::Requires(expr) => {
            expression_has_partial_operation_candidate(expr)
        }
    }) || action
        .statements
        .iter()
        .any(statement_has_partial_operation_candidate)
        || action
            .ensures
            .iter()
            .any(expression_has_partial_operation_candidate)
}

fn statement_has_partial_operation_candidate(statement: &Statement) -> bool {
    match statement {
        Statement::Assign { target, value, .. } => {
            expression_has_partial_operation_candidate(value)
                || lvalue_has_partial_operation_candidate(target)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            expression_has_partial_operation_candidate(condition)
                || then_statements
                    .iter()
                    .chain(else_statements)
                    .any(statement_has_partial_operation_candidate)
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            binder_has_partial_operation_candidate(binder)
                || statements
                    .iter()
                    .any(statement_has_partial_operation_candidate)
        }
    }
}

fn lvalue_has_partial_operation_candidate(target: &LValue) -> bool {
    match target {
        LValue::Var(_) => false,
        LValue::Index(_, _) => true,
        LValue::Field(base, _) => lvalue_has_partial_operation_candidate(base),
    }
}

pub(crate) fn action_instances<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
) -> Result<Vec<ActionInstance<S::Term>>, VerifyError> {
    let mut instances = Vec::new();
    for (action_index, action) in model.actions.iter().enumerate() {
        let mut bindings = vec![(Bindings::new(), BTreeMap::new())];
        for param in &action.params {
            let values = match param {
                ParamDef::Typed { ty, .. } => model.domain_values(ty)?,
                ParamDef::Range { lo, hi, .. } => (*lo..=*hi).map(FslValue::Int).collect(),
            };
            let ty = match param {
                ParamDef::Typed { ty, .. } => ty.clone(),
                ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
            };
            let mut next = Vec::new();
            for (existing_terms, existing_concrete) in bindings {
                for value in &values {
                    let mut terms = existing_terms.clone();
                    terms.insert(
                        param.name().to_owned(),
                        crate::value::concrete_value(solver, model, &ty, value)?,
                    );
                    let mut concrete = existing_concrete.clone();
                    concrete.insert(param.name().to_owned(), value.clone());
                    next.push((terms, concrete));
                }
            }
            bindings = next;
        }
        instances.extend(
            bindings
                .into_iter()
                .map(|(params, concrete_params)| ActionInstance {
                    action_index,
                    action: action.name.clone(),
                    params,
                    concrete_params,
                }),
        );
    }
    Ok(instances)
}

pub(crate) fn action_guards<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    params: &Bindings<S::Term>,
) -> Result<GuardEvaluation<S::Term>, VerifyError> {
    let mut bindings = params.clone();
    let mut guards = Vec::new();
    for guard in &action.guards {
        match guard {
            ActionGuard::Let(name, expr) => {
                let value = eval(solver, model, expr, state, &mut bindings, None)?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(expr) => {
                let value = eval(solver, model, expr, state, &mut bindings, None)?;
                guards.push(bool_term(&value)?.clone());
            }
        }
    }
    Ok((guards, bindings))
}

pub(crate) fn action_guard_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    params: &Bindings<S::Term>,
) -> Result<ActionGuardDefinedness<S::Term>, VerifyError> {
    let mut bindings = params.clone();
    let mut reaches_guard = solver.bool_value(true);
    let mut fully_defined = solver.bool_value(true);
    let mut first_partial = solver.bool_value(false);
    let mut has_partial_operation = false;
    for guard in &action.guards {
        let expression = match guard {
            ActionGuard::Let(_, expression) | ActionGuard::Requires(expression) => expression,
        };
        let status = evaluation_status(solver, model, expression, state, &bindings, None)?;
        has_partial_operation |= status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[reaches_guard.clone(), status.first_partial])?,
        ])?;
        fully_defined = solver.and(&[
            fully_defined,
            solver.implies(&reaches_guard, &status.fully_defined)?,
        ])?;
        let value = eval(solver, model, expression, state, &mut bindings, None)?;
        match guard {
            ActionGuard::Let(name, _) => {
                reaches_guard = solver.and(&[reaches_guard, status.fully_defined])?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(_) => {
                reaches_guard = solver.and(&[
                    reaches_guard,
                    status.fully_defined,
                    bool_term(&value)?.clone(),
                ])?;
            }
        }
    }
    Ok(ActionGuardDefinedness {
        enabled: reaches_guard,
        defined: fully_defined,
        first_partial,
        has_partial_operation,
        bindings,
    })
}

pub(crate) fn action_statements_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    Ok(action_statements_evaluation_status(solver, model, action, state, bindings)?.fully_defined)
}

pub(crate) fn action_statements_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    statements_evaluation_status(
        solver,
        model,
        &action.statements,
        state,
        &mut bindings.clone(),
    )
}

fn statements_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let statuses = statements
        .iter()
        .map(|statement| {
            statement_evaluation_status(solver, model, statement, read_state, bindings)
        })
        .collect::<Result<Vec<_>, _>>()?;
    sequence_statuses(solver, statuses)
}

#[allow(clippy::too_many_lines)]
fn statement_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value_status = evaluation_status(solver, model, value, read_state, bindings, None)?;
            let target_ty = model
                .state_lvalue_type(target)
                .map_err(|error| VerifyError::new(error.message))?;
            let _ = eval_expected(solver, model, value, &target_ty, read_state, bindings, None)?;
            let target_status =
                lvalue_evaluation_status(solver, model, target, read_state, bindings)?;
            sequence_statuses(solver, [value_status, target_status])
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition_status =
                evaluation_status(solver, model, condition, read_state, bindings, None)?;
            let condition = eval(solver, model, condition, read_state, bindings, None)?;
            let then_status = statements_evaluation_status(
                solver,
                model,
                then_statements,
                read_state,
                &mut bindings.clone(),
            )?;
            let else_status = statements_evaluation_status(
                solver,
                model,
                else_statements,
                read_state,
                &mut bindings.clone(),
            )?;
            let branch_defined = solver.ite(
                bool_term(&condition)?,
                &then_status.fully_defined,
                &else_status.fully_defined,
            )?;
            let branch_partial = solver.ite(
                bool_term(&condition)?,
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
        Statement::ForAll {
            binder, statements, ..
        } => {
            let mut active = solver.bool_value(true);
            let mut fully_defined = solver.bool_value(true);
            let mut first_partial = solver.bool_value(false);
            let mut has_partial_operation = false;
            for (name, value) in binder_values(solver, model, binder)? {
                let mut local = bindings.clone();
                local.insert(name, value);
                let where_status = match binder {
                    fsl_core::KernelBinder::Typed { where_expr, .. }
                    | fsl_core::KernelBinder::Range { where_expr, .. }
                    | fsl_core::KernelBinder::Collection { where_expr, .. } => {
                        where_expr.as_deref().map_or_else(
                            || {
                                Ok(EvaluationStatus {
                                    fully_defined: solver.bool_value(true),
                                    first_partial: solver.bool_value(false),
                                    has_partial_operation: false,
                                })
                            },
                            |expression| {
                                evaluation_status(
                                    solver, model, expression, read_state, &local, None,
                                )
                            },
                        )?
                    }
                };
                first_partial = solver.or(&[
                    first_partial,
                    solver.and(&[active.clone(), where_status.first_partial])?,
                ])?;
                has_partial_operation |= where_status.has_partial_operation;
                let where_term = binder_where(solver, model, binder, read_state, &mut local, None)?
                    .unwrap_or_else(|| solver.bool_value(true));
                let body_reached = solver.and(&[
                    active.clone(),
                    where_status.fully_defined.clone(),
                    where_term,
                ])?;
                let body_status = statements_evaluation_status(
                    solver, model, statements, read_state, &mut local,
                )?;
                has_partial_operation |= body_status.has_partial_operation;
                first_partial = solver.or(&[
                    first_partial,
                    solver.and(&[body_reached.clone(), body_status.first_partial])?,
                ])?;
                let iteration_ok = solver.and(&[
                    where_status.fully_defined,
                    solver.implies(&body_reached, &body_status.fully_defined)?,
                ])?;
                fully_defined = solver.and(&[fully_defined, iteration_ok.clone()])?;
                active = solver.and(&[active, iteration_ok])?;
            }
            Ok(EvaluationStatus {
                fully_defined,
                first_partial,
                has_partial_operation,
            })
        }
    }
}

fn lvalue_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    target: &LValue,
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match target {
        LValue::Var(_) => Ok(EvaluationStatus {
            fully_defined: solver.bool_value(true),
            first_partial: solver.bool_value(false),
            has_partial_operation: false,
        }),
        LValue::Index(name, index) => {
            let index_status = evaluation_status(solver, model, index, read_state, bindings, None)?;
            let index_value = eval(solver, model, index, read_state, bindings, None)?;
            let root = read_state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            let fully_accessible = index_accessible(solver, model, root, &index_value)?;
            let partial_accessible =
                partial_operation_index_accessible(solver, root, &index_value)?;
            sequence_statuses(
                solver,
                [
                    index_status,
                    EvaluationStatus {
                        fully_defined: fully_accessible,
                        first_partial: solver.not(&partial_accessible)?,
                        has_partial_operation: matches!(root, SymbolicValue::Seq { .. }),
                    },
                ],
            )
        }
        LValue::Field(base, _) => {
            lvalue_evaluation_status(solver, model, base, read_state, bindings)
        }
    }
}

pub(crate) fn init_constraints<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<Vec<S::Term>, VerifyError> {
    let mut constraints = Vec::new();
    let mut bindings = Bindings::new();
    for statement in &model.init {
        collect_init_statement(
            solver,
            model,
            statement,
            state,
            &mut bindings,
            &mut constraints,
        )?;
    }
    Ok(constraints)
}

fn collect_init_statement<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    constraints: &mut Vec<S::Term>,
) -> Result<(), VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let target_ty = model
                .state_lvalue_type(target)
                .map_err(|error| VerifyError::new(error.message))?;
            let value = eval_expected(solver, model, value, &target_ty, state, bindings, None)?;
            let mut assigned = state.clone();
            assign(solver, model, target, value, state, &mut assigned, bindings)?;
            constraints.extend(state_equalities(solver, model, state, &assigned)?);
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition = eval(solver, model, condition, state, bindings, None)?;
            let condition = bool_term(&condition)?.clone();
            let mut then_constraints = Vec::new();
            for statement in then_statements {
                collect_init_statement(
                    solver,
                    model,
                    statement,
                    state,
                    &mut bindings.clone(),
                    &mut then_constraints,
                )?;
            }
            constraints.extend(
                then_constraints
                    .into_iter()
                    .map(|term| solver.implies(&condition, &term))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let not_condition = solver.not(&condition)?;
            let mut else_constraints = Vec::new();
            for statement in else_statements {
                collect_init_statement(
                    solver,
                    model,
                    statement,
                    state,
                    &mut bindings.clone(),
                    &mut else_constraints,
                )?;
            }
            constraints.extend(
                else_constraints
                    .into_iter()
                    .map(|term| solver.implies(&not_condition, &term))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            for (name, value) in binder_values(solver, model, binder)? {
                let mut local = bindings.clone();
                local.insert(name, value);
                let where_term = binder_where(solver, model, binder, state, &mut local, None)?;
                let mut body_constraints = Vec::new();
                for statement in statements {
                    collect_init_statement(
                        solver,
                        model,
                        statement,
                        state,
                        &mut local,
                        &mut body_constraints,
                    )?;
                }
                if let Some(where_term) = where_term {
                    constraints.extend(
                        body_constraints
                            .into_iter()
                            .map(|term| solver.implies(&where_term, &term))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                } else {
                    constraints.extend(body_constraints);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn transition_constraint<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    instances: &[ActionInstance<S::Term>],
    current: &SymbolicState<S::Term>,
    next: &SymbolicState<S::Term>,
    choice: &S::Term,
) -> Result<S::Term, VerifyError> {
    let mut clauses = Vec::new();
    for (instance_index, instance) in instances.iter().enumerate() {
        let action = &model.actions[instance.action_index];
        let (guards, mut bindings) =
            action_guards(solver, model, action, current, &instance.params)?;
        let pending = compute_updates(
            solver,
            model,
            &action.statements,
            current,
            current.clone(),
            &mut bindings,
        )?;
        let mut body = guards;
        body.extend(state_equalities(solver, model, next, &pending)?);
        let selected = solver.equal(choice, &solver.int_value(i64_index(instance_index)?))?;
        clauses.push(solver.implies(&selected, &solver.and(&body)?)?);
    }
    Ok(solver.and(&clauses)?)
}

fn compute_updates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    mut pending: SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    for statement in statements {
        pending = compute_statement(solver, model, statement, read_state, pending, bindings)?;
    }
    Ok(pending)
}

fn compute_statement<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    read_state: &SymbolicState<S::Term>,
    mut pending: SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let target_ty = model
                .state_lvalue_type(target)
                .map_err(|error| VerifyError::new(error.message))?;
            let assigned_value =
                eval_expected(solver, model, value, &target_ty, read_state, bindings, None)?;
            assign(
                solver,
                model,
                target,
                assigned_value,
                read_state,
                &mut pending,
                bindings,
            )?;
            Ok(pending)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition = eval(solver, model, condition, read_state, bindings, None)?;
            let then_state = compute_updates(
                solver,
                model,
                then_statements,
                read_state,
                pending.clone(),
                &mut bindings.clone(),
            )?;
            let else_state = compute_updates(
                solver,
                model,
                else_statements,
                read_state,
                pending,
                &mut bindings.clone(),
            )?;
            merge_states(
                solver,
                model,
                bool_term(&condition)?,
                &then_state,
                &else_state,
            )
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            for (name, value) in binder_values(solver, model, binder)? {
                let mut local = bindings.clone();
                local.insert(name, value);
                let where_term = binder_where(solver, model, binder, read_state, &mut local, None)?;
                let candidate = compute_updates(
                    solver,
                    model,
                    statements,
                    read_state,
                    pending.clone(),
                    &mut local,
                )?;
                pending = if let Some(where_term) = where_term {
                    merge_states(solver, model, &where_term, &candidate, &pending)?
                } else {
                    candidate
                };
            }
            Ok(pending)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn assign<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    target: &LValue,
    value: SymbolicValue<S::Term>,
    read_state: &SymbolicState<S::Term>,
    target_state: &mut SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<(), VerifyError> {
    match target {
        LValue::Var(name) => {
            let ty = model
                .state_type(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            target_state.insert(name.clone(), coerce(solver, model, value, ty)?);
        }
        LValue::Index(name, index_expr) => {
            let index = eval(solver, model, index_expr, read_state, bindings, None)?;
            let root = target_state
                .get(name)
                .cloned()
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            target_state.insert(
                name.clone(),
                assign_index(solver, model, root, &index, value)?,
            );
        }
        LValue::Field(base, field) => match base.as_ref() {
            LValue::Var(name) => {
                let root = target_state
                    .get_mut(name)
                    .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
                assign_field(solver, model, root, field, value)?;
            }
            LValue::Index(name, index_expr) => {
                let index = eval(solver, model, index_expr, read_state, bindings, None)?;
                let root = target_state
                    .get(name)
                    .cloned()
                    .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
                let SymbolicValue::Map { ty, entries } = root else {
                    return Err(VerifyError::new("map field assignment requires a map"));
                };
                let TypeRef::Map(key_ty, value_ty) = &ty else {
                    unreachable!();
                };
                let key_ty = key_ty.as_ref().clone();
                let value_ty = value_ty.as_ref().clone();
                let mut selected = select_finite(solver, model, &entries, &index, &key_ty)?;
                assign_field(solver, model, &mut selected, field, value)?;
                let selected = coerce(solver, model, selected, &value_ty)?;
                target_state.insert(
                    name.clone(),
                    SymbolicValue::Map {
                        ty,
                        entries: store_finite(solver, model, &entries, &index, &key_ty, &selected)?,
                    },
                );
            }
            LValue::Field(_, _) => {
                return Err(VerifyError::new("nested field assignment is unsupported"));
            }
        },
    }
    Ok(())
}

fn assign_index<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    root: SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
    value: SymbolicValue<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match root {
        SymbolicValue::Map { ty, entries } => {
            let TypeRef::Map(key_ty, value_ty) = &ty else {
                unreachable!();
            };
            let key_ty = key_ty.as_ref().clone();
            let value_ty = value_ty.as_ref().clone();
            let value = coerce(solver, model, value, &value_ty)?;
            Ok(SymbolicValue::Map {
                ty,
                entries: store_finite(solver, model, &entries, index, &key_ty, &value)?,
            })
        }
        SymbolicValue::Seq { ty, slots, len } => {
            let TypeRef::Seq(element_ty, _) = &ty else {
                unreachable!();
            };
            let value = coerce(solver, model, value, element_ty)?;
            let entries = slots
                .iter()
                .enumerate()
                .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
                .collect::<Result<Vec<_>, VerifyError>>()?;
            Ok(SymbolicValue::Seq {
                ty,
                slots: store_finite(solver, model, &entries, index, &TypeRef::Int, &value)?
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect(),
                len,
            })
        }
        _ => Err(VerifyError::new(
            "indexed assignment requires a map or sequence",
        )),
    }
}

fn assign_field<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    root: &mut SymbolicValue<S::Term>,
    field: &str,
    value: SymbolicValue<S::Term>,
) -> Result<(), VerifyError> {
    let SymbolicValue::Struct { ty, fields } = root else {
        return Err(VerifyError::new("field assignment requires a struct"));
    };
    let TypeRef::Named(name) = ty else {
        unreachable!();
    };
    let field_ty = model
        .struct_fields(name)
        .and_then(|expected| {
            expected
                .iter()
                .find_map(|(name, ty)| (name == field).then_some(ty))
        })
        .ok_or_else(|| VerifyError::new(format!("unknown struct field '{field}'")))?;
    fields.insert(field.to_owned(), coerce(solver, model, value, field_ty)?);
    Ok(())
}

pub(crate) fn state_equalities<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    left: &SymbolicState<S::Term>,
    right: &SymbolicState<S::Term>,
) -> Result<Vec<S::Term>, VerifyError> {
    model
        .state
        .iter()
        .map(|(name, _)| {
            logical_equal(
                solver,
                model,
                left.get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
                right
                    .get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
            )
        })
        .collect()
}

fn merge_states<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    condition: &S::Term,
    then_state: &SymbolicState<S::Term>,
    else_state: &SymbolicState<S::Term>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    model
        .state
        .iter()
        .map(|(name, _)| {
            Ok((
                name.clone(),
                ite_value(
                    solver,
                    model,
                    condition,
                    &then_state[name],
                    &else_state[name],
                )?,
            ))
        })
        .collect()
}
