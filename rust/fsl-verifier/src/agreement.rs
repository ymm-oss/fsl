// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelExpr, KernelModel, ParamDef, TypeRef};
use fsl_solver::{SatResult, SmtSolver, Sort};

use crate::VerifyError;
use crate::eval::eval;
use crate::transition::{action_instances, transition_constraint};
use crate::value::{
    Bindings, bool_term, bounds, concrete_value, logical_equal, project_state,
    symbolic_state_with_suffix,
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

    let symbolic_current = symbolic_state_with_suffix(solver, model, "agreement_current")?;
    let symbolic_next = symbolic_state_with_suffix(solver, model, "agreement_next")?;
    pin_state(solver, model, &symbolic_current, current, "current")?;
    pin_state(solver, model, &symbolic_next, next, "next")?;
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
        let result = solver.check().await?;
        solver.pop(1)?;
        match result {
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

/// Check a complete concrete outcome, including disabled calls and rollback.
/// Successful outcomes must be admitted by the symbolic transition relation.
/// Post-update failures must expose a symbolic attempted successor while the
/// committed state remains the pre-state. Partial operations are concrete-only
/// because SMT integer and collection operators are totalized.
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
    match outcome_kind {
        "ok" => {
            if attempted.is_some() {
                return Ok(false);
            }
            transition_matches_step(model, solver, current, action, params, committed).await
        }
        "requires_failed" => {
            if committed != current || attempted.is_some() {
                return Ok(false);
            }
            Ok(!transition_matches_step(model, solver, current, action, params, current).await?)
        }
        "partial_op" => Ok(committed == current && attempted.is_none()),
        "type_bound" | "invariant" | "trans" | "ensures" => {
            let Some(attempted) = attempted else {
                return Ok(false);
            };
            if committed != current {
                return Ok(false);
            }
            transition_matches_step(model, solver, current, action, params, attempted).await
        }
        other => Err(VerifyError::new(format!(
            "unknown transition agreement outcome '{other}'"
        ))),
    }
}

fn pin_state<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    symbolic: &crate::value::SymbolicState<S::Term>,
    concrete: &BTreeMap<String, FslValue>,
    label: &str,
) -> Result<(), VerifyError> {
    if concrete.len() != model.state.len() {
        return Err(VerifyError::new(format!(
            "agreement {label} state has the wrong number of variables"
        )));
    }
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
