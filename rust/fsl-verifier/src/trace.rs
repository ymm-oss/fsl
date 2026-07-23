// SPDX-License-Identifier: Apache-2.0

//! Shared symbolic trace projection for verifier engines.

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelModel, TraceAction, TraceChange, TraceStep};
use fsl_solver::{ModelValue, SmtSolver};

use crate::VerifyError;
use crate::transition::ActionInstance;
use crate::value::{SymbolicState, project_state, project_value};

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
        let state = project_state(solver, model, &states[step])?;
        let action = if step == 0 {
            None
        } else {
            Some(project_action(
                solver,
                model,
                &choices[step - 1],
                instances,
            )?)
        };
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

fn project_action<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    choice: &S::Term,
    instances: &[ActionInstance<S::Term>],
) -> Result<TraceAction, VerifyError> {
    let index = match solver.model_eval(choice)? {
        Some(ModelValue::Int(value)) => usize::try_from(value)
            .map_err(|_| VerifyError::new("negative action choice in model"))?,
        Some(ModelValue::Bool(_)) => {
            return Err(VerifyError::new("Boolean action choice in model"));
        }
        None => return Err(VerifyError::new("action choice is unavailable in model")),
    };
    let instance = instances
        .get(index)
        .ok_or_else(|| VerifyError::new("action choice outside instance range"))?;
    Ok(TraceAction {
        name: instance.action.clone(),
        params: instance
            .params
            .iter()
            .map(|(name, value)| Ok((name.clone(), project_value(solver, model, value)?)))
            .collect::<Result<BTreeMap<String, FslValue>, VerifyError>>()?,
    })
}
