// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{DbArtifact, DbColumnRef, DbEnvironment, DbEnvironmentArtifact, DbSystem, Span};
use serde_json::{Map, Value, json};

const DIALECT: &str = "fsl-db-mvp.v0";
const FINDING_SCHEMA: &str = "fsl-db-finding.v0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbToolError {
    pub message: String,
    pub span: Option<Span>,
}

impl fmt::Display for DbToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DbToolError {}

fn reference(reference: &DbColumnRef) -> String {
    format!("{}.{}", reference.0, reference.1)
}

/// Validate cross-references in a database compatibility document.
///
/// # Errors
///
/// Returns [`DbToolError`] when a migration or environment references an
/// undeclared schema element, artifact, flag, or flag variant.
pub fn validate_db(system: &DbSystem) -> Result<(), DbToolError> {
    let columns = system
        .database
        .tables
        .iter()
        .flat_map(|table| table.columns.iter())
        .map(|column| (column.table.clone(), column.name.clone()))
        .collect::<BTreeSet<_>>();
    for migration in &system.migrations {
        for operation in &migration.ops {
            for column in std::iter::once(&operation.column).chain(operation.columns.iter()) {
                if !columns.contains(column) {
                    return Err(DbToolError {
                        message: format!("unknown column '{}'", reference(column)),
                        span: Some(operation.span),
                    });
                }
            }
        }
    }
    let artifacts = system
        .artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect::<BTreeSet<_>>();
    for environment in &system.environments {
        let flags = environment
            .flags
            .iter()
            .map(|flag| {
                (
                    flag.name.as_str(),
                    flag.variants.iter().collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for artifact in &environment.artifacts {
            if !artifacts.contains(artifact.artifact.as_str()) {
                return Err(DbToolError {
                    message: format!("unknown artifact '{}'", artifact.artifact),
                    span: Some(artifact.span),
                });
            }
            for condition in &artifact.flag_conditions {
                let Some(variants) = flags.get(condition.flag.as_str()) else {
                    return Err(DbToolError {
                        message: format!("unknown flag '{}'", condition.flag),
                        span: Some(condition.span),
                    });
                };
                if !variants.contains(&condition.variant) {
                    return Err(DbToolError {
                        message: format!(
                            "unknown variant '{}' for flag '{}'",
                            condition.variant, condition.flag
                        ),
                        span: Some(condition.span),
                    });
                }
            }
        }
    }
    Ok(())
}

fn assumption(id: &str) -> Value {
    let text = match id {
        "DB-ASSUME-ROLLING-SNAPSHOT" => {
            "environment schema ranges denote finite snapshots reachable in the declared migration order; percentages are modeled only as coexistence windows"
        }
        "DB-ASSUME-CAPABILITY-DECLARATIONS" => {
            "artifact capability declarations are complete for the checked compatibility window"
        }
        "DB-ASSUME-BOUNDED-ROW-MODEL" => {
            "data preservation and rollback are checked over a finite representative row model"
        }
        "DB-ASSUME-OFFLINE-TTL-FINITE" => {
            "offline payload compatibility is bounded by the declared finite TTL"
        }
        "DB-ASSUME-FINITE-FLAG-STATE" => {
            "feature flags range over their finite declared variants at a snapshot"
        }
        "DB-ASSUME-AI-CAPABILITY-PROFILES" => {
            "AI tool, retriever, and output capabilities are complete declarations"
        }
        _ => "external runtime evidence is complete for the observed window",
    };
    json!({"id": id, "text": text})
}

fn assumptions(system: &DbSystem) -> Vec<Value> {
    let mut ids = vec![
        "DB-ASSUME-ROLLING-SNAPSHOT",
        "DB-ASSUME-CAPABILITY-DECLARATIONS",
    ];
    if system.migrations.iter().any(|migration| {
        migration
            .annotations
            .iter()
            .any(|item| item == "rollbackable")
            || migration
                .ops
                .iter()
                .any(|operation| matches!(operation.op.as_str(), "split" | "merge" | "rename"))
    }) {
        ids.push("DB-ASSUME-BOUNDED-ROW-MODEL");
    }
    if system
        .artifacts
        .iter()
        .any(|artifact| !artifact.offline_ttls.is_empty())
    {
        ids.push("DB-ASSUME-OFFLINE-TTL-FINITE");
    }
    if system
        .environments
        .iter()
        .any(|environment| !environment.flags.is_empty())
    {
        ids.push("DB-ASSUME-FINITE-FLAG-STATE");
    }
    if system.artifacts.iter().any(|artifact| {
        artifact
            .capabilities
            .get("requires")
            .is_some_and(|items| !items.is_empty())
            || artifact
                .capabilities
                .get("provides")
                .is_some_and(|items| !items.is_empty())
    }) {
        ids.push("DB-ASSUME-AI-CAPABILITY-PROFILES");
    }
    ids.into_iter().map(assumption).collect()
}

fn common_finding(kind: &str, failed_rule: &str, assumptions: &[Value]) -> Map<String, Value> {
    let mut finding = Map::new();
    finding.insert("schema_version".to_owned(), json!(FINDING_SCHEMA));
    finding.insert("fsl".to_owned(), json!(DIALECT));
    finding.insert("result".to_owned(), json!("violated"));
    finding.insert("kind".to_owned(), json!(kind));
    finding.insert("severity".to_owned(), json!("error"));
    finding.insert("environment".to_owned(), Value::Null);
    finding.insert("migration".to_owned(), Value::Null);
    finding.insert("schema_element".to_owned(), Value::Null);
    finding.insert("artifact".to_owned(), Value::Null);
    finding.insert("artifact_version".to_owned(), Value::Null);
    finding.insert("failed_rule".to_owned(), json!(failed_rule));
    finding.insert("witness".to_owned(), json!({}));
    finding.insert("minimal_conflict_set".to_owned(), json!({}));
    finding.insert("repair_candidates".to_owned(), json!([]));
    finding.insert("assumptions".to_owned(), Value::Array(assumptions.to_vec()));
    finding.insert(
        "redaction".to_owned(),
        json!({"policy": "schema identifiers only; row values, SQL literals, and secrets are not emitted"}),
    );
    finding
}

fn capability<'a>(artifact: &'a DbArtifact, name: &str) -> &'a [DbColumnRef] {
    artifact.capabilities.get(name).map_or(&[], Vec::as_slice)
}

fn window(entry: &DbEnvironmentArtifact, environment: &DbEnvironment) -> (i64, i64) {
    entry.schema_window.unwrap_or(environment.schema_window)
}

fn conditions_match(entry: &DbEnvironmentArtifact, flags: &BTreeMap<String, String>) -> bool {
    entry.flag_conditions.iter().all(|condition| {
        flags
            .get(&condition.flag)
            .is_some_and(|variant| variant == &condition.variant)
    })
}

fn flag_snapshots(environment: &DbEnvironment) -> Vec<BTreeMap<String, String>> {
    let mut snapshots = vec![BTreeMap::new()];
    for flag in &environment.flags {
        snapshots = snapshots
            .into_iter()
            .flat_map(|snapshot| {
                flag.variants.iter().map(move |variant| {
                    let mut next = snapshot.clone();
                    next.insert(flag.name.clone(), variant.clone());
                    next
                })
            })
            .collect();
    }
    snapshots
}

fn artifact_by_name<'a>(system: &'a DbSystem, name: &str) -> Option<&'a DbArtifact> {
    system
        .artifacts
        .iter()
        .find(|artifact| artifact.name == name)
}

