// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    ActionDef, ActionGuard, FslValue, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, ParamDef, TypeRef,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::eval::{binder_values, binder_where, definedness, eval, index_accessible};
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
}

pub(crate) struct ActionGuardDefinedness<T> {
    pub enabled: T,
    pub defined: T,
    pub bindings: Bindings<T>,
}

pub(crate) fn action_instances<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
) -> Result<Vec<ActionInstance<S::Term>>, VerifyError> {
    let mut instances = Vec::new();
    for (action_index, action) in model.actions.iter().enumerate() {
        let mut bindings = vec![Bindings::new()];
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
            for existing in bindings {
                for value in &values {
                    let mut candidate = existing.clone();
                    candidate.insert(
                        param.name().to_owned(),
                        crate::value::concrete_value(solver, model, &ty, value)?,
                    );
                    next.push(candidate);
                }
            }
            bindings = next;
        }
        instances.extend(bindings.into_iter().map(|params| ActionInstance {
            action_index,
            action: action.name.clone(),
            params,
        }));
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
    let mut guard_terms = Vec::new();
    for guard in &action.guards {
        let expression = match guard {
            ActionGuard::Let(_, expression) | ActionGuard::Requires(expression) => expression,
        };
        let expression_defined = definedness(solver, model, expression, state, &bindings, None)?;
        guard_terms.push(solver.implies(&reaches_guard, &expression_defined)?);
        let value = eval(solver, model, expression, state, &mut bindings, None)?;
        match guard {
            ActionGuard::Let(name, _) => {
                reaches_guard = solver.and(&[reaches_guard, expression_defined])?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(_) => {
                reaches_guard = solver.and(&[
                    reaches_guard,
                    expression_defined,
                    bool_term(&value)?.clone(),
                ])?;
            }
        }
    }
    Ok(ActionGuardDefinedness {
        enabled: reaches_guard,
        defined: solver.and(&guard_terms)?,
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
    statements_definedness(
        solver,
        model,
        &action.statements,
        state,
        &mut bindings.clone(),
    )
}

fn statements_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    let terms = statements
        .iter()
        .map(|statement| statement_definedness(solver, model, statement, read_state, bindings))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(solver.and(&terms)?)
}

fn statement_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value_defined = definedness(solver, model, value, read_state, bindings, None)?;
            let _ = eval(solver, model, value, read_state, bindings, None)?;
            let target_defined = lvalue_definedness(solver, model, target, read_state, bindings)?;
            Ok(solver.and(&[value_defined, target_defined])?)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition_defined =
                definedness(solver, model, condition, read_state, bindings, None)?;
            let condition = eval(solver, model, condition, read_state, bindings, None)?;
            let then_defined = statements_definedness(
                solver,
                model,
                then_statements,
                read_state,
                &mut bindings.clone(),
            )?;
            let else_defined = statements_definedness(
                solver,
                model,
                else_statements,
                read_state,
                &mut bindings.clone(),
            )?;
            Ok(solver.and(&[
                condition_defined,
                solver.ite(bool_term(&condition)?, &then_defined, &else_defined)?,
            ])?)
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            let mut terms = Vec::new();
            for (name, value) in binder_values(solver, model, binder)? {
                let mut local = bindings.clone();
                local.insert(name, value);
                let where_defined = match binder {
                    fsl_core::KernelBinder::Typed { where_expr, .. }
                    | fsl_core::KernelBinder::Range { where_expr, .. }
                    | fsl_core::KernelBinder::Collection { where_expr, .. } => {
                        where_expr.as_deref().map_or_else(
                            || Ok(solver.bool_value(true)),
                            |expression| {
                                definedness(solver, model, expression, read_state, &local, None)
                            },
                        )?
                    }
                };
                let where_term = binder_where(solver, model, binder, read_state, &mut local, None)?
                    .unwrap_or_else(|| solver.bool_value(true));
                let body_defined =
                    statements_definedness(solver, model, statements, read_state, &mut local)?;
                terms.push(
                    solver.and(&[where_defined, solver.implies(&where_term, &body_defined)?])?,
                );
            }
            Ok(solver.and(&terms)?)
        }
    }
}

fn lvalue_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    target: &LValue,
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    match target {
        LValue::Var(_) => Ok(solver.bool_value(true)),
        LValue::Index(name, index) => {
            let index_defined = definedness(solver, model, index, read_state, bindings, None)?;
            let index_value = eval(solver, model, index, read_state, bindings, None)?;
            let root = read_state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            Ok(solver.and(&[
                index_defined,
                index_accessible(solver, model, root, &index_value)?,
            ])?)
        }
        LValue::Field(base, _) => lvalue_definedness(solver, model, base, read_state, bindings),
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
            let value = eval(solver, model, value, state, bindings, None)?;
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
            let value = eval(solver, model, value, read_state, bindings, None)?;
            assign(
                solver,
                model,
                target,
                value,
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
