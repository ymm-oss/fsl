// SPDX-License-Identifier: Apache-2.0

//! Render and validate the generated cargo-mutants configuration.
//!
//! The scope manifest's anchors are the sole authority for the line-addressed
//! liveness exclusions. This module is shared by the explicit generator and
//! the manifest freshness check so they cannot carry separate derivations.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

const TEMPLATE_PATH: &str = "rust/.cargo/mutants.toml.in";
const CONFIG_PATH: &str = "rust/.cargo/mutants.toml";
const SCOPE_PATH: &str = "rust/fslc/tests/implementation_mutation/scope.v1.json";
const EXCLUSIONS_MARKER: &str = "{{GENERATED_EXCLUDE_RE}}";
const REGENERATE_COMMAND: &str =
    "cargo run --manifest-path rust/Cargo.toml -p fslc-rust --example generate_mutants_config";

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing non-empty string '{key}'"))
}

fn read_source(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|source| source.replace("\r\n", "\n").replace('\r', "\n"))
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn generic_exclusions(root: &Path) -> Result<Vec<String>, String> {
    let scope_path = root.join(SCOPE_PATH);
    let scope: Value = serde_json::from_slice(
        &std::fs::read(&scope_path)
            .map_err(|error| format!("cannot read {}: {error}", scope_path.display()))?,
    )
    .map_err(|error| format!("cannot parse {}: {error}", scope_path.display()))?;
    let exclusions = scope
        .get("generic_exclusions")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
        .ok_or_else(|| "scope declares no generic mutation exclusions".to_owned())?;
    let mut exclusion_ids = BTreeSet::new();
    let mut rendered = Vec::new();
    for exclusion in exclusions {
        let id = required_string(exclusion, "id")?;
        if !exclusion_ids.insert(id) {
            return Err(format!("duplicate generic exclusion id '{id}'"));
        }
        let path = required_string(exclusion, "path")?;
        let function = required_string(exclusion, "function")?;
        let anchor = required_string(exclusion, "anchor")?;
        required_string(exclusion, "reason")?;
        let occurrence = exclusion
            .get("occurrence")
            .and_then(Value::as_u64)
            .filter(|occurrence| *occurrence > 0)
            .ok_or_else(|| format!("generic exclusion '{id}' has no positive occurrence"))?;
        let matching_lines = read_source(&root.join(path))?
            .lines()
            .enumerate()
            .filter_map(|(index, line)| line.contains(anchor).then_some(index + 1))
            .collect::<Vec<_>>();
        let line = matching_lines
            .get(usize::try_from(occurrence - 1).map_err(|error| error.to_string())?)
            .copied()
            .ok_or_else(|| {
                format!(
                    "generic exclusion '{id}' anchor occurrence {occurrence} is stale in {path}; update the scope anchor, then regenerate"
                )
            })?;
        let runner_path = path.strip_prefix("rust/").unwrap_or(path);
        let escaped_path = runner_path.replace('.', "\\.");
        rendered.push(format!("^{escaped_path}:{line}:.* in {function}$"));
    }
    Ok(rendered)
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render `rust/.cargo/mutants.toml` from the source template and scope manifest.
///
/// # Errors
///
/// Returns an error if the template, scope manifest, or an anchored source file
/// is unreadable, malformed, or cannot yield one requested anchor occurrence.
pub fn render(root: &Path) -> Result<String, String> {
    let template_path = root.join(TEMPLATE_PATH);
    let template = std::fs::read_to_string(&template_path)
        .map_err(|error| format!("cannot read {}: {error}", template_path.display()))?;
    if template.matches(EXCLUSIONS_MARKER).count() != 1 {
        return Err(format!(
            "{} must contain exactly one {EXCLUSIONS_MARKER}",
            template_path.display()
        ));
    }
    let exclusions = generic_exclusions(root)?
        .iter()
        .map(|exclusion| format!("  \"{}\",", toml_string(exclusion)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(template.replace(EXCLUSIONS_MARKER, &exclusions))
}

/// Fail with an actionable regeneration command when the committed config is stale.
///
/// # Errors
///
/// Returns an error if rendering fails, the committed configuration is
/// unreadable, or it differs from the rendered configuration.
pub fn check(root: &Path) -> Result<(), String> {
    let config_path = root.join(CONFIG_PATH);
    let actual = std::fs::read_to_string(&config_path)
        .map_err(|error| format!("cannot read {}: {error}", config_path.display()))?;
    let expected = render(root)?;
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "generated mutation runner configuration is stale; run `{REGENERATE_COMMAND}` to regenerate {CONFIG_PATH}\n--- {CONFIG_PATH}\n+++ generated"
    ))
}

/// Regenerate the committed cargo-mutants configuration from its source authority.
///
/// # Errors
///
/// Returns an error if rendering fails or the generated configuration cannot
/// be written.
pub fn regenerate(root: &Path) -> Result<(), String> {
    std::fs::write(root.join(CONFIG_PATH), render(root)?)
        .map_err(|error| format!("cannot write {CONFIG_PATH}: {error}"))
}

/// The command developers run to regenerate the configuration.
#[must_use]
pub const fn regenerate_command() -> &'static str {
    REGENERATE_COMMAND
}
