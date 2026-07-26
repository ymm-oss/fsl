// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{
    DbArtifact, DbColumnRef, DbEnvironment, DbEnvironmentArtifact, DbMigration, DbMigrationOp,
    DbSystem, Span,
};
use serde_json::{Map, Value, json};

const DIALECT: &str = "fsl-db-mvp.v0";
const FINDING_SCHEMA: &str = "fsl-db-finding.v0";
const OBSERVATION_SCHEMA_VERSION: &str = "fsl-db-observation.v0";
const OBSERVATION_CAPABILITIES: &[&str] = &["reads", "writes", "calls", "requires", "provides"];

/// The default `check compatibility` rule set applied when a `dbsystem` omits
/// the block entirely (`docs/DESIGN-db.md` "Syntax"). `data_preserved` and
/// `rollback_equivalent` are deliberately excluded: they remain opt-in.
const DEFAULT_RULES: &[&str] = &[
    "all_active_reads_exist",
    "all_active_writes_exist",
    "removed_only_after_unused",
    "not_null_after_backfill",
    "destructive_operations_annotated",
    "preservation_transforms_annotated",
    "api_calls_accepted",
    "api_responses_expected",
    "offline_payloads_accepted",
    "artifact_capabilities_provided",
];

/// Rules that exist in the closed vocabulary but are never enabled by
/// default; a `dbsystem` must select them explicitly.
const OPT_IN_RULES: &[&str] = &["data_preserved", "rollback_equivalent"];

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

fn tool_error(message: String, span: Option<Span>) -> DbToolError {
    DbToolError { message, span }
}

fn reference(reference: &DbColumnRef) -> String {
    format!("{}.{}", reference.0, reference.1)
}

