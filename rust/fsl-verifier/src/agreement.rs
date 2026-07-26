// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelExpr, KernelModel, ParamDef, TypeDef, TypeRef};
use fsl_solver::{SatResult, SmtSolver, Sort};

use crate::VerifyError;
use crate::eval::{definedness, eval};
use crate::transition::{
    action_guard_definedness, action_guards, action_instances, action_statements_definedness,
    transition_constraint,
};
use crate::value::{
    Bindings, SymbolicState, SymbolicValue, bool_term, bounds, concrete_value, logical_equal,
    project_state, symbolic_state_with_suffix,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImplicationResult {
    Implied,
    Counterexample(BTreeMap<String, FslValue>),
}

/// Check that one successful concrete Monitor step is admitted by the symbolic
/// transition relation for the same action instance and successor state.
///
/// # Errors
///
/// Returns [`VerifyError`] for an incomplete state, unknown action/parameter,
/// ill-typed concrete value, symbolic evaluation failure, or unknown solver
/// result.
pub async fn transition_matches_step<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    next: &BTreeMap<String, FslValue>,
) -> Result<bool, VerifyError> {
    solver.push();
    let checked = transition_matches_step_with_bounds(
        model, solver, current, action, params, next, false, false,
    )
    .await;
    let popped = solver.pop(1);
    popped?;
    checked
}

#[allow(clippy::too_many_arguments)]
async fn transition_matches_step_with_bounds<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    next: &BTreeMap<String, FslValue>,
    allow_next_bound_violation: bool,
    totalize_partial_values: bool,
) -> Result<bool, VerifyError> {
    let definition = model
        .actions
        .iter()
        .find(|definition| definition.name == action)
        .ok_or_else(|| VerifyError::new(format!("unknown agreement action '{action}'")))?;
    if definition.params.len() != params.len()
        || definition
            .params
            .iter()
            .any(|parameter| !params.contains_key(parameter.name()))
    {
        return Err(VerifyError::new(format!(
            "agreement parameters do not match action '{action}'"
        )));
    }

    let mut symbolic_current = symbolic_state_with_suffix(solver, model, "agreement_current")?;
    let mut symbolic_next = symbolic_state_with_suffix(solver, model, "agreement_next")?;
    pin_state(solver, model, &symbolic_current, current, "current", false)?;
    pin_state(
        solver,
        model,
        &symbolic_next,
        next,
        "next",
        allow_next_bound_violation,
    )?;
    if totalize_partial_values {
        totalize_agreement_state(solver, model, &mut symbolic_current)?;
        totalize_agreement_state(solver, model, &mut symbolic_next)?;
    }
    let instances = action_instances(solver, model)?;
    let choice = solver.constant("agreement_action_choice", &Sort::Int)?;
    let transition = transition_constraint(
        solver,
        model,
        &instances,
        &symbolic_current,
        &symbolic_next,
        &choice,
    )?;
    solver.assert(&transition)?;

    for (index, instance) in instances.iter().enumerate() {
        if instance.action != action {
            continue;
        }
        solver.push();
        let assertions = (|| -> Result<(), VerifyError> {
            let index = i64::try_from(index)
                .map_err(|_| VerifyError::new("too many action instances for agreement"))?;
            solver.assert(&solver.equal(&choice, &solver.int_value(index))?)?;
            for parameter in &definition.params {
                let name = parameter.name();
                let value = params.get(name).ok_or_else(|| {
                    VerifyError::new(format!("agreement parameter '{name}' is missing"))
                })?;
                let ty = match parameter {
                    ParamDef::Typed { ty, .. } => ty.clone(),
                    ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
                };
                let expected = concrete_value(solver, model, &ty, value)?;
                let actual = instance.params.get(name).ok_or_else(|| {
                    VerifyError::new(format!("symbolic parameter '{name}' is missing"))
                })?;
                solver.assert(&logical_equal(solver, model, actual, &expected)?)?;
            }
            Ok(())
        })();
        if let Err(error) = assertions {
            solver.pop(1)?;
            return Err(error);
        }
        let result = solver.check().await;
        solver.pop(1)?;
        match result? {
            SatResult::Sat => return Ok(true),
            SatResult::Unsat => {}
            SatResult::Unknown => {
                return Err(VerifyError::new(
                    "solver returned unknown in transition agreement",
                ));
            }
        }
    }
    Ok(false)
}

