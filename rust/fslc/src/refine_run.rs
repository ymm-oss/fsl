// SPDX-License-Identifier: Apache-2.0

//! The refinement sequence `fslc refine` (and through it `chain`'s refine
//! layer and native governance preservations) and the Worker's governance
//! preservations both run: load the implementation, load the abstraction,
//! read and parse the mapping, walk the correspondence, and run the progress
//! stage on a walk that refines.
//!
//! Each surface still loads the two sides its own way (native from disk with
//! its requirements refutation, the Worker from the request's files) and
//! renders its own output from the [`RefineRun`] or [`RefineRunError`].

use fsl_core::{KernelModel, Refinement, RefinementError};

use crate::outcome::{RefineOutcome, refine_outcome};
use crate::requirements_solver::NewSolver;

/// A refinement sequence that reached a verdict.
#[derive(Debug)]
pub struct RefineRun {
    pub implementation: KernelModel,
    pub abstraction: KernelModel,
    pub mapping: Refinement,
    pub checked: fsl_runtime::RefinementCheck,
    /// The progress stage, run only when the walk refines and the mapping
    /// declares `preserve progress`: a safety correspondence that was not
    /// decided cannot be the premise of a progress check.
    pub progress: Option<fsl_verifier::ProgressCheck>,
}

impl RefineRun {
    /// The walk's verdict, read through the one owner of its projection.
    pub fn outcome(&self) -> RefineOutcome<'_> {
        refine_outcome(self.checked.verdict())
    }

    /// The progress stage's counterexample: the walk refines, but a
    /// pulled-back abstract `leadsTo` stays pending.
    #[must_use]
    pub fn progress_violation(&self) -> Option<&fsl_verifier::BmcViolation> {
        self.progress
            .as_ref()
            .and_then(|progress| progress.violation.as_ref())
    }
}

/// The stage at which a refinement sequence stopped without a verdict.
/// `E` is the caller's own load failure.
#[derive(Debug)]
pub enum RefineRunError<E> {
    Implementation(E),
    Abstraction(E),
    MappingSource(E),
    /// The mapping does not parse against the two loaded models, named here.
    Mapping {
        error: RefinementError,
        implementation: String,
        abstraction: String,
    },
    Walk(fsl_runtime::RuntimeError),
    /// The progress stage's solver could not be created.
    Solver(String),
    Progress(fsl_verifier::VerifyError),
}

/// Run the refinement sequence in its fixed order. Each loader runs only after
/// the one before it succeeded, and `new_solver` only when the progress stage
/// runs. `walk_depth` bounds the correspondence walk and `progress_depth` the
/// progress stage; native passes its one depth as both, the Worker its fixed 8.
///
/// # Errors
///
/// The first stage that fails, with its own failure.
pub async fn run_refinement<E, S: fsl_solver::SmtSolver>(
    load_implementation: impl FnOnce() -> Result<KernelModel, E>,
    load_abstraction: impl FnOnce() -> Result<KernelModel, E>,
    read_mapping: impl FnOnce() -> Result<String, E>,
    options: &fsl_syntax::ParseOptions,
    walk_depth: usize,
    progress_depth: usize,
    new_solver: &mut NewSolver<'_, S>,
) -> Result<RefineRun, RefineRunError<E>> {
    let implementation = load_implementation().map_err(RefineRunError::Implementation)?;
    let abstraction = load_abstraction().map_err(RefineRunError::Abstraction)?;
    let source = read_mapping().map_err(RefineRunError::MappingSource)?;
    let mapping =
        fsl_core::parse_refinement_with_options(&source, &implementation, &abstraction, options)
            .map_err(|error| RefineRunError::Mapping {
                error,
                implementation: implementation.name.clone(),
                abstraction: abstraction.name.clone(),
            })?;
    let checked =
        fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, walk_depth)
            .map_err(RefineRunError::Walk)?;
    let progress = if refine_outcome(checked.verdict()) == RefineOutcome::Refines
        && !mapping.progress.is_empty()
    {
        let mut solver = new_solver().map_err(RefineRunError::Solver)?;
        Some(
            fsl_verifier::check_refinement_progress(
                &implementation,
                &abstraction,
                &mapping,
                &mut solver,
                progress_depth,
            )
            .await
            .map_err(RefineRunError::Progress)?,
        )
    } else {
        None
    };
    Ok(RefineRun {
        implementation,
        abstraction,
        mapping,
        checked,
        progress,
    })
}
