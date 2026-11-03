// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::binder_range::{InclusiveI64, IntegerEnvelopeFact, RangeBinding};
use fsl_core::{
    ActionDef, ActionGuard, FslValue, KernelExpr as Expr, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, ParamDef, Span, TypeDef, TypeRef,
};
use fsl_solver::{ModelValue, SmtSolver};

use crate::VerifyError;
use crate::bmc::CheckedStateCarriers;
use crate::eval::{
    EvaluationStatus, FailureClass, binder_condition, binder_source_expressions, binder_values,
    binder_values_in, binder_where, binder_where_expression, binder_where_in,
    classified_evaluation_status_in, enumerated_binder_binding, eval, eval_binding_in,
    eval_expected, eval_expected_in, eval_in, eval_with_bindings_in, index_access_status,
    index_accessible, safe_status, sequence_statuses,
};
use crate::value::TypedBindings as Bindings;
use crate::value::{
    BindingRecord, RangeFact, SymbolicState, SymbolicValue, TypedBinding, bool_term, coerce,
    collection_contains, collection_entries, collection_types, concrete_value, i64_index,
    ite_value, literal_key, logical_equal, map_collections, select_finite, solver_sort,
    store_finite, symbolic_state_with_suffix,
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

pub(crate) use fsl_core::action_has_partial_operation_candidate;

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
                    let binding = TypedBinding(
                        crate::value::concrete_value(solver, model, &ty, value)?,
                        Some(ty.clone()),
                    );
                    let binding = match value {
                        FslValue::Int(value) => {
                            BindingRecord::with_range_fact(binding, RangeFact::parameter(*value))
                        }
                        _ => binding.into(),
                    };
                    terms.insert(param.name().to_owned(), binding);
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
    action_guards_in(solver, model, action, state, params, None)
}