/// Check the solver-provable parts of a concrete outcome, including disabled
/// calls, rollback, and the exact non-partial post-update failure phase.
/// Successful outcomes must be admitted by the symbolic transition relation
/// and pass bounds, invariants, transition properties, and ensures. Evidence
/// A non-partial claim whose reached path is undefined is rejected. A claimed
/// partial outcome, or concrete identity that cannot be retained by the bounded
/// symbolic representation, fails closed with [`VerifyError`] until an exact
/// partial-evidence oracle is available.
///
/// # Errors
///
/// Returns [`VerifyError`] for an unknown outcome kind or the same failures as
/// [`transition_matches_step`].
#[allow(clippy::too_many_arguments)]
pub async fn transition_outcome_matches_step<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    committed: &BTreeMap<String, FslValue>,
    attempted: Option<&BTreeMap<String, FslValue>>,
    outcome_kind: &str,
) -> Result<bool, VerifyError> {
    solver.push();
    let checked = transition_outcome_matches_step_scoped(
        model,
        solver,
        current,
        action,
        params,
        committed,
        attempted,
        outcome_kind,
    )
    .await;
    let popped = solver.pop(1);
    popped?;
    checked
}

#[allow(clippy::too_many_arguments)]
async fn transition_outcome_matches_step_scoped<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    committed: &BTreeMap<String, FslValue>,
    attempted: Option<&BTreeMap<String, FslValue>>,
    outcome_kind: &str,
) -> Result<bool, VerifyError> {
    match outcome_kind {
        "ok" => {
            if attempted.is_some() {
                return Ok(false);
            }
            if !transition_is_admitted(model, solver, current, action, params, committed, false)
                .await?
            {
                return Ok(false);
            }
            if !action_execution_is_defined(model, solver, current, action, params).await? {
                return Ok(false);
            }
            post_outcome_matches(
                model,
                solver,
                current,
                action,
                params,
                committed,
                outcome_kind,
            )
            .await
        }
        "requires_failed" => {
            if committed != current || attempted.is_some() {
                return Ok(false);
            }
            action_is_disabled_and_defined(model, solver, current, action, params).await
        }
        "partial_op" => {
            if committed != current {
                return Ok(false);
            }
            validate_action_parameters(model, action, params)?;
            pin_concrete_state(model, current, "current", false)?;
            if let Some(attempted) = attempted {
                pin_concrete_state(model, attempted, "attempted", true)?;
            }
            Err(VerifyError::new(
                "exact partial-operation agreement is not implemented",
            ))
        }
        "type_bound" | "invariant" | "trans" | "ensures" => {
            let Some(attempted) = attempted else {
                return Ok(false);
            };
            if committed != current {
                return Ok(false);
            }
            if !transition_is_admitted(
                model,
                solver,
                current,
                action,
                params,
                attempted,
                outcome_kind == "type_bound",
            )
            .await?
            {
                return Ok(false);
            }
            if !action_execution_is_defined(model, solver, current, action, params).await? {
                return Ok(false);
            }
            post_outcome_matches(
                model,
                solver,
                current,
                action,
                params,
                attempted,
                outcome_kind,
            )
            .await
        }
        other => Err(VerifyError::new(format!(
            "unknown transition agreement outcome '{other}'"
        ))),
    }
}

