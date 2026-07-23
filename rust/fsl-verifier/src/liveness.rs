// SPDX-License-Identifier: Apache-2.0

//! Shared symbolic liveness constraints for verifier engines.

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelBinder, KernelModel, LeadsToDef, static_leadsto_bindings};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::eval::{binder_values, eval};
use crate::value::{Bindings, SymbolicState, bool_term};

#[derive(Clone)]
pub(crate) struct LeadstoBinding<T> {
    pub(crate) concrete: BTreeMap<String, FslValue>,
    pub(crate) symbolic: Bindings<T>,
}

pub(crate) fn leadsto_bindings<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    property: &LeadsToDef,
) -> Result<Vec<LeadstoBinding<S::Term>>, VerifyError> {
    let mut expanded = vec![Bindings::new()];
    for binder in &property.binders {
        let symbolic = binder_values(solver, model, binder)?;
        let name = match binder {
            KernelBinder::Typed { name, .. }
            | KernelBinder::Range { name, .. }
            | KernelBinder::Collection { name, .. } => name,
        };
        let mut next = Vec::new();
        for binding in expanded {
            for (_, term) in &symbolic {
                let mut candidate = binding.clone();
                candidate.insert(name.clone(), term.clone());
                next.push(candidate);
            }
        }
        expanded = next;
    }
    let concrete = static_leadsto_bindings(model, property)?;
    if concrete.len() != expanded.len() {
        return Err(VerifyError::new("leadsTo binder expansion mismatch"));
    }
    Ok(concrete
        .into_iter()
        .zip(expanded)
        .map(|(concrete, symbolic)| LeadstoBinding { concrete, symbolic })
        .collect())
}

pub(crate) fn leadsto_condition<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &fsl_core::KernelExpr,
    state: &SymbolicState<S::Term>,
    binding: &Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    let mut binding = binding.clone();
    let value = eval(solver, model, expr, state, &mut binding, None)?;
    Ok(bool_term(&value)?.clone())
}
