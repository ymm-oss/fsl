// SPDX-License-Identifier: Apache-2.0

//! Backend-neutral symbolic semantics and bounded verification for FSL.

mod agreement;
mod bmc;
mod eval;
mod induction;
mod liveness;
mod refinement;
mod trace;
mod transition;
mod value;

use std::error::Error;
use std::fmt;

use fsl_core::ModelError;
use fsl_solver::SolverError;

pub use agreement::{
    ImplicationResult, expression_matches_value, invariant_implication, transition_matches_step,
    transition_outcome_matches_step,
};
pub use bmc::{
    BmcResult, BmcViolation, LeadsToViolation, ReachableWitness, verify_bounded,
    verify_bounded_from_state, verify_bounded_selected,
};
pub use induction::{
    InductionCti, InductionResult, RankFailure, RankProof, RankedLeadstoResult, prove_induction,
    prove_ranked_leadstos,
};
pub use refinement::{ProgressCheck, check_refinement_progress};
pub use value::{Bindings, SymbolicState, SymbolicValue};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyError {
    pub message: String,
}

impl VerifyError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for VerifyError {}

impl From<SolverError> for VerifyError {
    fn from(error: SolverError) -> Self {
        Self::new(error.message())
    }
}

impl From<ModelError> for VerifyError {
    fn from(error: ModelError) -> Self {
        Self::new(error.message)
    }
}