async fn transition_is_admitted<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    next: &BTreeMap<String, FslValue>,
    allow_next_bound_violation: bool,
) -> Result<bool, VerifyError> {
    solver.push();
    let checked = transition_matches_step_with_bounds(
        model,
        solver,
        current,
        action,
        params,
        next,
        allow_next_bound_violation,
        true,
    )
    .await;
    let popped = solver.pop(1);
    popped?;
    checked
}

#[derive(Clone, Copy)]
enum ActionCondition {
    EnabledAndDefined,
    DisabledAndDefined,
}

async fn action_execution_is_defined<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
) -> Result<bool, VerifyError> {
    action_condition_is_implied(
        model,
        solver,
        current,
        action,
        params,
        ActionCondition::EnabledAndDefined,
    )
    .await
}

async fn action_is_disabled_and_defined<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
) -> Result<bool, VerifyError> {
    action_condition_is_implied(
        model,
        solver,
        current,
        action,
        params,
        ActionCondition::DisabledAndDefined,
    )
    .await
}

async fn action_condition_is_implied<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    expected: ActionCondition,
) -> Result<bool, VerifyError> {
    solver.push();
    let assertions = (|| -> Result<S::Term, VerifyError> {
        let definition = validate_action_parameters(model, action, params)?;
        let mut symbolic_current =
            symbolic_state_with_suffix(solver, model, "agreement_action_current")?;
        pin_state(solver, model, &symbolic_current, current, "current", false)?;
        totalize_agreement_state(solver, model, &mut symbolic_current)?;
        let symbolic_params = symbolic_action_parameters(solver, model, definition, params)?;
        let status = action_guard_definedness(
            solver,
            model,
            definition,
            &symbolic_current,
            &symbolic_params,
        )?;
        let condition = match expected {
            ActionCondition::EnabledAndDefined => {
                let statements_defined = action_statements_definedness(
                    solver,
                    model,
                    definition,
                    &symbolic_current,
                    &status.bindings,
                )?;
                solver.and(&[status.defined, status.enabled, statements_defined])?
            }
            ActionCondition::DisabledAndDefined => {
                solver.and(&[status.defined, solver.not(&status.enabled)?])?
            }
        };
        solver.assert(&solver.not(&condition)?)?;
        Ok(condition)
    })();
    if let Err(error) = assertions {
        solver.pop(1)?;
        return Err(error);
    }
    let result = solver.check().await;
    solver.pop(1)?;
    match result? {
        SatResult::Unsat => Ok(true),
        SatResult::Sat => Ok(false),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in action-outcome agreement",
        )),
    }
}

