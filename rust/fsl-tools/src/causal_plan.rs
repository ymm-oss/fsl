// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Validation-plan artifacts for the causal portfolio ledger (issue #364).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::causal_evidence::{DESIGN_VOCABULARY, EvidenceError, LifecycleStatus, artifact_digest};

pub const PLAN_SCHEMA_VERSION: &str = "fsl-causal-validation-plan.v0";

#[derive(Clone, Debug)]
pub struct PlanArtifact {
    pub plan_id: String,
    pub claims: Vec<(String, u64)>,
    pub design: String,
    pub scope: BTreeMap<String, Vec<String>>,
    pub observation_window: Option<ObservationWindow>,
    pub measurements: Vec<String>,
    pub external_refs: Vec<Value>,
    pub declared_digest: String,
    pub lifecycle_status: LifecycleStatus,
}

#[derive(Clone, Debug)]
pub struct ObservationWindow {
    pub timebase: String,
    pub minimum: u64,
}

fn error(kind: &'static str, message: impl Into<String>) -> EvidenceError {
    EvidenceError {
        kind,
        message: message.into(),
    }
}

fn required_str<'json>(
    value: &'json Value,
    field: &str,
    context: &str,
) -> Result<&'json str, EvidenceError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        error(
            "causal_plan_schema_mismatch",
            format!("{context}: missing or non-string required field '{field}'"),
        )
    })
}

/// Parse and validate a validation-plan JSON artifact.
///
/// # Errors
///
/// Returns [`EvidenceError`] for schema/digest mismatches.
#[allow(clippy::too_many_lines)]
pub fn parse_plan(artifact: &Value) -> Result<PlanArtifact, EvidenceError> {
    let context = "validation plan";
    let schema_version = required_str(artifact, "schema_version", context)?;
    if schema_version != PLAN_SCHEMA_VERSION {
        return Err(error(
            "causal_plan_schema_mismatch",
            format!(
                "{context}: expected schema_version \"{PLAN_SCHEMA_VERSION}\", got \"{schema_version}\""
            ),
        ));
    }
    let plan_id = required_str(artifact, "plan_id", context)?.to_owned();
    let id_context = format!("plan '{plan_id}'");

    let claims_array = artifact
        .get("claims")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "causal_plan_schema_mismatch",
                format!("{id_context}: missing or non-array 'claims'"),
            )
        })?;
    if claims_array.is_empty() {
        return Err(error(
            "causal_plan_schema_mismatch",
            format!("{id_context}: claims must not be empty"),
        ));
    }
    let mut claims = Vec::new();
    for (index, entry) in claims_array.iter().enumerate() {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                error(
                    "causal_plan_schema_mismatch",
                    format!("{id_context}: claims[{index}].id must be a string"),
                )
            })?
            .to_owned();
        let version = entry
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                error(
                    "causal_plan_schema_mismatch",
                    format!("{id_context}: claims[{index}].version must be a positive integer"),
                )
            })?;
        if version == 0 {
            return Err(error(
                "causal_plan_schema_mismatch",
                format!("{id_context}: claims[{index}].version must be >= 1"),
            ));
        }
        claims.push((id, version));
    }

    let design = required_str(artifact, "design", &id_context)?.to_owned();
    if !DESIGN_VOCABULARY.contains(&design.as_str()) {
        return Err(error(
            "causal_plan_schema_mismatch",
            format!("{id_context}: unknown design '{design}'"),
        ));
    }

    let mut scope = BTreeMap::new();
    if let Some(scope_obj) = artifact.get("scope").and_then(Value::as_object) {
        for (dimension, tokens) in scope_obj {
            let tokens = tokens
                .as_array()
                .ok_or_else(|| {
                    error(
                        "causal_plan_schema_mismatch",
                        format!("{id_context}: scope.{dimension} must be an array"),
                    )
                })?
                .iter()
                .map(|token| {
                    token.as_str().map(str::to_owned).ok_or_else(|| {
                        error(
                            "causal_plan_schema_mismatch",
                            format!("{id_context}: scope.{dimension} tokens must be strings"),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            scope.insert(dimension.clone(), tokens);
        }
    }

    let observation_window = artifact
        .get("observation_window")
        .and_then(Value::as_object)
        .map(|window| {
            let timebase = window
                .get("timebase")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    error(
                        "causal_plan_schema_mismatch",
                        format!("{id_context}: observation_window.timebase must be a string"),
                    )
                })?
                .to_owned();
            let minimum = window
                .get("minimum")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    error(
                        "causal_plan_schema_mismatch",
                        format!(
                            "{id_context}: observation_window.minimum must be a positive integer"
                        ),
                    )
                })?;
            if minimum == 0 {
                return Err(error(
                    "causal_plan_schema_mismatch",
                    format!("{id_context}: observation_window.minimum must be >= 1"),
                ));
            }
            Ok(ObservationWindow { timebase, minimum })
        })
        .transpose()?;

    let measurements = artifact
        .get("measurements")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let external_refs = artifact
        .get("external_refs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let declared_digest = required_str(artifact, "artifact_digest", &id_context)?.to_owned();
    let expected = artifact_digest(artifact);
    if declared_digest != expected {
        return Err(error(
            "causal_plan_digest_mismatch",
            format!(
                "{id_context}: artifact_digest mismatch (declared {declared_digest}, computed {expected})"
            ),
        ));
    }

    Ok(PlanArtifact {
        plan_id,
        claims,
        design,
        scope,
        observation_window,
        measurements,
        external_refs,
        declared_digest,
        lifecycle_status: LifecycleStatus::Unknown,
    })
}
