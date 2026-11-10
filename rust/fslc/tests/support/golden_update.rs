// SPDX-License-Identifier: Apache-2.0

//! Golden updates are local-only. CI rejects a set update variable;
//! without that variable, CI compares without rewriting.

/// Whether `variable` (whose value is `update`) asks to rewrite a golden:
/// only `1` does, and under `CI` any set value is an error naming `variable`.
pub fn golden_update_requested_for(
    variable: &str,
    update: Option<&str>,
    ci: Option<&str>,
) -> Result<bool, String> {
    if update.is_some() && ci.is_some() {
        return Err(format!("CI must not set {variable}"));
    }
    Ok(update == Some("1"))
}

/// [`golden_update_requested_for`] for the skills keyword table.
#[allow(dead_code)] // `edition_header.rs` uses only the generic form.
pub fn golden_update_requested(update: Option<&str>, ci: Option<&str>) -> Result<bool, String> {
    golden_update_requested_for("FSLC_SKILLS_KEYWORDS_UPDATE", update, ci)
}

/// [`golden_update_requested_for`] read from the process environment.
#[allow(dead_code)] // the guard tests use only the pure forms.
pub fn golden_update_requested_from_env(variable: &str) -> Result<bool, String> {
    let update = std::env::var_os(variable);
    let ci = std::env::var_os("CI");
    golden_update_requested_for(
        variable,
        update.as_ref().map(|value| value.to_str().unwrap_or("")),
        ci.as_ref().map(|value| value.to_str().unwrap_or("")),
    )
}