async fn post_outcome_matches<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    next: &BTreeMap<String, FslValue>,
    outcome_kind: &str,
) -> Result<bool, VerifyError> {
    let definition = validate_action_parameters(model, action, params)?;
    let mut symbolic_current =
        symbolic_state_with_suffix(solver, model, "agreement_outcome_current")?;
    let mut symbolic_next = symbolic_state_with_suffix(solver, model, "agreement_outcome_next")?;
    pin_state(solver, model, &symbolic_current, current, "current", false)?;
    pin_state(
        solver,
        model,
        &symbolic_next,
        next,
        "next",
        outcome_kind == "type_bound",
    )?;
    totalize_agreement_state(solver, model, &mut symbolic_current)?;
    totalize_agreement_state(solver, model, &mut symbolic_next)?;

    let bound_terms = model
        .state
        .iter()
        .map(|(name, _)| {
            bounds(
                solver,
                model,
                symbolic_next.get(name).ok_or_else(|| {
                    VerifyError::new(format!("symbolic next state is missing '{name}'"))
                })?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bounds_hold = solver.and(&bound_terms)?;

    let (invariants_hold, invariant_fails) = ordered_property_phase(
        solver,
        model,
        &model.invariants,
        &symbolic_next,
        &symbolic_current,
    )?;
    let (transitions_hold, transition_fails) = ordered_property_phase(
        solver,
        model,
        &model.transitions,
        &symbolic_next,
        &symbolic_current,
    )?;

    let symbolic_params = symbolic_action_parameters(solver, model, definition, params)?;
    let (_, mut bindings) = action_guards(
        solver,
        model,
        definition,
        &symbolic_current,
        &symbolic_params,
    )?;
    let (ensures_hold, ensure_fails) = ordered_expression_phase(
        solver,
        model,
        &definition.ensures,
        &symbolic_next,
        &mut bindings,
        Some(&symbolic_current),
    )?;

    let condition = match outcome_kind {
        "ok" => solver.and(&[bounds_hold, invariants_hold, transitions_hold, ensures_hold])?,
        "type_bound" => solver.not(&bounds_hold)?,
        "invariant" => solver.and(&[bounds_hold, invariant_fails])?,
        "trans" => solver.and(&[bounds_hold, invariants_hold, transition_fails])?,
        "ensures" => solver.and(&[bounds_hold, invariants_hold, transitions_hold, ensure_fails])?,
        _ => unreachable!("post outcome kind was matched by the caller"),
    };
    solver.assert(&solver.not(&condition)?)?;
    match solver.check().await? {
        SatResult::Unsat => Ok(true),
        SatResult::Sat => Ok(false),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in post-outcome agreement",
        )),
    }
}

fn ordered_property_phase<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    properties: &[fsl_core::PropertyDef],
    next: &SymbolicState<S::Term>,
    current: &SymbolicState<S::Term>,
) -> Result<(S::Term, S::Term), VerifyError> {
    let mut prefix = solver.bool_value(true);
    let mut failures = Vec::new();
    for property in properties {
        let mut bindings = Bindings::new();
        let expression_defined = definedness(
            solver,
            model,
            &property.expr,
            next,
            &bindings,
            Some(current),
        )?;
        let value = eval(
            solver,
            model,
            &property.expr,
            next,
            &mut bindings,
            Some(current),
        )?;
        let value = bool_term(&value)?.clone();
        failures.push(solver.and(&[
            prefix.clone(),
            expression_defined.clone(),
            solver.not(&value)?,
        ])?);
        prefix = solver.and(&[prefix, expression_defined, value])?;
    }
    Ok((prefix, solver.or(&failures)?))
}

fn ordered_expression_phase<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expressions: &[fsl_core::KernelExpr],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<(S::Term, S::Term), VerifyError> {
    let mut prefix = solver.bool_value(true);
    let mut failures = Vec::new();
    for expression in expressions {
        let expression_defined =
            definedness(solver, model, expression, state, bindings, old_state)?;
        let value = eval(solver, model, expression, state, bindings, old_state)?;
        let value = bool_term(&value)?.clone();
        failures.push(solver.and(&[
            prefix.clone(),
            expression_defined.clone(),
            solver.not(&value)?,
        ])?);
        prefix = solver.and(&[prefix, expression_defined, value])?;
    }
    Ok((prefix, solver.or(&failures)?))
}

fn symbolic_action_parameters<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    definition: &fsl_core::ActionDef,
    params: &BTreeMap<String, FslValue>,
) -> Result<Bindings<S::Term>, VerifyError> {
    definition
        .params
        .iter()
        .map(|parameter| {
            let name = parameter.name();
            let value = params.get(name).ok_or_else(|| {
                VerifyError::new(format!("agreement parameter '{name}' is missing"))
            })?;
            let ty = match parameter {
                ParamDef::Typed { ty, .. } => ty.clone(),
                ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
            };
            Ok((name.to_owned(), concrete_value(solver, model, &ty, value)?))
        })
        .collect()
}