pub(crate) fn action_guards_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    params: &Bindings<S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<GuardEvaluation<S::Term>, VerifyError> {
    let mut bindings = params.clone();
    let mut guards = Vec::new();
    for guard in &action.guards {
        match guard {
            ActionGuard::Let(name, expr) => {
                let value = eval_binding_in(solver, model, expr, state, &mut bindings, None, ctx)?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(expr) => {
                let value =
                    eval_with_bindings_in(solver, model, expr, state, &mut bindings, None, ctx)?;
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
    classified_action_guard_definedness(
        solver,
        model,
        action,
        state,
        params,
        FailureClass::PartialOperation,
    )
}

/// [`action_guard_definedness`] with `first_partial` classifying `class`
/// (#1221: [`FailureClass::KeyDomain`] for the induction step obligation).
pub(crate) fn classified_action_guard_definedness<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    params: &Bindings<S::Term>,
    class: FailureClass,
) -> Result<ActionGuardDefinedness<S::Term>, VerifyError> {
    classified_action_guard_definedness_in(solver, model, action, state, params, class, None)
}

pub(crate) fn classified_action_guard_definedness_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    params: &Bindings<S::Term>,
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
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
        let status = classified_evaluation_status_in(
            solver, model, expression, state, &bindings, None, class, ctx,
        )?;
        has_partial_operation |= status.has_partial_operation;
        first_partial = solver.or(&[
            first_partial,
            solver.and(&[reaches_guard.clone(), status.first_partial])?,
        ])?;
        fully_defined = solver.and(&[
            fully_defined,
            solver.implies(&reaches_guard, &status.fully_defined)?,
        ])?;
        match guard {
            ActionGuard::Let(name, _) => {
                let value = eval_binding_in(
                    solver,
                    model,
                    expression,
                    state,
                    &mut bindings,
                    Some(&status.fully_defined),
                    ctx,
                )?;
                reaches_guard = solver.and(&[reaches_guard, status.fully_defined])?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(_) => {
                let value = eval_with_bindings_in(
                    solver,
                    model,
                    expression,
                    state,
                    &mut bindings,
                    None,
                    ctx,
                )?;
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
    classified_action_statements_evaluation_status(
        solver,
        model,
        action,
        state,
        bindings,
        FailureClass::PartialOperation,
    )
}

/// [`action_statements_evaluation_status`] with `first_partial` classifying
/// `class` (#1221).
pub(crate) fn classified_action_statements_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    class: FailureClass,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    classified_action_statements_evaluation_status_in(
        solver, model, action, state, bindings, class, None,
    )
}

pub(crate) fn classified_action_statements_evaluation_status_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    action: &ActionDef,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    statements_evaluation_status(
        solver,
        model,
        &action.statements,
        state,
        &mut bindings.clone(),
        class,
        ctx,
    )
}

fn statements_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let statuses = statements
        .iter()
        .map(|statement| {
            statement_evaluation_status(solver, model, statement, read_state, bindings, class, ctx)
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
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value_status = classified_evaluation_status_in(
                solver, model, value, read_state, bindings, None, class, ctx,
            )?;
            let target_ty = model
                .state_lvalue_type(target)
                .map_err(|error| VerifyError::new(error.message))?;
            let _ = eval_expected_in(
                solver, model, value, &target_ty, read_state, bindings, None, ctx,
            )?;
            let target_status =
                lvalue_evaluation_status(solver, model, target, read_state, bindings, class, ctx)?;
            sequence_statuses(solver, [value_status, target_status])
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition_status = classified_evaluation_status_in(
                solver, model, condition, read_state, bindings, None, class, ctx,
            )?;
            let condition = eval_in(solver, model, condition, read_state, bindings, None, ctx)?;
            let then_status = statements_evaluation_status(
                solver,
                model,
                then_statements,
                read_state,
                &mut bindings.clone(),
                class,
                ctx,
            )?;
            let else_status = statements_evaluation_status(
                solver,
                model,
                else_statements,
                read_state,
                &mut bindings.clone(),
                class,
                ctx,
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
            let source_status = sequence_statuses(
                solver,
                binder_source_expressions(binder)
                    .into_iter()
                    .map(|expr| {
                        classified_evaluation_status_in(
                            solver, model, expr, read_state, bindings, None, class, ctx,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?;
            let mut active = source_status.fully_defined.clone();
            let mut fully_defined = source_status.fully_defined;
            let mut first_partial = source_status.first_partial;
            let mut has_partial_operation = source_status.has_partial_operation;
            for (name, value, membership) in
                binder_values_in(solver, model, binder, read_state, bindings, None, ctx)?
            {
                let mut local = bindings.clone();
                local.insert(name, enumerated_binder_binding(solver, model, value));
                let where_reached = solver.and(&[
                    active.clone(),
                    membership.unwrap_or_else(|| solver.bool_value(true)),
                ])?;
                let where_status = binder_where_expression(binder).map_or_else(
                    || Ok(safe_status(solver)),
                    |expression| {
                        classified_evaluation_status_in(
                            solver, model, expression, read_state, &local, None, class, ctx,
                        )
                    },
                )?;
                first_partial = solver.or(&[
                    first_partial,
                    solver.and(&[where_reached.clone(), where_status.first_partial])?,
                ])?;
                has_partial_operation |= where_status.has_partial_operation;
                let where_term =
                    binder_where_in(solver, model, binder, read_state, &mut local, None, ctx)?
                        .unwrap_or_else(|| solver.bool_value(true));
                let body_reached = solver.and(&[
                    where_reached.clone(),
                    where_status.fully_defined.clone(),
                    where_term,
                ])?;
                let body_status = statements_evaluation_status(
                    solver, model, statements, read_state, &mut local, class, ctx,
                )?;
                has_partial_operation |= body_status.has_partial_operation;
                first_partial = solver.or(&[
                    first_partial,
                    solver.and(&[body_reached.clone(), body_status.first_partial])?,
                ])?;
                let iteration_ok = solver.and(&[
                    solver.implies(&where_reached, &where_status.fully_defined)?,
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
    class: FailureClass,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match target {
        LValue::Var(_) => Ok(EvaluationStatus {
            fully_defined: solver.bool_value(true),
            first_partial: solver.bool_value(false),
            has_partial_operation: false,
        }),
        LValue::Index(name, index) => {
            let index_status = classified_evaluation_status_in(
                solver, model, index, read_state, bindings, None, class, ctx,
            )?;
            let index_value = eval_in(solver, model, index, read_state, bindings, None, ctx)?;
            let root = read_state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            let fully_accessible = index_accessible(solver, model, root, &index_value)?;
            let access = index_access_status(solver, root, &index_value, fully_accessible, class)?;
            sequence_statuses(solver, [index_status, access])
        }
        LValue::Field(base, _) => {
            lvalue_evaluation_status(solver, model, base, read_state, bindings, class, ctx)
        }
    }
}

/// Whether evaluating `init` against `state` is defined (#1258): the same
/// action-context `evaluation_status` the action checks use, over the init
/// statements, reading `state` exactly as [`init_constraints`] does.
pub(crate) fn init_evaluation_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    let mut result = safe_status(solver);
    visit_init_reads(
        solver,
        model,
        state,
        &mut |read, state, bindings, reached| {
            let status = init_read_status(solver, model, read, state, bindings)?;
            result.fully_defined = solver.and(&[
                result.fully_defined.clone(),
                solver.implies(reached, &status.fully_defined)?,
            ])?;
            result.first_partial = solver.or(&[
                result.first_partial.clone(),
                solver.and(&[reached.clone(), status.first_partial])?,
            ])?;
            result.has_partial_operation |= status.has_partial_operation;
            Ok(status.fully_defined)
        },
    )?;
    Ok(result)
}

/// Init value reads see completed assignments; statement binder sources do not.
#[derive(Clone, Copy)]
pub(crate) enum InitRead<'a> {
    Expression(&'a Expr, Span),
    Target(&'a LValue, Span),
}

type InitObserver<'a, T> =
    dyn FnMut(InitRead<'_>, &SymbolicState<T>, &Bindings<T>, &T) -> Result<T, VerifyError> + 'a;

pub(crate) fn init_read_status<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    read: InitRead<'_>,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<EvaluationStatus<S::Term>, VerifyError> {
    match read {
        InitRead::Expression(expr, _) => classified_evaluation_status_in(
            solver,
            model,
            expr,
            state,
            bindings,
            None,
            FailureClass::PartialOperation,
            None,
        ),
        InitRead::Target(target, _) => lvalue_evaluation_status(
            solver,
            model,
            target,
            state,
            &mut bindings.clone(),
            FailureClass::PartialOperation,
            None,
        ),
    }
}

pub(crate) fn visit_init_reads<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
    observer: &mut InitObserver<'_, S::Term>,
) -> Result<(), VerifyError> {
    let mut state = state.clone();
    let mut recipes = Bindings::new();
    let mut reached = Some(solver.bool_value(true));
    let mut observer = Some(observer);
    for statement in &model.init {
        collect_init_statement(
            solver,
            model,
            statement,
            &mut state,
            &mut Bindings::new(),
            &mut Vec::new(),
            &mut |_, _| Ok(()),
            &mut recipes,
            &mut reached,
            &mut observer,
        )?;
    }
    Ok(())
}

fn init_expression_bindings<T: Clone>(recipes: &Bindings<T>, lexical: &Bindings<T>) -> Bindings<T> {
    let mut bindings = recipes.clone();
    for (name, record) in lexical.records() {
        bindings.insert(name.clone(), record.clone());
    }
    bindings
}

/// Select only a literal or an unshadowed model-constant condition; never query SMT.
fn init_known_condition<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    condition: &Expr,
    bindings: &Bindings<S::Term>,
    value: Option<&SymbolicValue<S::Term>>,
) -> Option<bool> {
    if let Expr::Bool(value) = condition {
        return Some(*value);
    }
    let value = value.or_else(|| match condition {
        Expr::Var(name) => bindings.get(name),
        _ => None,
    });
    if let Some(SymbolicValue::Scalar { term, .. }) = value
        && let Some(ModelValue::Bool(value)) = solver.literal_value(term)
    {
        return Some(value);
    }
    if bindings.keys().any(|name| model.consts.contains_key(name)) {
        return None;
    }
    fsl_core::binder_range::constant_condition(model, condition)
}

/// Derive from the actual RHS and unconditional lexical facts, never a state carrier.
fn init_expression_envelope<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    bindings: &Bindings<S::Term>,
) -> Option<InclusiveI64> {
    let lexical: Option<Vec<_>> = bindings
        .records()
        .map(|(name, record)| {
            Some(RangeBinding {
                name,
                semantic_ty: record.binding.1.as_ref()?,
                fact: record
                    .range_fact
                    .as_ref()
                    .filter(|fact| fact.valid_when().is_none())
                    .map(RangeFact::envelope),
            })
        })
        .collect();
    fsl_core::binder_range::init_expression_envelope(
        model,
        expr,
        lexical.as_deref(),
        &mut |condition| init_known_condition(solver, model, condition, bindings, None),
    )
}

fn init_recipe<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    bindings: &Bindings<S::Term>,
    value: SymbolicValue<S::Term>,
    ty: &TypeRef,
) -> Option<BindingRecord<S::Term>> {
    let mut record =
        enumerated_binder_binding(solver, model, TypedBinding(value, Some(ty.clone())));
    if record.range_fact.is_none() {
        record.range_fact = init_expression_envelope(solver, model, expr, bindings)
            .map(|envelope| RangeFact::ParameterValue(IntegerEnvelopeFact::new(envelope)));
    }
    let integer = matches!(ty, TypeRef::Int | TypeRef::Range(_, _))
        || matches!(ty, TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Domain { .. })));
    if integer
        && !matches!(ty, TypeRef::Int)
        && record.range_fact.as_ref().is_some_and(|fact| {
            !fsl_core::binder_range::init_envelope_fits(model, ty, fact.envelope().envelope())
        })
    {
        record.range_fact = None;
    }
    (!integer || record.range_fact.is_some()).then_some(record)
}

fn merge_init_recipes<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    condition: &S::Term,
    then_recipes: &Bindings<S::Term>,
    else_recipes: &Bindings<S::Term>,
) -> Result<Bindings<S::Term>, VerifyError> {
    match solver.literal_value(condition) {
        Some(ModelValue::Bool(true)) => return Ok(then_recipes.clone()),
        Some(ModelValue::Bool(false)) => return Ok(else_recipes.clone()),
        _ => {}
    }
    let mut joined = Bindings::new();
    for (name, left) in then_recipes.records() {
        let Some((_, right)) = else_recipes.records().find(|(other, _)| *other == name) else {
            continue;
        };
        let mut record = BindingRecord::from(TypedBinding(
            ite_value(solver, model, condition, &left.binding.0, &right.binding.0)?,
            left.binding.1.clone(),
        ));
        if let (Some(left), Some(right)) = (&left.range_fact, &right.range_fact)
            && left.valid_when().is_none()
            && right.valid_when().is_none()
        {
            let left = left.envelope().envelope();
            let right = right.envelope().envelope();
            record.range_fact =
                InclusiveI64::new(left.lo().min(right.lo()), left.hi().max(right.hi()))
                    .map(|envelope| RangeFact::ParameterValue(IntegerEnvelopeFact::new(envelope)));
        }
        joined.insert(name.clone(), record);
    }
    Ok(joined)
}

fn observe_init_read<S: SmtSolver>(
    solver: &S,
    read: InitRead<'_>,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &mut Option<S::Term>,
    observer: &mut Option<&mut InitObserver<'_, S::Term>>,
) -> Result<(), VerifyError> {
    if let Some(observer) = observer {
        let active = reached
            .as_ref()
            .expect("init observer has a reached condition");
        let defined = observer(read, state, bindings, active)?;
        *reached = Some(solver.and(&[active.clone(), defined])?);
    }
    Ok(())
}

pub(crate) fn init_constraints<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<Vec<S::Term>, VerifyError> {
    let mut constraints = Vec::new();
    let mut bindings = Bindings::new();
    let mut state = state.clone();
    let mut recipes = Bindings::new();
    for statement in &model.init {
        collect_init_statement(
            solver,
            model,
            statement,
            &mut state,
            &mut bindings,
            &mut constraints,
            &mut |_, _| Ok(()),
            &mut recipes,
            &mut None,
            &mut None,
        )?;
    }
    Ok(constraints)
}

/// Reserve open collection slots, then apply the ordinary relational init
/// constraints. Repeated assignments still have to agree; this is not an
/// action-style last-writer initializer. Only fresh shape is propagated in
/// checked dependency order; scalar values and membership remain unconstrained.
pub(crate) fn initial_state<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    suffix: &str,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    let mut state = symbolic_state_with_suffix(solver, model, suffix)?;
    let mut index = 0;
    let mut recipes = Bindings::new();
    for statement in &model.init {
        collect_init_statement(
            solver,
            model,
            statement,
            &mut state,
            &mut Bindings::new(),
            &mut Vec::new(),
            &mut |state, assigned| {
                for (name, value) in assigned {
                    extend_initial_collections(
                        solver,
                        model,
                        suffix,
                        &mut index,
                        state.get_mut(name).unwrap(),
                        value,
                    )?;
                }
                Ok(())
            },
            &mut recipes,
            &mut None,
            &mut None,
        )?;
    }
    Ok(state)
}

fn extend_initial_collections<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    suffix: &str,
    index: &mut usize,
    state: &mut SymbolicValue<S::Term>,
    assigned: &SymbolicValue<S::Term>,
) -> Result<(), VerifyError> {
    map_collections(state, assigned, &mut |state, assigned| {
        let SymbolicValue::Collection {
            entries: candidates,
            ..
        } = assigned
        else {
            return Ok(());
        };
        let (ty, mut entries) = collection_entries(solver, model, state)?;
        let (_, grid) = collection_entries(
            solver,
            model,
            &concrete_value(solver, model, &ty, &model.default_value(&ty)?)?,
        )?;
        let extra = candidates
            .iter()
            .filter(|(candidate, _)| {
                literal_key(solver, candidate).is_none_or(|key| {
                    !grid
                        .iter()
                        .any(|(candidate, _)| literal_key(solver, candidate).as_ref() == Some(&key))
                })
            })
            .count();
        for _ in entries.len()..grid.len() + extra {
            let name = format!("__init_collection_{suffix}_{}", *index);
            *index += 1;
            let key = collection_types(&ty)?
                .into_iter()
                .enumerate()
                .map(|(part, ty)| {
                    Ok(SymbolicValue::Scalar {
                        ty: ty.clone(),
                        term: solver
                            .constant(&format!("{name}_{part}"), &solver_sort(model, ty)?)?,
                    })
                })
                .collect::<Result<Vec<_>, VerifyError>>()?;
            let present = solver.and(&[
                solver.constant(&name, &fsl_solver::Sort::Bool)?,
                solver.not(&collection_contains(solver, model, &entries, &key)?)?,
            ])?;
            entries.push((key, present));
        }
        *state = SymbolicValue::Collection { ty, entries };
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_init_statement<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &mut SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    constraints: &mut Vec<S::Term>,
    on_assignment: &mut impl FnMut(
        &mut SymbolicState<S::Term>,
        &SymbolicState<S::Term>,
    ) -> Result<(), VerifyError>,
    recipes: &mut Bindings<S::Term>,
    reached: &mut Option<S::Term>,
    observer: &mut Option<&mut InitObserver<'_, S::Term>>,
) -> Result<(), VerifyError> {
    match statement {
        Statement::Assign { .. } => collect_init_assignment(
            solver,
            model,
            statement,
            state,
            bindings,
            constraints,
            on_assignment,
            recipes,
            reached,
            observer,
        ),
        Statement::If { .. } => collect_init_branches(
            solver,
            model,
            statement,
            state,
            bindings,
            constraints,
            on_assignment,
            recipes,
            reached,
            observer,
        ),
        Statement::ForAll { .. } => collect_init_forall(
            solver,
            model,
            statement,
            state,
            bindings,
            constraints,
            on_assignment,
            recipes,
            reached,
            observer,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_init_assignment<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &mut SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    constraints: &mut Vec<S::Term>,
    on_assignment: &mut impl FnMut(
        &mut SymbolicState<S::Term>,
        &SymbolicState<S::Term>,
    ) -> Result<(), VerifyError>,
    recipes: &mut Bindings<S::Term>,
    reached: &mut Option<S::Term>,
    observer: &mut Option<&mut InitObserver<'_, S::Term>>,
) -> Result<(), VerifyError> {
    let Statement::Assign {
        target,
        value,
        span,
    } = statement
    else {
        unreachable!("init assign collector receives its matched statement");
    };
    let target_ty = model
        .state_lvalue_type(target)
        .map_err(|error| VerifyError::new(error.message))?;
    let mut read = init_expression_bindings(recipes, bindings);
    observe_init_read(
        solver,
        InitRead::Expression(value, *span),
        state,
        &read,
        reached,
        observer,
    )?;
    let assigned_value = eval_expected(solver, model, value, &target_ty, state, &mut read, None)?;
    observe_init_read(
        solver,
        InitRead::Target(target, *span),
        state,
        &read,
        reached,
        observer,
    )?;
    let mut assigned = state.clone();
    assign(
        solver,
        model,
        target,
        assigned_value,
        state,
        &mut assigned,
        &mut read,
        None,
    )?;
    on_assignment(state, &assigned)?;
    constraints.extend(state_equalities(solver, model, state, &assigned)?);
    if let LValue::Var(name) = target {
        let recipe = init_recipe(
            solver,
            model,
            value,
            &read,
            assigned[name].clone(),
            &target_ty,
        );
        let mut updated = Bindings::new();
        for (other, record) in recipes.records() {
            if other != name {
                updated.insert(other.clone(), record.clone());
            }
        }
        if let Some(recipe) = recipe {
            updated.insert(name.clone(), recipe);
        }
        *recipes = updated;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_init_branches<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &mut SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    constraints: &mut Vec<S::Term>,
    on_assignment: &mut impl FnMut(
        &mut SymbolicState<S::Term>,
        &SymbolicState<S::Term>,
    ) -> Result<(), VerifyError>,
    recipes: &mut Bindings<S::Term>,
    reached: &mut Option<S::Term>,
    observer: &mut Option<&mut InitObserver<'_, S::Term>>,
) -> Result<(), VerifyError> {
    let Statement::If {
        condition,
        then_statements,
        else_statements,
        span,
    } = statement
    else {
        unreachable!("init if collector receives its matched statement");
    };
    let mut read = init_expression_bindings(recipes, bindings);
    observe_init_read(
        solver,
        InitRead::Expression(condition, *span),
        state,
        &read,
        reached,
        observer,
    )?;
    let value = eval(solver, model, condition, state, &mut read, None)?;
    let selected = init_known_condition(solver, model, condition, &read, Some(&value));
    let condition = bool_term(&value)?.clone();
    let mut then_constraints = Vec::new();
    let mut then_recipes = recipes.clone();
    let mut then_reached = reached
        .as_ref()
        .map(|active| solver.and(&[active.clone(), condition.clone()]))
        .transpose()?;
    for statement in then_statements {
        collect_init_statement(
            solver,
            model,
            statement,
            state,
            &mut bindings.clone(),
            &mut then_constraints,
            on_assignment,
            &mut then_recipes,
            &mut then_reached,
            observer,
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
    let mut else_recipes = recipes.clone();
    let mut else_reached = reached
        .as_ref()
        .map(|active| solver.and(&[active.clone(), not_condition.clone()]))
        .transpose()?;
    for statement in else_statements {
        collect_init_statement(
            solver,
            model,
            statement,
            state,
            &mut bindings.clone(),
            &mut else_constraints,
            on_assignment,
            &mut else_recipes,
            &mut else_reached,
            observer,
        )?;
    }
    constraints.extend(
        else_constraints
            .into_iter()
            .map(|term| solver.implies(&not_condition, &term))
            .collect::<Result<Vec<_>, _>>()?,
    );
    *recipes = match selected {
        Some(true) => then_recipes,
        Some(false) => else_recipes,
        None => merge_init_recipes(solver, model, &condition, &then_recipes, &else_recipes)?,
    };
    if let (Some(left), Some(right)) = (then_reached, else_reached) {
        *reached = Some(solver.or(&[left, right])?);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_init_forall<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &mut SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    constraints: &mut Vec<S::Term>,
    on_assignment: &mut impl FnMut(
        &mut SymbolicState<S::Term>,
        &SymbolicState<S::Term>,
    ) -> Result<(), VerifyError>,
    recipes: &mut Bindings<S::Term>,
    reached: &mut Option<S::Term>,
    observer: &mut Option<&mut InitObserver<'_, S::Term>>,
) -> Result<(), VerifyError> {
    let Statement::ForAll {
        binder,
        statements,
        span,
    } = statement
    else {
        unreachable!("init forall collector receives its matched statement");
    };
    let bound_names = bindings.keys().map(String::as_str);
    fsl_core::check_init_collection_binder(model, binder, bound_names)?;
    for expression in binder_source_expressions(binder) {
        observe_init_read(
            solver,
            InitRead::Expression(expression, *span),
            state,
            bindings,
            reached,
            observer,
        )?;
    }
    for (name, value, membership) in binder_values(solver, model, binder, state, bindings, None)? {
        let mut local = bindings.clone();
        local.insert(name, enumerated_binder_binding(solver, model, value));
        let membership_term = membership
            .clone()
            .unwrap_or_else(|| solver.bool_value(true));
        let mut where_reached = reached
            .as_ref()
            .map(|active| solver.and(&[active.clone(), membership_term.clone()]))
            .transpose()?;
        let mut read = init_expression_bindings(recipes, &local);
        if let Some(expression) = binder_where_expression(binder) {
            observe_init_read(
                solver,
                InitRead::Expression(expression, *span),
                state,
                &read,
                &mut where_reached,
                observer,
            )?;
        }
        let where_term = binder_where(solver, model, binder, state, &mut read, None)?;
        let unconditional = membership.is_none() && where_term.is_none();
        let filter = where_term
            .clone()
            .unwrap_or_else(|| solver.bool_value(true));
        let condition = binder_condition(solver, membership, where_term)?;
        let mut body_reached = where_reached
            .as_ref()
            .map(|active| solver.and(&[active.clone(), filter.clone()]))
            .transpose()?;
        let mut body_constraints = Vec::new();
        let mut body_recipes = recipes.clone();
        for statement in statements {
            collect_init_statement(
                solver,
                model,
                statement,
                state,
                &mut local,
                &mut body_constraints,
                on_assignment,
                &mut body_recipes,
                &mut body_reached,
                observer,
            )?;
        }
        constraints.extend(
            body_constraints
                .into_iter()
                .map(|term| solver.implies(&condition, &term))
                .collect::<Result<Vec<_>, _>>()?,
        );
        *recipes = if unconditional {
            body_recipes
        } else {
            merge_init_recipes(solver, model, &condition, &body_recipes, recipes)?
        };
        if let (Some(active), Some(where_active), Some(body)) =
            (reached.as_ref(), where_reached, body_reached)
        {
            let iteration = solver.or(&[
                solver.not(&membership_term)?,
                solver.and(&[where_active, solver.or(&[solver.not(&filter)?, body])?])?,
            ])?;
            *reached = Some(solver.and(&[active.clone(), iteration])?);
        }
    }
    Ok(())
}

pub(crate) fn transition_constraint<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    instances: &[ActionInstance<S::Term>],
    current: &SymbolicState<S::Term>,
    next: &mut SymbolicState<S::Term>,
    choice: &S::Term,
) -> Result<S::Term, VerifyError> {
    transition_constraint_in(solver, model, instances, current, next, choice, None)
}

pub(crate) fn transition_constraint_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    instances: &[ActionInstance<S::Term>],
    current: &SymbolicState<S::Term>,
    next: &mut SymbolicState<S::Term>,
    choice: &S::Term,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<S::Term, VerifyError> {
    let mut candidates = Vec::new();
    for (instance_index, instance) in instances.iter().enumerate() {
        let action = &model.actions[instance.action_index];
        let (guards, mut bindings) =
            action_guards_in(solver, model, action, current, &instance.params, ctx)?;
        let pending = compute_updates_in(
            solver,
            model,
            &action.statements,
            current,
            current.clone(),
            &mut bindings,
            ctx,
        )?;
        let selected = solver.equal(choice, &solver.int_value(i64_index(instance_index)?))?;
        for (name, value) in &pending {
            retain_collections(solver, model, &selected, next.get_mut(name).unwrap(), value)?;
        }
        candidates.push((selected, guards, pending));
    }
    let mut clauses = Vec::new();
    for (selected, mut body, pending) in candidates {
        body.extend(state_equalities(solver, model, next, &pending)?);
        clauses.push(solver.implies(&selected, &solver.and(&body)?)?);
    }
    Ok(solver.and(&clauses)?)
}

/// Fresh finite grids cannot represent an escaped member. Carry complete
/// collection expressions into the successor, leaving other fresh fields alone.
fn retain_collections<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    selected: &S::Term,
    next: &mut SymbolicValue<S::Term>,
    pending: &SymbolicValue<S::Term>,
) -> Result<(), VerifyError> {
    map_collections(next, pending, &mut |next, pending| {
        if matches!(next, SymbolicValue::Collection { .. })
            || matches!(pending, SymbolicValue::Collection { .. })
        {
            *next = ite_value(solver, model, selected, pending, next)?;
        }
        Ok(())
    })
}

pub(crate) fn compute_updates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    pending: SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    compute_updates_in(
        solver, model, statements, read_state, pending, bindings, None,
    )
}

pub(crate) fn compute_updates_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statements: &[Statement],
    read_state: &SymbolicState<S::Term>,
    mut pending: SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    for statement in statements {
        pending = compute_statement(solver, model, statement, read_state, pending, bindings, ctx)?;
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
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let target_ty = model
                .state_lvalue_type(target)
                .map_err(|error| VerifyError::new(error.message))?;
            let assigned_value = eval_expected_in(
                solver, model, value, &target_ty, read_state, bindings, None, ctx,
            )?;
            assign(
                solver,
                model,
                target,
                assigned_value,
                read_state,
                &mut pending,
                bindings,
                ctx,
            )?;
            Ok(pending)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let condition = eval_in(solver, model, condition, read_state, bindings, None, ctx)?;
            let then_state = compute_updates_in(
                solver,
                model,
                then_statements,
                read_state,
                pending.clone(),
                &mut bindings.clone(),
                ctx,
            )?;
            let else_state = compute_updates_in(
                solver,
                model,
                else_statements,
                read_state,
                pending,
                &mut bindings.clone(),
                ctx,
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
            for (name, value, membership) in
                binder_values_in(solver, model, binder, read_state, bindings, None, ctx)?
            {
                let mut local = bindings.clone();
                local.insert(name, enumerated_binder_binding(solver, model, value));
                let where_term =
                    binder_where_in(solver, model, binder, read_state, &mut local, None, ctx)?;
                let condition = binder_condition(solver, membership, where_term)?;
                let candidate = compute_updates_in(
                    solver,
                    model,
                    statements,
                    read_state,
                    pending.clone(),
                    &mut local,
                    ctx,
                )?;
                pending = merge_states(solver, model, &condition, &candidate, &pending)?;
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
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<(), VerifyError> {
    match target {
        LValue::Var(name) => {
            let ty = model
                .state_type(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            target_state.insert(name.clone(), coerce(solver, model, value, ty)?);
        }
        LValue::Index(name, index_expr) => {
            let index = eval_in(solver, model, index_expr, read_state, bindings, None, ctx)?;
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
                let index = eval_in(solver, model, index_expr, read_state, bindings, None, ctx)?;
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
