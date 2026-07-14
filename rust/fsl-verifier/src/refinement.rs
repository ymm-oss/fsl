// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use fsl_core::{KernelModel, LeadsToDef, Refinement, substitute_expr};
use fsl_solver::SmtSolver;

use crate::{BmcViolation, VerifyError, verify_bounded};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressCheck {
    pub violation: Option<BmcViolation>,
    pub checked: BTreeMap<String, Vec<String>>,
}

/// Pull abstract `leadsTo` properties through scalar refinement maps and check
/// them over the implementation transition system.
///
/// # Errors
///
/// Returns [`VerifyError`] for indexed progress maps in the current slice,
/// missing properties, or bounded-verifier failures.
pub async fn check_refinement_progress<S: SmtSolver>(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    mapping: &Refinement,
    solver: &mut S,
    depth: usize,
) -> Result<ProgressCheck, VerifyError> {
    if mapping.progress.is_empty() {
        return Ok(ProgressCheck {
            violation: None,
            checked: BTreeMap::new(),
        });
    }
    let replacements = mapping
        .state_maps
        .iter()
        .map(|(name, state_map)| {
            if state_map.binder.is_some() {
                return Err(VerifyError::new(format!(
                    "indexed progress map for '{name}' is not implemented"
                )));
            }
            Ok((name.clone(), state_map.expr.clone()))
        })
        .collect::<Result<HashMap<_, _>, VerifyError>>()?;
    let mut pulled = implementation.clone();
    for (name, definition) in &abstraction.types {
        pulled.types.insert(name.clone(), definition.clone());
    }
    for (name, value) in &abstraction.enum_members {
        pulled.enum_members.insert(name.clone(), value.clone());
    }
    pulled.reachables.clear();
    pulled.leadstos = mapping
        .progress
        .iter()
        .map(|declaration| {
            let property = abstraction
                .leadstos
                .iter()
                .find(|property| property.name == declaration.leads_to)
                .ok_or_else(|| {
                    VerifyError::new(format!(
                        "unknown abstract leadsTo '{}'",
                        declaration.leads_to
                    ))
                })?;
            Ok(LeadsToDef {
                name: property.name.clone(),
                span: property.span,
                binders: property.binders.clone(),
                before: substitute_expr(property.before.clone(), &replacements),
                after: substitute_expr(property.after.clone(), &replacements),
                meta: property.meta.clone(),
                annotations: property.annotations.clone(),
                decreases: property
                    .decreases
                    .clone()
                    .map(|expr| substitute_expr(expr, &replacements)),
                within: property.within,
            })
        })
        .collect::<Result<Vec<_>, VerifyError>>()?;
    let result = verify_bounded(&pulled, solver, depth).await?;
    let checked = mapping
        .progress
        .iter()
        .map(|declaration| (declaration.leads_to.clone(), declaration.actions.clone()))
        .collect();
    Ok(ProgressCheck {
        violation: result.leadsto_violation,
        checked,
    })
}