fn validate_action_parameters<'a>(
    model: &'a KernelModel,
    action: &str,
    params: &BTreeMap<String, FslValue>,
) -> Result<&'a fsl_core::ActionDef, VerifyError> {
    let definition = model
        .actions
        .iter()
        .find(|definition| definition.name == action)
        .ok_or_else(|| VerifyError::new(format!("unknown agreement action '{action}'")))?;
    if definition.params.len() != params.len()
        || definition
            .params
            .iter()
            .any(|parameter| !params.contains_key(parameter.name()))
    {
        return Err(VerifyError::new(format!(
            "agreement parameters do not match action '{action}'"
        )));
    }
    for parameter in &definition.params {
        let name = parameter.name();
        let value = params
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("agreement parameter '{name}' is missing")))?;
        let domain = match parameter {
            ParamDef::Typed { ty, .. } => model.domain_values(ty)?,
            ParamDef::Range { lo, hi, .. } => (*lo..=*hi).map(FslValue::Int).collect(),
        };
        if !domain.contains(value) {
            return Err(VerifyError::new(format!(
                "agreement parameter '{name}' is outside the action domain"
            )));
        }
    }
    Ok(definition)
}

fn pin_concrete_state(
    model: &KernelModel,
    state: &BTreeMap<String, FslValue>,
    label: &str,
    allow_bound_violation: bool,
) -> Result<(), VerifyError> {
    if state.len() != model.state.len() {
        return Err(VerifyError::new(format!(
            "agreement {label} state has the wrong number of variables"
        )));
    }
    for (name, ty) in &model.state {
        let value = state.get(name).ok_or_else(|| {
            VerifyError::new(format!("agreement {label} state is missing '{name}'"))
        })?;
        validate_concrete_value_shape(model, ty, value, allow_bound_violation)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_concrete_value_shape(
    model: &KernelModel,
    ty: &TypeRef,
    value: &FslValue,
    allow_bound_violation: bool,
) -> Result<(), VerifyError> {
    match (ty, value) {
        (TypeRef::Bool, FslValue::Bool(_))
        | (TypeRef::Int, FslValue::Int(_))
        | (TypeRef::Option(_), FslValue::None) => Ok(()),
        (TypeRef::Range(lo, hi), FslValue::Int(value)) => {
            if allow_bound_violation || lo <= value && value <= hi {
                Ok(())
            } else {
                Err(VerifyError::new("agreement value is outside its range"))
            }
        }
        (TypeRef::Named(name), FslValue::Int(value)) => {
            let Some(TypeDef::Domain { lo, hi, .. }) = model.types.get(name) else {
                return Err(VerifyError::new(format!("'{name}' is not a domain")));
            };
            if allow_bound_violation || lo <= value && value <= hi {
                Ok(())
            } else {
                Err(VerifyError::new(format!(
                    "agreement value is outside domain '{name}'"
                )))
            }
        }
        (TypeRef::Named(name), FslValue::Enum { type_name, member }) if name == type_name => {
            let Some(TypeDef::Enum { members, .. }) = model.types.get(name) else {
                return Err(VerifyError::new(format!("'{name}' is not an enum")));
            };
            if members.contains(member) {
                Ok(())
            } else {
                Err(VerifyError::new(format!("unknown enum member '{member}'")))
            }
        }
        (TypeRef::Named(name), FslValue::Struct { type_name, fields }) if name == type_name => {
            let Some(TypeDef::Struct { fields: expected }) = model.types.get(name) else {
                return Err(VerifyError::new(format!("'{name}' is not a struct")));
            };
            if fields.len() != expected.len() {
                return Err(VerifyError::new(format!(
                    "agreement struct '{name}' has the wrong number of fields"
                )));
            }
            for (field, field_ty) in expected {
                let field_value = fields.get(field).ok_or_else(|| {
                    VerifyError::new(format!("agreement struct '{name}' is missing '{field}'"))
                })?;
                validate_concrete_value_shape(model, field_ty, field_value, allow_bound_violation)?;
            }
            Ok(())
        }
        (TypeRef::Option(inner), FslValue::Some(value)) => {
            validate_concrete_value_shape(model, inner, value, allow_bound_violation)
        }
        (TypeRef::Map(key_ty, value_ty), FslValue::Map(values)) => {
            let expected = model.map_key_values(key_ty)?;
            if values.len() != expected.len()
                || expected.iter().any(|key| !values.contains_key(key))
            {
                return Err(VerifyError::new(
                    "agreement map keys do not match the finite key domain",
                ));
            }
            values.values().try_for_each(|value| {
                validate_concrete_value_shape(model, value_ty, value, allow_bound_violation)
            })
        }
        (TypeRef::Set(element_ty), FslValue::Set(values)) => {
            let domain = model.domain_values(element_ty)?;
            if values.iter().any(|value| !domain.contains(value)) {
                return Err(VerifyError::new(
                    "agreement set contains a value outside its finite domain",
                ));
            }
            Ok(())
        }
        (TypeRef::Seq(element_ty, capacity), FslValue::Seq(values)) => {
            if values.len() > *capacity {
                return Err(VerifyError::new(
                    "exact agreement cannot represent an over-capacity sequence",
                ));
            }
            values.iter().try_for_each(|value| {
                validate_concrete_value_shape(model, element_ty, value, allow_bound_violation)
            })
        }
        (TypeRef::Relation(source_ty, target_ty), FslValue::Relation(values)) => {
            let sources = model.domain_values(source_ty)?;
            let targets = model.domain_values(target_ty)?;
            if values
                .iter()
                .any(|(source, target)| !sources.contains(source) || !targets.contains(target))
            {
                return Err(VerifyError::new(
                    "agreement relation contains a pair outside its finite domains",
                ));
            }
            Ok(())
        }
        _ => Err(VerifyError::new(format!(
            "agreement value does not match type {ty:?}"
        ))),
    }
}

fn totalize_agreement_state<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &mut SymbolicState<S::Term>,
) -> Result<(), VerifyError> {
    state
        .values_mut()
        .try_for_each(|value| totalize_agreement_value(solver, model, value))
}

fn totalize_agreement_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &mut SymbolicValue<S::Term>,
) -> Result<(), VerifyError> {
    match value {
        SymbolicValue::Option { value, .. } => totalize_agreement_value(solver, model, value),
        SymbolicValue::Struct { fields, .. } => fields
            .values_mut()
            .try_for_each(|value| totalize_agreement_value(solver, model, value)),
        SymbolicValue::Map { entries, .. } => entries
            .iter_mut()
            .try_for_each(|(_, value)| totalize_agreement_value(solver, model, value)),
        SymbolicValue::Seq { ty, slots, .. } => {
            let TypeRef::Seq(element_ty, capacity) = ty else {
                unreachable!();
            };
            if *capacity == 0 && slots.is_empty() {
                let mut fallback =
                    concrete_value(solver, model, element_ty, &model.default_value(element_ty)?)?;
                totalize_agreement_value(solver, model, &mut fallback)?;
                slots.push(fallback);
            }
            slots
                .iter_mut()
                .try_for_each(|value| totalize_agreement_value(solver, model, value))
        }
        SymbolicValue::SetLiteral(values) | SymbolicValue::SeqLiteral(values) => values
            .iter_mut()
            .try_for_each(|value| totalize_agreement_value(solver, model, value)),
        SymbolicValue::Scalar { .. }
        | SymbolicValue::None
        | SymbolicValue::Set { .. }
        | SymbolicValue::Relation { .. } => Ok(()),
    }
}