/// Validate cross-references in a database compatibility document.
///
/// # Errors
///
/// Returns [`DbToolError`] when a migration or environment references an
/// undeclared schema element, artifact, flag, or flag variant; when
/// `check compatibility` selects a rule outside the closed vocabulary; or
/// when a migration or environment schema window is not reachable in the
/// declared, strictly sequential migration plan.
#[allow(clippy::too_many_lines)]
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
                    return Err(tool_error(
                        format!("unknown column '{}'", reference(column)),
                        Some(operation.span),
                    ));
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
                return Err(tool_error(
                    format!("unknown artifact '{}'", artifact.artifact),
                    Some(artifact.span),
                ));
            }
            for condition in &artifact.flag_conditions {
                let Some(variants) = flags.get(condition.flag.as_str()) else {
                    return Err(tool_error(
                        format!("unknown flag '{}'", condition.flag),
                        Some(condition.span),
                    ));
                };
                if !variants.contains(&condition.variant) {
                    return Err(tool_error(
                        format!(
                            "unknown variant '{}' for flag '{}'",
                            condition.variant, condition.flag
                        ),
                        Some(condition.span),
                    ));
                }
            }
        }
    }

    // DB-01 (#490): `check compatibility` accepted any rule name, so a typo
    // silently selected nothing instead of raising an error. The closed
    // vocabulary is `DEFAULT_RULES` plus `OPT_IN_RULES`.
    for rule in &system.check.rules {
        if !DEFAULT_RULES.contains(&rule.name.as_str())
            && !OPT_IN_RULES.contains(&rule.name.as_str())
        {
            let mut allowed = DEFAULT_RULES.to_vec();
            allowed.extend_from_slice(OPT_IN_RULES);
            allowed.sort_unstable();
            return Err(tool_error(
                format!(
                    "unknown db compatibility rule '{}'; supported rules: {}",
                    rule.name,
                    allowed.join(", ")
                ),
                Some(rule.span),
            ));
        }
    }

    // DB-04 (#490): an `environment schema lo..hi` must denote schema
    // versions actually reachable in the declared migration plan
    // (`docs/DESIGN-db.md` "Compatibility Snapshot"). Migrations are a
    // single, strictly sequential rollout plan, so this also rejects a
    // migration whose `from` does not chain from the previous schema.
    let mut current_schema = system.database.initial_schema;
    let mut reached = BTreeSet::new();
    reached.insert(current_schema);
    for migration in &system.migrations {
        if migration.from_schema != current_schema {
            return Err(tool_error(
                format!(
                    "migration '{}' starts at schema {}, but the declared rollout plan is currently at {current_schema}",
                    migration.name, migration.from_schema
                ),
                Some(migration.span),
            ));
        }
        current_schema = migration.to_schema;
        reached.insert(current_schema);
    }
    for environment in &system.environments {
        for version in environment.schema_window.0..=environment.schema_window.1 {
            if !reached.contains(&version) {
                return Err(tool_error(
                    format!(
                        "environment '{}' includes schema {version}, which is not reachable in the declared migration plan",
                        environment.name
                    ),
                    Some(environment.span),
                ));
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

fn compat_repairs(capability_name: &str, artifact: &str, column: &str, environment: &str) -> Value {
    json!([
        {
            "kind": "compat_shim",
            "weakens_spec": false,
            "description": format!("keep or restore {column} until {artifact} is outside {environment}"),
        },
        {
            "kind": "rollout_window_change",
            "weakens_spec": false,
            "description": format!("narrow the {artifact} environment window before dropping {column}"),
        },
        {
            "kind": "declaration_change",
            "weakens_spec": true,
            "description": format!("remove the declared {capability_name} capability only if {artifact} truly no longer uses {column}"),
        }
    ])
}

fn destructive_repairs(column: &str) -> Value {
    json!([
        {
            "kind": "annotation_change",
            "weakens_spec": false,
            "description": format!("mark the operation as irreversible/destructive if {column} loss is intended"),
        },
        {
            "kind": "compat_shim",
            "weakens_spec": false,
            "description": format!("keep {column} or replace it with a compatibility shim before dropping it"),
        }
    ])
}

fn preservation_repairs(element: &str) -> Value {
    json!([
        {
            "kind": "preservation_mapping",
            "weakens_spec": false,
            "description": format!("provide a lossless preservation transform for {element}"),
        },
        {
            "kind": "annotation_change",
            "weakens_spec": false,
            "description": format!("mark {element} as lossy/irreversible when information loss is intended"),
        }
    ])
}

fn rollback_repairs(element: &str) -> Value {
    json!([
        {
            "kind": "rollback_contract_change",
            "weakens_spec": false,
            "description": format!("remove rollbackable from the migration unless {element} has an inverse"),
        },
        {
            "kind": "preservation_mapping",
            "weakens_spec": false,
            "description": format!("add a lossless inverse transform for {element}"),
        }
    ])
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

/// Whether `rule` is active. An empty `check compatibility` block (or its
/// absence) falls back to `DEFAULT_RULES`; `data_preserved` and
/// `rollback_equivalent` stay off unless selected explicitly (DB-10 / #491).
fn rule_enabled(system: &DbSystem, rule: &str) -> bool {
    if system.check.rules.is_empty() {
        DEFAULT_RULES.contains(&rule)
    } else {
        system.check.rules.iter().any(|item| item.name == rule)
    }
}

/// Column lifecycle state used to statically simulate a migration plan
/// (mirrors the frozen Python reference's `_apply_op`/`_static_findings`).
#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct ColumnState {
    exists: bool,
    backfilled: bool,
    not_null: bool,
}

type ColumnStates = BTreeMap<DbColumnRef, ColumnState>;

fn initial_column_states(system: &DbSystem) -> ColumnStates {
    system
        .database
        .tables
        .iter()
        .flat_map(|table| table.columns.iter())
        .map(|column| {
            (
                (column.table.clone(), column.name.clone()),
                ColumnState {
                    exists: column.present,
                    backfilled: column.backfilled,
                    not_null: column.not_null,
                },
            )
        })
        .collect()
}

fn apply_migration_op(state: &mut ColumnStates, operation: &DbMigrationOp) {
    match operation.op.as_str() {
        "add" => {
            state.insert(
                operation.column.clone(),
                ColumnState {
                    exists: true,
                    backfilled: false,
                    not_null: operation.nullability.as_deref() == Some("not_null"),
                },
            );
        }
        "backfill" => {
            if let Some(cell) = state.get_mut(&operation.column)
                && cell.exists
            {
                cell.backfilled = true;
            }
        }
        "set_not_null" => {
            if let Some(cell) = state.get_mut(&operation.column)
                && cell.exists
            {
                cell.not_null = true;
            }
        }
        "drop" => {
            state.insert(operation.column.clone(), ColumnState::default());
        }
        "rename" => {
            if let Some(target) = operation.columns.first() {
                let source = state.get(&operation.column).copied().unwrap_or_default();
                state.insert(
                    target.clone(),
                    ColumnState {
                        exists: true,
                        backfilled: source.backfilled,
                        not_null: source.not_null,
                    },
                );
                state.insert(operation.column.clone(), ColumnState::default());
            }
        }
        "split" => {
            state.insert(operation.column.clone(), ColumnState::default());
            for target in &operation.columns {
                state.insert(
                    target.clone(),
                    ColumnState {
                        exists: true,
                        backfilled: true,
                        not_null: false,
                    },
                );
            }
        }
        "merge" => {
            for source in &operation.columns {
                state.insert(source.clone(), ColumnState::default());
            }
            state.insert(
                operation.column.clone(),
                ColumnState {
                    exists: true,
                    backfilled: true,
                    not_null: false,
                },
            );
        }
        _ => {}
    }
}

fn operation_annotations<'a>(
    migration: &'a DbMigration,
    operation: &'a DbMigrationOp,
) -> BTreeSet<&'a str> {
    migration
        .annotations
        .iter()
        .chain(operation.annotations.iter())
        .map(String::as_str)
        .collect()
}

/// Whether `operation` loses information in the bounded row model: an
/// existing column dropped, or a split/merge not marked `lossless`
/// (`docs/DESIGN-db.md` "Preservation and Rollback").
fn breaks_preservation(
    operation: &DbMigrationOp,
    annotations: &BTreeSet<&str>,
    before_exists: bool,
) -> bool {
    match operation.op.as_str() {
        "drop" => before_exists,
        "split" | "merge" => !annotations.contains("lossless"),
        _ => false,
    }
}

/// Findings produced by one migration operation, evaluated against the
/// column state just before and just after it is (hypothetically) applied.
/// DB-03/DB-05/DB-06/DB-08/DB-09/DB-10 (#490, #491) are all cases where the
/// old code either encoded the forbidden state as kernel-action
/// disabledness (so the checker never saw it), checked only one of several
/// documented operations, or fired without consulting its rule gate.
#[allow(clippy::too_many_lines)]
fn migration_op_findings(
    system: &DbSystem,
    migration: &DbMigration,
    operation: &DbMigrationOp,
    before: &ColumnStates,
    after: &ColumnStates,
    assumptions: &[Value],
) -> Vec<Value> {
    let mut findings = Vec::new();
    let element = reference(&operation.column);
    let before_exists = before
        .get(&operation.column)
        .copied()
        .unwrap_or_default()
        .exists;

    // not_null_after_backfill: `add ... not_null` and `set_not_null` both
    // reach this state; DB-03 was that `set_not_null`'s violating
    // trajectory was made unreachable at the kernel level instead of
    // reported.
    if matches!(operation.op.as_str(), "add" | "set_not_null")
        && rule_enabled(system, "not_null_after_backfill")
    {
        let after_cell = after.get(&operation.column).copied().unwrap_or_default();
        if after_cell.not_null && !after_cell.backfilled {
            let mut finding = common_finding(
                "not_null_before_backfill",
                "not_null_after_backfill",
                assumptions,
            );
            finding.insert("migration".to_owned(), json!(migration.name));
            finding.insert("schema_element".to_owned(), json!(element));
            finding.insert(
                "witness".to_owned(),
                json!({"schema_version": migration.to_schema, "operation": operation.op, "column": element}),
            );
            finding.insert(
                "minimal_conflict_set".to_owned(),
                json!({"migration": migration.name, "schema_element": element}),
            );
            finding.insert(
                "repair_candidates".to_owned(),
                json!([
                    {"kind": "compat_shim", "weakens_spec": false, "description": format!("backfill {element} before setting it not_null")},
                    {"kind": "migration_change", "weakens_spec": false, "description": format!("keep {element} nullable until a later migration")},
                    {"kind": "declaration_change", "weakens_spec": true, "description": "remove the not_null marker only if the product contract truly allows nulls"}
                ]),
            );
            findings.push(Value::Object(finding));
        }
    }

    let annotations = operation_annotations(migration, operation);

    // destructive_operations_annotated: DB-08 was that `destructive` was
    // never accepted (only `irreversible` was) and that a drop of an
    // already-absent column still fired.
    if operation.op == "drop"
        && before_exists
        && rule_enabled(system, "destructive_operations_annotated")
        && !annotations.contains("destructive")
        && !annotations.contains("irreversible")
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
            destructive_repairs(&element),
        );
        findings.push(Value::Object(finding));
    }

    // preservation_transforms_annotated: DB-09 was that `irreversible` was
    // never accepted as a preservation classification (only
    // `lossless`/`lossy` were).
    if matches!(operation.op.as_str(), "split" | "merge")
        && rule_enabled(system, "preservation_transforms_annotated")
        && !annotations.contains("lossless")
        && !annotations.contains("lossy")
        && !annotations.contains("irreversible")
    {
        let mut finding = common_finding(
            "preservation_transform_unannotated",
            "preservation_transforms_annotated",
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
            preservation_repairs(&element),
        );
        findings.push(Value::Object(finding));
    }

    // data_preserved: DB-06 was that dropping an existing column never
    // produced `data_preservation_loss` (only split/merge did).
    if rule_enabled(system, "data_preserved")
        && breaks_preservation(operation, &annotations, before_exists)
    {
        let mut finding = common_finding("data_preservation_loss", "data_preserved", assumptions);
        finding.insert("migration".to_owned(), json!(migration.name));
        finding.insert("schema_element".to_owned(), json!(element));
        finding.insert(
            "minimal_conflict_set".to_owned(),
            json!({"migration": migration.name, "schema_element": element}),
        );
        finding.insert(
            "repair_candidates".to_owned(),
            preservation_repairs(&element),
        );
        findings.push(Value::Object(finding));
    }

    // rollback_equivalent: DB-05 was that `rollback_not_equivalent` only
    // ever fired for `drop`, never for a lossy rollbackable split/merge.
    if rule_enabled(system, "rollback_equivalent")
        && migration
            .annotations
            .iter()
            .any(|item| item == "rollbackable")
        && breaks_preservation(operation, &annotations, before_exists)
    {
        let mut finding = common_finding(
            "rollback_not_equivalent",
            "rollback_equivalent",
            assumptions,
        );
        finding.insert("migration".to_owned(), json!(migration.name));
        finding.insert("schema_element".to_owned(), json!(element));
        finding.insert(
            "minimal_conflict_set".to_owned(),
            json!({"migration": migration.name, "schema_element": element}),
        );
        finding.insert("repair_candidates".to_owned(), rollback_repairs(&element));
        findings.push(Value::Object(finding));
    }

    findings
}

