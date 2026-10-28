// SPDX-License-Identifier: Apache-2.0

//! Phase-I-owned adapter for the keyword golden's environment policy.
//! This Phase-T legacy body deliberately preserves the missing CI guard.

#[allow(clippy::unnecessary_wraps)] // Phase I adds the error path; keep its approved API now.
pub fn golden_update_requested(update: Option<&str>, _ci: Option<&str>) -> Result<bool, String> {
    Ok(update == Some("1"))
}