fn pin_state<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    symbolic: &crate::value::SymbolicState<S::Term>,
    concrete: &BTreeMap<String, FslValue>,
    label: &str,
    allow_bound_violation: bool,
) -> Result<(), VerifyError> {
    pin_concrete_state(model, concrete, label, allow_bound_violation)?;
    for (name, ty) in &model.state {
        let value = concrete.get(name).ok_or_else(|| {
            VerifyError::new(format!("agreement {label} state is missing '{name}'"))
        })?;
        let expected = concrete_value(solver, model, ty, value)?;
        let actual = symbolic
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("symbolic state is missing '{name}'")))?;
        solver.assert(&logical_equal(solver, model, actual, &expected)?)?;
    }
    Ok(())
}

/// Pin a symbolic state to one concrete Monitor state and prove that evaluating
/// `expression` cannot differ from the concrete evaluator's `expected` value.
///
/// This is the reusable half of the dual-evaluator agreement gate. Keeping the
/// concrete evaluator outside this crate preserves the solver-free runtime
/// dependency boundary.
///
/// # Errors
///
/// Returns [`VerifyError`] for incomplete states, ill-typed values/expressions,
/// or an unknown solver result.
pub async fn expression_matches_value<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    expression: &KernelExpr,
    state: &BTreeMap<String, FslValue>,
    expected: &FslValue,
) -> Result<bool, VerifyError> {
    let symbolic = symbolic_state_with_suffix(solver, model, "agreement")?;
    for (name, ty) in &model.state {
        let value = state
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("agreement state is missing '{name}'")))?;
        let concrete = concrete_value(solver, model, ty, value)?;
        let symbolic_value = symbolic
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("symbolic state is missing '{name}'")))?;
        solver.assert(&logical_equal(solver, model, symbolic_value, &concrete)?)?;
    }
    if state.len() != model.state.len() {
        return Err(VerifyError::new(
            "agreement state contains an unknown state variable",
        ));
    }
    let actual = eval(
        solver,
        model,
        expression,
        &symbolic,
        &mut Bindings::new(),
        None,
    )?;
    let ty = actual
        .ty()
        .ok_or_else(|| VerifyError::new("agreement expression has no concrete type"))?;
    let expected = concrete_value(solver, model, ty, expected)?;
    let equal = logical_equal(solver, model, &actual, &expected)?;
    solver.assert(&solver.not(&equal)?)?;
    match solver.check().await? {
        SatResult::Unsat => Ok(true),
        SatResult::Sat => Ok(false),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in expression agreement",
        )),
    }
}