/// The `accepts`/`responds`/`provides` capability set contributed by every
/// `active` or `supported` artifact live at `(schema, flags)`. `may_exist`
/// artifacts are never providers (`docs/DESIGN-db.md` "an active or
/// supported provider").
fn provided_capability(
    environment: &DbEnvironment,
    system: &DbSystem,
    schema: i64,
    flags: &BTreeMap<String, String>,
    capability_name: &str,
) -> BTreeSet<DbColumnRef> {
    environment
        .artifacts
        .iter()
        .filter(|entry| matches!(entry.role.as_str(), "active" | "supported"))
        .filter(|entry| {
            let range = window(entry, environment);
            range.0 <= schema && schema <= range.1 && conditions_match(entry, flags)
        })
        .filter_map(|entry| artifact_by_name(system, &entry.artifact))
        .flat_map(|artifact| capability(artifact, capability_name))
        .cloned()
        .collect()
}

#[allow(clippy::too_many_lines)]
fn findings(system: &DbSystem, assumptions: &[Value]) -> Vec<Value> {
    let mut findings = Vec::new();

    // Materialize column state after every migration (DB-11 / #490): reads
    // and writes are checked against the actual state reached at each
    // snapshot instead of "a drop operation exists somewhere in the
    // source," which both missed an initially absent column with a live
    // reader and false-fired on a column dropped then re-added later.
    let mut states: BTreeMap<i64, ColumnStates> = BTreeMap::new();
    let mut sources: BTreeMap<i64, Option<String>> = BTreeMap::new();
    let mut current_state = initial_column_states(system);
    states.insert(system.database.initial_schema, current_state.clone());
    sources.insert(system.database.initial_schema, None);

    for migration in &system.migrations {
        for operation in &migration.ops {
            let before_state = current_state.clone();
            apply_migration_op(&mut current_state, operation);
            findings.extend(migration_op_findings(
                system,
                migration,
                operation,
                &before_state,
                &current_state,
                assumptions,
            ));
        }
        states.insert(migration.to_schema, current_state.clone());
        sources.insert(migration.to_schema, Some(migration.name.clone()));
    }

    let reads_enabled = rule_enabled(system, "all_active_reads_exist")
        || rule_enabled(system, "removed_only_after_unused");
    let writes_enabled = rule_enabled(system, "all_active_writes_exist")
        || rule_enabled(system, "removed_only_after_unused");
    let calls_enabled = rule_enabled(system, "api_calls_accepted");
    let expects_enabled = rule_enabled(system, "api_responses_expected");
    let offline_enabled = rule_enabled(system, "offline_payloads_accepted");
    let requires_enabled = rule_enabled(system, "artifact_capabilities_provided");

    for environment in &system.environments {
        for schema in environment.schema_window.0..=environment.schema_window.1 {
            let state = states.get(&schema).cloned().unwrap_or_default();
            let source = sources.get(&schema).cloned().flatten();
            for flags in flag_snapshots(environment) {
                let accepts = provided_capability(environment, system, schema, &flags, "accepts");
                let responds = provided_capability(environment, system, schema, &flags, "responds");
                let provides = provided_capability(environment, system, schema, &flags, "provides");

                // Every declared entry is checked as a consumer regardless
                // of role (DB-02 / #490): an `active` artifact that itself
                // `calls`/`expects`/`emits_offline`/`requires` was
                // previously classified only as a provider and never
                // checked. `provided_capability` above already restricts
                // *providers* to active/supported (DB-07 / #491): a
                // `supported` provider's `accepts`/`responds`/`provides`
                // used to be ignored entirely.
                let active_entries = environment.artifacts.iter().filter(|entry| {
                    let range = window(entry, environment);
                    range.0 <= schema && schema <= range.1 && conditions_match(entry, &flags)
                });

                for entry in active_entries {
                    let Some(artifact) = artifact_by_name(system, &entry.artifact) else {
                        continue;
                    };

                    for (capability_name, kind, rule, gate) in [
                        (
                            "reads",
                            "column_removed_while_still_read",
                            "all_active_reads_exist",
                            reads_enabled,
                        ),
                        (
                            "writes",
                            "column_removed_while_still_written",
                            "all_active_writes_exist",
                            writes_enabled,
                        ),
                    ] {
                        if !gate {
                            continue;
                        }
                        for column in capability(artifact, capability_name) {
                            if state.get(column).copied().unwrap_or_default().exists {
                                continue;
                            }
                            let column_element = reference(column);
                            let mut finding = common_finding(kind, rule, assumptions);
                            finding.insert("environment".to_owned(), json!(environment.name));
                            finding.insert("migration".to_owned(), json!(source));
                            finding.insert("schema_element".to_owned(), json!(column_element));
                            finding.insert("artifact".to_owned(), json!(artifact.name));
                            finding.insert("artifact_version".to_owned(), json!(artifact.name));
                            finding.insert(
                                "witness".to_owned(),
                                json!({"environment_role": entry.role, "schema_version": schema, "declared_capability": capability_name}),
                            );
                            finding.insert(
                                "minimal_conflict_set".to_owned(),
                                json!({"environment": environment.name, "artifact": artifact.name, "migration": source, "schema_element": column_element}),
                            );
                            finding.insert(
                                "repair_candidates".to_owned(),
                                compat_repairs(
                                    capability_name,
                                    &artifact.name,
                                    &column_element,
                                    &environment.name,
                                ),
                            );
                            findings.push(Value::Object(finding));
                        }
                    }

                    if calls_enabled {
                        for called in capability(artifact, "calls") {
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
                            finding.insert("artifact".to_owned(), json!(artifact.name));
                            finding.insert("schema_element".to_owned(), json!(element));
                            finding.insert(
                                "witness".to_owned(),
                                json!({"schema_version": schema, "flags": flags}),
                            );
                            finding.insert(
                                "minimal_conflict_set".to_owned(),
                                json!({"environment": environment.name, "artifact": artifact.name, "schema_element": element, "flags": flags}),
                            );
                            findings.push(Value::Object(finding));
                        }
                    }

                    if expects_enabled {
                        for expected in capability(artifact, "expects") {
                            if responds.contains(expected) {
                                continue;
                            }
                            let element = reference(expected);
                            // DB-12 (#492): the documented rule for this
                            // finding is `api_responses_expected`
                            // (`docs/LANGUAGE.md`), not an undocumented
                            // `api_response_fields_available`.
                            let mut finding = common_finding(
                                "api_response_field_missing",
                                "api_responses_expected",
                                assumptions,
                            );
                            finding.insert("environment".to_owned(), json!(environment.name));
                            finding.insert("artifact".to_owned(), json!(artifact.name));
                            finding.insert("schema_element".to_owned(), json!(element));
                            finding.insert(
                                "witness".to_owned(),
                                json!({"schema_version": schema, "flags": flags}),
                            );
                            finding.insert(
                                "minimal_conflict_set".to_owned(),
                                json!({"environment": environment.name, "artifact": artifact.name, "schema_element": element, "flags": flags}),
                            );
                            findings.push(Value::Object(finding));
                        }
                    }

                    if offline_enabled {
                        for emitted in capability(artifact, "emits_offline") {
                            let ttl = artifact.offline_ttls.get(emitted).copied().unwrap_or(0);
                            // DB-14 (#490): the payload's obligation
                            // extends across its declared finite TTL
                            // window, not only the emitting schema.
                            let end = (schema + ttl).min(environment.schema_window.1);
                            let Some(failing_tick) = (schema..=end).find(|tick| {
                                !provided_capability(environment, system, *tick, &flags, "accepts")
                                    .contains(emitted)
                            }) else {
                                continue;
                            };
                            let element = reference(emitted);
                            let mut finding = common_finding(
                                "offline_payload_not_accepted",
                                "offline_payloads_accepted",
                                assumptions,
                            );
                            finding.insert("environment".to_owned(), json!(environment.name));
                            finding.insert("artifact".to_owned(), json!(artifact.name));
                            finding.insert("schema_element".to_owned(), json!(element));
                            finding.insert(
                                "witness".to_owned(),
                                json!({"schema_version": schema, "ttl_ticks": ttl, "unaccepted_schema_version": failing_tick}),
                            );
                            findings.push(Value::Object(finding));
                        }
                    }

                    if requires_enabled {
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
                            finding.insert(
                                "minimal_conflict_set".to_owned(),
                                json!({"environment": environment.name, "artifact": artifact.name, "schema_element": element}),
                            );
                            findings.push(Value::Object(finding));
                        }
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

/// Validate a runtime observation payload against
/// `schemas/fslc/db/observation.v0.schema.json` and return its event array.
///
/// # Errors
///
/// Returns [`DbToolError`] when the payload is not an array or
/// `{"events": [...]}`, when a declared `schema_version` does not equal
/// `fsl-db-observation.v0`, or when an event is not an object, is missing a
/// required field, has a wrongly typed field, or uses an unknown
/// `capability` (DB-505).
fn validate_observation_payload(payload: &Value) -> Result<&[Value], DbToolError> {
    let events = match payload {
        Value::Array(items) => items.as_slice(),
        Value::Object(fields) => {
            if let Some(version) = fields.get("schema_version") {
                match version.as_str() {
                    Some(text) if text == OBSERVATION_SCHEMA_VERSION => {}
                    Some(text) => {
                        return Err(tool_error(
                            format!(
                                "unsupported observation schema_version '{text}'; expected '{OBSERVATION_SCHEMA_VERSION}'"
                            ),
                            None,
                        ));
                    }
                    None => {
                        return Err(tool_error(
                            "observation schema_version must be a string".to_owned(),
                            None,
                        ));
                    }
                }
            }
            match fields.get("events") {
                Some(Value::Array(items)) => items.as_slice(),
                Some(_) => {
                    return Err(tool_error(
                        "observation 'events' must be an array".to_owned(),
                        None,
                    ));
                }
                None => {
                    return Err(tool_error(
                        "observation JSON must be an array or {\"events\": [...]}".to_owned(),
                        None,
                    ));
                }
            }
        }
        _ => {
            return Err(tool_error(
                "observation JSON must be an array or {\"events\": [...]}".to_owned(),
                None,
            ));
        }
    };
    for (index, event) in events.iter().enumerate() {
        validate_observation_event(index, event)?;
    }
    Ok(events)
}

fn validate_observation_event(index: usize, event: &Value) -> Result<(), DbToolError> {
    let Value::Object(fields) = event else {
        return Err(tool_error(
            format!("observation event {index} must be an object"),
            None,
        ));
    };
    for field in ["environment", "artifact", "target"] {
        match fields.get(field) {
            Some(Value::String(_)) => {}
            Some(_) => {
                return Err(tool_error(
                    format!("observation event {index} field '{field}' must be a string"),
                    None,
                ));
            }
            None => {
                return Err(tool_error(
                    format!("observation event {index} is missing required field '{field}'"),
                    None,
                ));
            }
        }
    }
    match fields.get("schema_version") {
        Some(value) if value.is_i64() || value.is_u64() => {}
        Some(_) => {
            return Err(tool_error(
                format!("observation event {index} field 'schema_version' must be an integer"),
                None,
            ));
        }
        None => {
            return Err(tool_error(
                format!("observation event {index} is missing required field 'schema_version'"),
                None,
            ));
        }
    }
    match fields.get("capability") {
        Some(Value::String(value)) if OBSERVATION_CAPABILITIES.contains(&value.as_str()) => {}
        Some(Value::String(value)) => {
            return Err(tool_error(
                format!("observation event {index} has unknown capability '{value}'"),
                None,
            ));
        }
        Some(_) => {
            return Err(tool_error(
                format!("observation event {index} field 'capability' must be a string"),
                None,
            ));
        }
        None => {
            return Err(tool_error(
                format!("observation event {index} is missing required field 'capability'"),
                None,
            ));
        }
    }
    if let Some(flags) = fields.get("flags") {
        let Value::Object(flags) = flags else {
            return Err(tool_error(
                format!("observation event {index} field 'flags' must be an object"),
                None,
            ));
        };
        for (name, value) in flags {
            if !value.is_string() {
                return Err(tool_error(
                    format!("observation event {index} flag '{name}' must be a string"),
                    None,
                ));
            }
        }
    }
    Ok(())
}

fn event_flags(event: &Value) -> BTreeMap<String, String> {
    event
        .get("flags")
        .and_then(Value::as_object)
        .map(|flags| {
            flags
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Compare runtime observation events with a database compatibility document.
///
/// # Errors
///
/// Returns [`DbToolError`] when the document contains an invalid reference,
/// or when the observation payload fails the `fsl-db-observation.v0`
/// envelope/event validation (DB-505).
#[allow(clippy::too_many_lines)]
pub fn observe_db(system: &DbSystem, payload: &Value) -> Result<Value, DbToolError> {
    validate_db(system)?;
    let events = validate_observation_payload(payload)?;
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
        // DB-506 (#506): honor the event's feature-flag snapshot instead of
        // only matching schema range, both when deciding whether the
        // observed artifact is declared in the window and when looking up
        // an accepting provider for `calls`.
        let flags = event_flags(event);
        let environment = system
            .environments
            .iter()
            .find(|item| item.name == environment_name);
        let artifact = artifact_by_name(system, artifact_name);
        let declared_in_window = environment.is_some_and(|environment| {
            environment.artifacts.iter().any(|entry| {
                entry.artifact == artifact_name && {
                    let range = window(entry, environment);
                    range.0 <= schema && schema <= range.1 && conditions_match(entry, &flags)
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
                            range.0 <= schema
                                && schema <= range.1
                                && conditions_match(entry, &flags)
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
                "flags": flags,
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
        "observation_schema_version": OBSERVATION_SCHEMA_VERSION,
        "dbsystem": system.name,
        "assumptions": observation_assumptions,
        "findings": observed,
        "formal_result": "not_run",
        "note": "runtime observation is separate from fsl-db formal compatibility verification",
    }))
}
