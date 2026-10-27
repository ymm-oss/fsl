// SPDX-License-Identifier: Apache-2.0

//! Shared symbolic trace projection for verifier engines.

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelModel, TraceAction, TraceChange, TraceStep};
use fsl_solver::{ModelValue, SmtSolver};

use crate::VerifyError;
use crate::transition::{ActionInstance, action_guards, compute_updates};
use crate::value::{SymbolicState, SymbolicValue, i64_index, project_state, project_value};

pub(crate) fn project_trace<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    upto: usize,
) -> Result<Vec<TraceStep>, VerifyError> {
    let mut trace = Vec::new();
    for step in 0..=upto {
        let instance = if step == 0 {
            None
        } else {
            Some(chosen_instance(solver, &choices[step - 1], instances)?)
        };
        let state = if let Some(instance) = instance
            && has_missing_sequence_slots(solver, &states[step])?
        {
            // Frames have the declared capacity, while a failed assignment can
            // have a longer attempted value. Recover its real suffix from the
            // selected action, never from padding or a clamped frame length.
            let previous = &states[step - 1];
            let action = &model.actions[instance.action_index];
            let (_, mut bindings) =
                action_guards(solver, model, action, previous, &instance.params)?;
            let attempted = compute_updates(
                solver,
                model,
                &action.statements,
                previous,
                previous.clone(),
                &mut bindings,
            )?;
            project_state(solver, model, &attempted)?
        } else {
            project_state(solver, model, &states[step])?
        };
        let action = instance
            .map(|instance| project_action(solver, model, instance))
            .transpose()?;
        let changes = trace
            .last()
            .map_or_else(BTreeMap::new, |previous: &TraceStep| {
                state
                    .iter()
                    .filter_map(|(name, value)| {
                        let before = &previous.state[name];
                        (before != value).then(|| {
                            (
                                name.clone(),
                                TraceChange {
                                    from: before.clone(),
                                    to: value.clone(),
                                },
                            )
                        })
                    })
                    .collect()
            });
        trace.push(TraceStep {
            step,
            state,
            action,
            changes,
        });
    }
    Ok(trace)
}

pub(crate) fn has_missing_sequence_slots<S: SmtSolver>(
    solver: &S,
    state: &SymbolicState<S::Term>,
) -> Result<bool, VerifyError> {
    for value in state.values() {
        if let SymbolicValue::Seq { slots, len, .. } = value
            && let Some(ModelValue::Int(len)) = solver.model_eval(len)?
            && len > i64_index(slots.len())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn chosen_instance<'a, S: SmtSolver>(
    solver: &S,
    choice: &S::Term,
    instances: &'a [ActionInstance<S::Term>],
) -> Result<&'a ActionInstance<S::Term>, VerifyError> {
    let index = match solver.model_eval(choice)? {
        Some(ModelValue::Int(value)) => usize::try_from(value)
            .map_err(|_| VerifyError::new("negative action choice in model"))?,
        Some(ModelValue::Bool(_)) => {
            return Err(VerifyError::new("Boolean action choice in model"));
        }
        None => return Err(VerifyError::solver("action choice is unavailable in model")),
    };
    instances
        .get(index)
        .ok_or_else(|| VerifyError::new("action choice outside instance range"))
}

fn project_action<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    instance: &ActionInstance<S::Term>,
) -> Result<TraceAction, VerifyError> {
    Ok(TraceAction {
        name: instance.action.clone(),
        params: instance
            .params
            .iter()
            .map(|(name, value)| Ok((name.clone(), project_value(solver, model, value)?)))
            .collect::<Result<BTreeMap<String, FslValue>, VerifyError>>()?,
    })
}
