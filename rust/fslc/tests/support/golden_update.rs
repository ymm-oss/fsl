// SPDX-License-Identifier: Apache-2.0

//! Keyword golden updates are local-only; CI always compares without rewriting.

pub fn golden_update_requested(update: Option<&str>, ci: Option<&str>) -> Result<bool, String> {
    if update.is_some() && ci.is_some() {
        return Err("CI must not set FSLC_SKILLS_KEYWORDS_UPDATE".to_owned());
    }
    Ok(update == Some("1"))
}