/// Decide whether every type-correct state satisfying `antecedent` invariants
/// also satisfies `consequent` invariants, returning a concrete counterexample
/// when implication fails.
///
/// # Errors
///
/// Returns [`VerifyError`] for incompatible state schemas, symbolic evaluation
/// failures, or an unknown solver result.
pub async fn invariant_implication<S: SmtSolver>(
    antecedent: &KernelModel,
    consequent: &KernelModel,
    solver: &mut S,
) -> Result<ImplicationResult, VerifyError> {
    if antecedent.state != consequent.state {
        return Err(VerifyError::new("invariant state schemas differ"));
    }
    let state = symbolic_state_with_suffix(solver, consequent, "implication")?;
    for (name, _) in &consequent.state {
        solver.assert(&bounds(
            solver,
            consequent,
            state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
        )?)?;
    }
    for property in &antecedent.invariants {
        let value = eval(
            solver,
            antecedent,
            &property.expr,
            &state,
            &mut Bindings::new(),
            None,
        )?;
        solver.assert(bool_term(&value)?)?;
    }
    let mut consequents = Vec::new();
    for property in &consequent.invariants {
        let value = eval(
            solver,
            consequent,
            &property.expr,
            &state,
            &mut Bindings::new(),
            None,
        )?;
        consequents.push(bool_term(&value)?.clone());
    }
    let conjunction = solver.and(&consequents)?;
    solver.assert(&solver.not(&conjunction)?)?;
    match solver.check().await? {
        SatResult::Unsat => Ok(ImplicationResult::Implied),
        SatResult::Sat => Ok(ImplicationResult::Counterexample(project_state(
            solver, consequent, &state,
        )?)),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in invariant implication",
        )),
    }
}