fn rule_enabled(system: &DbSystem, rule: &str) -> bool {
    system.check.rules.is_empty() || system.check.rules.iter().any(|item| item.name == rule)
}

#[allow(clippy::too_many_lines)]
fn findings(system: &DbSystem, assumptions: &[Value]) -> Vec<Value> {
    let mut findings = Vec::new();
    for migration in &system.migrations {
        for operation in &migration.ops {
            let element = reference(&operation.column);
            if operation.op == "add" && operation.nullability.as_deref() == Some("not_null") {
                let mut finding = common_finding(
                    "not_null_before_backfill",
                    "not_null_after_backfill",
                    assumptions,
                );
                finding.insert("migration".to_owned(), json!(migration.name));
                finding.insert("schema_element".to_owned(), json!(element));
                finding.insert("witness".to_owned(), json!({"schema_version": migration.to_schema, "operation": "add", "column": element}));
                finding.insert(
                    "minimal_conflict_set".to_owned(),
                    json!({"migration": migration.name, "schema_element": element}),
                );
                finding.insert("repair_candidates".to_owned(), json!([
                    {"kind": "compat_shim", "weakens_spec": false, "description": format!("backfill {element} before setting it not_null")},
                    {"kind": "migration_change", "weakens_spec": false, "description": format!("keep {element} nullable until a later migration")},
                    {"kind": "declaration_change", "weakens_spec": true, "description": "remove the not_null marker only if the product contract truly allows nulls"}
                ]));
                findings.push(Value::Object(finding));
            }
            if rule_enabled(system, "destructive_operations_annotated")
                && operation.op == "drop"
                && !operation
                    .annotations
                    .iter()
                    .any(|item| item == "irreversible")
            {
                let mut finding = common_finding(
                    "destructive_migration_unannotated",
                    "destructive_operations_annotated",
                    assumptions,
                );
                finding.insert("migration".to_owned(), json!(migration.name));
                finding.insert("schema_element".to_owned(), json!(element));
                finding.insert(
                    "minimal_conflict_set".to_owned(),
                    json!({"migration": migration.name, "schema_element": element}),
                );
                finding.insert(
                    "repair_candidates".to_owned(),
                    json!([
                        {"kind": "annotation_change", "weakens_spec": false},
                        {"kind": "compat_shim", "weakens_spec": false}
                    ]),
                );
                findings.push(Value::Object(finding));
            }
            if matches!(operation.op.as_str(), "split" | "merge") {
                let (kind, rule) = if operation.annotations.iter().any(|item| item == "lossy") {
                    ("data_preservation_loss", "data_preserved")
                } else if !operation.annotations.iter().any(|item| item == "lossless") {
                    (
                        "preservation_transform_unannotated",
                        "preservation_transforms_annotated",
                    )
                } else {
                    continue;
                };
                let mut finding = common_finding(kind, rule, assumptions);
                finding.insert("migration".to_owned(), json!(migration.name));
                finding.insert("schema_element".to_owned(), json!(element));
                findings.push(Value::Object(finding));
            }
            if migration
                .annotations
                .iter()
                .any(|item| item == "rollbackable")
                && operation.op == "drop"
            {
                let mut finding = common_finding(
                    "rollback_not_equivalent",
                    "rollback_equivalent",
                    assumptions,
                );
                finding.insert("migration".to_owned(), json!(migration.name));
                finding.insert("schema_element".to_owned(), json!(element));
                findings.push(Value::Object(finding));
            }
            if operation.op == "drop" {
                for environment in &system.environments {
                    for entry in &environment.artifacts {
                        let Some(artifact) = artifact_by_name(system, &entry.artifact) else {
                            continue;
                        };
                        let range = window(entry, environment);
                        if range.1 < migration.to_schema
                            || !capability(artifact, "reads").contains(&operation.column)
                        {
                            continue;
                        }
                        let mut finding = common_finding(
                            "column_removed_while_still_read",
                            "all_active_reads_exist",
                            assumptions,
                        );
                        finding.insert("environment".to_owned(), json!(environment.name));
                        finding.insert("migration".to_owned(), json!(migration.name));
                        finding.insert("schema_element".to_owned(), json!(element));
                        finding.insert("artifact".to_owned(), json!(artifact.name));
                        finding.insert("witness".to_owned(), json!({"environment_role": entry.role, "schema_version": migration.to_schema}));
                        finding.insert("minimal_conflict_set".to_owned(), json!({"environment": environment.name, "artifact": artifact.name, "migration": migration.name, "schema_element": element}));
                        findings.push(Value::Object(finding));
                    }
                }
            }
        }
    }

    for environment in &system.environments {
        for schema in environment.schema_window.0..=environment.schema_window.1 {
            for flags in flag_snapshots(environment) {
                let active = environment
                    .artifacts
                    .iter()
                    .filter(|entry| {
                        entry.role == "active" && {
                            let range = window(entry, environment);
                            range.0 <= schema
                                && schema <= range.1
                                && conditions_match(entry, &flags)
                        }
                    })
                    .filter_map(|entry| artifact_by_name(system, &entry.artifact))
                    .collect::<Vec<_>>();
                let consumers = environment
                    .artifacts
                    .iter()
                    .filter(|entry| {
                        entry.role != "active" && {
                            let range = window(entry, environment);
                            range.0 <= schema
                                && schema <= range.1
                                && conditions_match(entry, &flags)
                        }
                    })
                    .filter_map(|entry| artifact_by_name(system, &entry.artifact))
                    .collect::<Vec<_>>();
                let responses = active
                    .iter()
                    .flat_map(|artifact| capability(artifact, "responds"))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let accepts = active
                    .iter()
                    .flat_map(|artifact| capability(artifact, "accepts"))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let provides = active
                    .iter()
                    .flat_map(|artifact| capability(artifact, "provides"))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                for consumer in &consumers {
                    for called in capability(consumer, "calls") {
                        if accepts.contains(called) {
                            continue;
                        }
                        let element = reference(called);
                        let mut finding = common_finding(
                            "api_call_not_accepted",
                            "api_calls_accepted",
                            assumptions,
                        );
                        finding.insert("environment".to_owned(), json!(environment.name));
                        finding.insert("artifact".to_owned(), json!(consumer.name));
                        finding.insert("schema_element".to_owned(), json!(element));
                        finding.insert(
                            "witness".to_owned(),
                            json!({"schema_version": schema, "flags": flags}),
                        );
                        finding.insert(
                            "minimal_conflict_set".to_owned(),
                            json!({"environment": environment.name, "artifact": consumer.name, "schema_element": element, "flags": flags}),
                        );
                        findings.push(Value::Object(finding));
                    }
                    for expected in capability(consumer, "expects") {
                        if responses.contains(expected) {
                            continue;
                        }
                        let element = reference(expected);
                        let mut finding = common_finding(
                            "api_response_field_missing",
                            "api_response_fields_available",
                            assumptions,
                        );
                        finding.insert("environment".to_owned(), json!(environment.name));
                        finding.insert("artifact".to_owned(), json!(consumer.name));
                        finding.insert("schema_element".to_owned(), json!(element));
                        finding.insert(
                            "witness".to_owned(),
                            json!({"schema_version": schema, "flags": flags}),
                        );
                        finding.insert("minimal_conflict_set".to_owned(), json!({"environment": environment.name, "artifact": consumer.name, "schema_element": element, "flags": flags}));
                        findings.push(Value::Object(finding));
                    }
                    for emitted in capability(consumer, "emits_offline") {
                        if accepts.contains(emitted) {
                            continue;
                        }
                        let element = reference(emitted);
                        let ttl = consumer.offline_ttls.get(emitted).copied().unwrap_or(0);
                        let mut finding = common_finding(
                            "offline_payload_not_accepted",
                            "offline_payloads_accepted",
                            assumptions,
                        );
                        finding.insert("environment".to_owned(), json!(environment.name));
                        finding.insert("artifact".to_owned(), json!(consumer.name));
                        finding.insert("schema_element".to_owned(), json!(element));
                        finding.insert(
                            "witness".to_owned(),
                            json!({"schema_version": schema, "ttl_ticks": ttl}),
                        );
                        findings.push(Value::Object(finding));
                    }
                }
                for artifact in active.iter().chain(consumers.iter()) {
                    for required in capability(artifact, "requires") {
                        if provides.contains(required) {
                            continue;
                        }
                        let element = reference(required);
                        let mut finding = common_finding(
                            "required_capability_missing",
                            "artifact_capabilities_provided",
                            assumptions,
                        );
                        finding.insert("environment".to_owned(), json!(environment.name));
                        finding.insert("artifact".to_owned(), json!(artifact.name));
                        finding.insert("schema_element".to_owned(), json!(element));
                        finding.insert(
                            "witness".to_owned(),
                            json!({"declared_capability": "requires", "schema_version": schema}),
                        );
                        finding.insert("minimal_conflict_set".to_owned(), json!({"environment": environment.name, "artifact": artifact.name, "schema_element": element}));
                        findings.push(Value::Object(finding));
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    findings.retain(|finding| {
        let key = (
            finding["kind"].as_str().unwrap_or_default().to_owned(),
            finding["environment"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            finding["migration"].as_str().unwrap_or_default().to_owned(),
            finding["artifact"].as_str().unwrap_or_default().to_owned(),
            finding["schema_element"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        );
        seen.insert(key)
    });
    findings
}

/// Produce the solver-independent database findings envelope payload.
///
/// # Errors
///
/// Returns [`DbToolError`] when the document contains an invalid reference.
pub fn check_db(system: &DbSystem) -> Result<Value, DbToolError> {
    validate_db(system)?;
    let assumptions = assumptions(system);
    let findings = findings(system, &assumptions);
    Ok(json!({
        "result": if findings.is_empty() { "verified_under_assumptions" } else { "violated" },
        "dialect": DIALECT,
        "finding_schema_version": FINDING_SCHEMA,
        "dbsystem": system.name,
        "assumptions": assumptions,
        "findings": findings,
    }))
}

/// Compare runtime observation events with a database compatibility document.
///
/// # Errors
///
/// Returns [`DbToolError`] when the event payload is not an array or an object
/// containing an `events` array.
#[allow(clippy::too_many_lines)]
pub fn observe_db(system: &DbSystem, payload: &Value) -> Result<Value, DbToolError> {
    validate_db(system)?;
    let events = payload
        .as_array()
        .or_else(|| payload.get("events").and_then(Value::as_array))
        .ok_or_else(|| DbToolError {
            message: "observation JSON must be an array or {\"events\": [...]}".to_owned(),
            span: None,
        })?;
    let mut observation_assumptions = assumptions(system);
    observation_assumptions.push(json!({
        "id": "DB-ASSUME-OBSERVABILITY-COVERAGE",
        "text": "runtime observation is evidence only; absence from logs is not a proof that a capability is unused or unsupported"
    }));
    let mut observed = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let environment_name = event
            .get("environment")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let artifact_name = event
            .get("artifact")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let capability_name = event
            .get("capability")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let target = event
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let schema = event
            .get("schema_version")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let environment = system
            .environments
            .iter()
            .find(|item| item.name == environment_name);
        let artifact = artifact_by_name(system, artifact_name);
        let declared_in_window = environment.is_some_and(|environment| {
            environment.artifacts.iter().any(|entry| {
                entry.artifact == artifact_name && {
                    let range = window(entry, environment);
                    range.0 <= schema && schema <= range.1
                }
            })
        });
        let target_ref = target.split_once('.').map_or_else(
            || ("unknown".to_owned(), target.to_owned()),
            |(left, right)| (left.to_owned(), right.to_owned()),
        );
        let (kind, reason) = if artifact.is_none() || !declared_in_window {
            (
                "unsupported_artifact_observed",
                "observed artifact is not declared in the environment/schema window",
            )
        } else if matches!(
            capability_name,
            "reads" | "writes" | "requires" | "provides"
        ) && !artifact
            .is_some_and(|artifact| capability(artifact, capability_name).contains(&target_ref))
        {
            (
                "declared_unused_but_observed",
                "observed DB access is not declared as an artifact capability",
            )
        } else if capability_name == "calls"
            && !environment.is_some_and(|environment| {
                environment.artifacts.iter().any(|entry| {
                    entry.role != "may_exist"
                        && {
                            let range = window(entry, environment);
                            range.0 <= schema && schema <= range.1
                        }
                        && artifact_by_name(system, &entry.artifact).is_some_and(|provider| {
                            capability(provider, "accepts").contains(&target_ref)
                        })
                })
            })
        {
            (
                "legacy_api_still_called",
                "observed API call is not accepted by an active/supported artifact",
            )
        } else {
            continue;
        };
        let mut finding = common_finding(kind, "runtime_observation", &observation_assumptions);
        finding.insert("result".to_owned(), json!("observed_mismatch"));
        finding.insert("environment".to_owned(), json!(environment_name));
        finding.insert("schema_element".to_owned(), json!(target));
        finding.insert("artifact".to_owned(), json!(artifact_name));
        finding.insert("artifact_version".to_owned(), json!(artifact_name));
        finding.insert(
            "witness".to_owned(),
            json!({
                "event_index": index,
                "schema_version": schema,
                "capability": capability_name,
                "target": target,
                "reason": reason,
            }),
        );
        finding.insert(
            "minimal_conflict_set".to_owned(),
            json!({"environment": environment_name, "artifact": artifact_name, "schema_element": target}),
        );
        finding.insert(
            "repair_candidates".to_owned(),
            json!([
                {"kind": "declaration_change", "weakens_spec": false, "description": format!("declare the observed {capability_name} capability for {artifact_name} on {target}")},
                {"kind": "rollout_window_change", "weakens_spec": false, "description": format!("keep {artifact_name} in the environment window until observations stop")},
                {"kind": "compat_shim", "weakens_spec": false, "description": format!("restore compatibility for observed use of {target}")}
            ]),
        );
        observed.push(Value::Object(finding));
    }
    Ok(json!({
        "result": if observed.is_empty() { "observed_conformant" } else { "observed_mismatch" },
        "dialect": DIALECT,
        "finding_schema_version": FINDING_SCHEMA,
        "observation_schema_version": "fsl-db-observation.v0",
        "dbsystem": system.name,
        "assumptions": observation_assumptions,
        "findings": observed,
        "formal_result": "not_run",
        "note": "runtime observation is separate from fsl-db formal compatibility verification",
    }))
}
