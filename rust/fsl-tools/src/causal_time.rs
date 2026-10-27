// SPDX-License-Identifier: Apache-2.0

//! Shared numeric boundary for causal analysis, not a source acceptance gate.

use std::cmp::Ordering;

use crate::causal::{CausalError, Claim};

pub(crate) fn add_claim_time(
    total: u64,
    increment: u64,
    claim: &Claim,
) -> Result<u64, CausalError> {
    total.checked_add(increment).ok_or_else(|| CausalError {
        kind: "causal_invalid_model",
        message: format!(
            "causal time range overflow while adding claim '{}'",
            claim.id
        ),
        line: claim.line,
        column: claim.column,
    })
}

/// Compare scaled times exactly without exposing a wider output integer.
/// A u64 multiplied by a u8 always fits in u128.
pub(crate) fn scaled_time_cmp(
    left: u64,
    left_factor: u8,
    right: u64,
    right_factor: u8,
) -> Ordering {
    let left = u128::from(left) * u128::from(left_factor);
    let right = u128::from(right) * u128::from(right_factor);
    left.cmp(&right)
}
