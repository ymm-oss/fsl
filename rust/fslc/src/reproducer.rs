// SPDX-License-Identifier: Apache-2.0

//! Versioned reproducer artifact export for verifier counterexamples.

use std::path::Path;

use fsl_core::{KernelModel, REPRODUCER_V1_SCHEMA_ID, REPRODUCER_V1_SCHEMA_VERSION};
use serde_json::{Map, Value, json};

/// Reproducer v1 explicitly rejects these verifier/spec shapes.
pub const REPRODUCER_V1_UNSUPPORTED: &[&str] =
    &["leadsTo", "refinement", "cti", "nondeterministic_init"];

/// Assurance boundaries recorded in every exported artifact.
pub const REPRODUCER_DO_NOT_ASSUME: &[&str] = &[
    "The repaired specification is the oracle for stage-2 reproducer testgen; this artifact records bounded failure evidence from the origin specification only.",
    "Export success does not establish implementation correctness.",
    "Canonical action and parameter projection alone does not establish compatibility with an unrelated adapter surface.",
];

/// Return a fail-closed preflight diagnostic from the root source before model load.
#[must_use]
pub fn reproducer_source_preflight_error(source: &str, engine: &str) -> Option<String> {
    if matches!(
        fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source)),
        Ok(fsl_syntax::ParsedDocument {
            surface: fsl_syntax::SurfaceDocument::Refinement(_),
            ..
        })
    ) {
        return Some("reproducer export v1 does not support refinement documents".to_owned());
    }
    if engine == "induction" {
        return Some(
            "reproducer export v1 does not support the induction engine or CTI counterexamples"
                .to_owned(),
        );
    }
    None
}

/// Return a fail-closed preflight diagnostic when the spec or engine cannot be
/// exported under reproducer v1.
#[must_use]
pub fn reproducer_preflight_error(model: &KernelModel) -> Option<String> {
    if !model.leadstos.is_empty() {
        return Some(
            "reproducer export v1 does not support specifications with leadsTo properties"
                .to_owned(),
        );
    }
    if fsl_runtime::deterministic_initial_state(model).is_err() {
        return Some(
            "reproducer export v1 does not support nondeterministic or partial initialization"
                .to_owned(),
        );
    }
    None
}

/// Return a fail-closed diagnostic when a bounded verification envelope cannot
/// be exported as reproducer v1.
#[must_use]
pub fn reproducer_verify_error(verify_output: &Value) -> Option<String> {
    match verify_output.get("result").and_then(Value::as_str) {
        Some("violated") => {}
        Some("verified" | "proved") => {
            return Some("no counterexample to export: verification succeeded".to_owned());
        }
        _ => {
            return Some(
                verify_output
                    .get("message")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || "verification did not produce a counterexample".to_owned(),
                        str::to_owned,
                    ),
            );
        }
    }
    if verify_output.get("violation_kind").and_then(Value::as_str) != Some("invariant") {
        let kind = verify_output
            .get("violation_kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Some(format!(
            "reproducer export v1 supports safety invariant violations only; got violation_kind '{kind}'"
        ));
    }
    if verify_output.get("trace_type").and_then(Value::as_str) == Some("induction_cti") {
        return Some(
            "reproducer export v1 does not support induction CTI counterexamples".to_owned(),
        );
    }
    if verify_output
        .get("trace")
        .and_then(Value::as_array)
        .is_none()
    {
        return Some("verification counterexample trace is missing".to_owned());
    }
    None
}

/// Build the closed reproducer v1 artifact from one bounded verification envelope.
///
/// # Panics
///
/// Panics if `verify_output` lacks a `trace` array after `reproducer_verify_error`
/// returned `None`.
///
/// # Errors
///
/// Returns a diagnostic when the envelope is not exportable under v1.
pub fn build_reproducer_artifact(
    path: &Path,
    spec_digest: &str,
    verify_output: &Value,
) -> Result<Value, String> {
    if let Some(error) = reproducer_verify_error(verify_output) {
        return Err(error);
    }
    let trace = verify_output
        .get("trace")
        .cloned()
        .expect("caller checked trace presence");
    let canonical_steps = canonical_steps_from_trace(&trace);
    let mut engine_metadata = Map::new();
    for key in [
        "closure",
        "states_explored",
        "max_frontier_width",
        "depth_reached",
        "action_profile",
        "cost",
    ] {
        if let Some(value) = verify_output.get(key) {
            engine_metadata.insert(key.to_owned(), value.clone());
        }
    }
    let mut verification = Map::from_iter([
        (
            "engine".to_owned(),
            verify_output
                .get("engine")
                .cloned()
                .unwrap_or_else(|| json!("bmc")),
        ),
        (
            "depth".to_owned(),
            verify_output
                .get("depth")
                .or_else(|| verify_output.get("checked_to_depth"))
                .cloned()
                .unwrap_or_else(|| json!(0)),
        ),
        (
            "completeness".to_owned(),
            verify_output
                .get("completeness")
                .cloned()
                .unwrap_or_else(|| json!("bounded")),
        ),
        (
            "checked_to_depth".to_owned(),
            verify_output
                .get("checked_to_depth")
                .cloned()
                .unwrap_or_else(|| json!(0)),
        ),
    ]);
    if !engine_metadata.is_empty() {
        verification.insert("engine_metadata".to_owned(), Value::Object(engine_metadata));
    }
    let mut violation = Map::from_iter([
        ("trace_type".to_owned(), json!("invariant")),
        ("violation_kind".to_owned(), json!("invariant")),
        (
            "property".to_owned(),
            verify_output
                .get("invariant")
                .or_else(|| verify_output.get("generated_name"))
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "violated_at_step".to_owned(),
            verify_output
                .get("violated_at_step")
                .cloned()
                .unwrap_or_else(|| json!(0)),
        ),
    ]);
    for key in ["loc", "generated_name", "origin"] {
        if let Some(value) = verify_output.get(key) {
            violation.insert(key.to_owned(), value.clone());
        }
    }
    Ok(json!({
        "$schema": REPRODUCER_V1_SCHEMA_ID,
        "schema_version": REPRODUCER_V1_SCHEMA_VERSION,
        "result": "reproducer",
        "origin": {
            "spec": verify_output.get("spec").cloned().unwrap_or(Value::Null),
            "source": path.to_string_lossy(),
            "spec_digest": spec_digest,
            "spec_digest_algorithm": "fsl-kernel-ast-v1+sha256",
        },
        "verification": Value::Object(verification),
        "violation": Value::Object(violation),
        "trace": trace,
        "canonical_steps": canonical_steps,
        "provenance": {
            "scaling_notes": []
        },
        "do_not_assume": REPRODUCER_DO_NOT_ASSUME,
        "unsupported_v1": REPRODUCER_V1_UNSUPPORTED,
    }))
}

fn canonical_steps_from_trace(trace: &Value) -> Value {
    let steps = trace
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let action = entry.get("action")?;
            Some(json!({
                "action": action.get("name")?,
                "params": action.get("params").cloned().unwrap_or_else(|| json!({})),
            }))
        })
        .collect::<Vec<_>>();
    Value::Array(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_steps_project_action_params_only() {
        let trace = json!([
            {"step": 0, "state": {"balance": 0}},
            {
                "step": 1,
                "state": {"balance": 2},
                "action": {"name": "deposit", "params": {"a": 2}}
            }
        ]);
        assert_eq!(
            canonical_steps_from_trace(&trace),
            json!([{"action": "deposit", "params": {"a": 2}}])
        );
    }
}
