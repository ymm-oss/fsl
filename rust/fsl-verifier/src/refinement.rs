// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{KernelModel, Refinement, pull_refinement_leadsto};
use fsl_solver::SmtSolver;

use crate::{BmcViolation, VerifyError, verify_bounded};

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ProgressCheck {
    pub violation: Option<BmcViolation>,
    pub checked: BTreeMap<String, Vec<String>>,
}

/// Pull abstract `leadsTo` properties through scalar and indexed refinement
/// maps and check them over the implementation transition system.
///
/// A scalar map (`map a = expr`) substitutes bare reads of `a`. An indexed
/// map (`map a[i: K] = expr`) substitutes each read `a[e]` with `expr`'s
/// binder `i` replaced by `e` (DESIGN-refinement.md's "substituted on the
/// read" rule), regardless of whether the pulled `leadsTo` also reads other,
/// unrelated indexed maps.
///
/// # Errors
///
/// Returns [`VerifyError`] for missing properties or bounded-verifier
/// failures.
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
            pull_refinement_leadsto(
                implementation,
                abstraction,
                &mapping.state_maps,
                property,
                declaration.span,
            )
            .map_err(|error| VerifyError::new(error.message))
        })
        .collect::<Result<Vec<_>, VerifyError>>()?;
    let result = verify_bounded(&pulled, solver, depth).await?;
    let checked = mapping
        .progress
        .iter()
        .map(|declaration| {
            (
                declaration.leads_to.clone(),
                declaration
                    .actions
                    .iter()
                    .map(|action| action.0.clone())
                    .collect(),
            )
        })
        .collect();
    Ok(ProgressCheck {
        violation: result.leadsto_violation,
        checked,
    })
}
