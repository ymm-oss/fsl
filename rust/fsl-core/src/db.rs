// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{DbColumnRef, DbEnvironmentArtifact, DbSystem};

fn safe(name: &str) -> String {
    let mut output = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        output.push('x');
    }
    if output.starts_with(|character: char| character.is_ascii_digit()) {
        output.insert(0, '_');
    }
    output
}

fn column_member(column: &DbColumnRef) -> String {
    format!("col_{}_{}", safe(&column.0), safe(&column.1))
}

fn column_label(column: &DbColumnRef) -> String {
    format!("{}.{}", column.0, column.1)
}

fn invariant_name(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| safe(part))
        .collect::<Vec<_>>()
        // `__` is reserved for composed-name display (`component__name` ->
        // `component.name`). Keep the database dialect's public double
        // underscores through parsing and restore them at presentation time.
        .join("QqDbSepqQ")
}

fn entry_name_parts(entry: &DbEnvironmentArtifact) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some((lo, hi)) = entry.schema_window {
        parts.extend(["schema".to_owned(), lo.to_string(), hi.to_string()]);
    }
    for condition in &entry.flag_conditions {
        parts.extend([
            "flag".to_owned(),
            condition.flag.clone(),
            condition.variant.clone(),
        ]);
    }
    if parts.is_empty() {
        parts.push("all".to_owned());
    }
    parts
}

fn compatibility_expression(window: (i64, i64), member: &str) -> String {
    format!(
        "not ((schema_version >= {}) and (schema_version <= {})) or column_exists[{member}]",
        window.0, window.1
    )
}

fn quote_meta(id: &str, text: &str) -> String {
    format!(
        "\"{}: {}\"",
        id.replace(['\\', '\"'], "_"),
        text.replace(['\\', '\"'], "_")
    )
}

/// Render the executable kernel used by generic check/verify for a dbsystem.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn db_kernel_source(system: &DbSystem) -> String {
    let columns = system
        .database
        .tables
        .iter()
        .flat_map(|table| table.columns.iter())
        .map(|column| ((column.table.clone(), column.name.clone()), column))
        .collect::<BTreeMap<_, _>>();
    let artifacts = system
        .artifacts
        .iter()
        .map(|artifact| (artifact.name.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let mut schema_values = vec![system.database.initial_schema];
    for migration in &system.migrations {
        schema_values.extend([migration.from_schema, migration.to_schema]);
    }
    for environment in &system.environments {
        schema_values.extend([environment.schema_window.0, environment.schema_window.1]);
        for entry in &environment.artifacts {
            if let Some((lo, hi)) = entry.schema_window {
                schema_values.extend([lo, hi]);
            }
        }
    }
    let schema_lo = schema_values.iter().min().copied().unwrap_or(0);
    let schema_hi = schema_values.iter().max().copied().unwrap_or(0);
    let rules = if system.check.rules.is_empty() {
        [
            "all_active_reads_exist",
            "all_active_writes_exist",
            "removed_only_after_unused",
            "not_null_after_backfill",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>()
    } else {
        system.check.rules.iter().map(String::as_str).collect()
    };

    let mut lines = vec![format!(
        "spec {} \"db: database multi-environment compatibility\" {{",
        system.name
    )];
    lines.push(format!("  type SchemaVersion = {schema_lo}..{schema_hi}"));
    lines.push(format!(
        "  enum Column {{ {} }}",
        columns
            .keys()
            .map(column_member)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines.push("  state {".to_owned());
    lines.push("    schema_version: SchemaVersion,".to_owned());
    lines.push("    column_exists: Map<Column, Bool>,".to_owned());
    lines.push("    column_backfilled: Map<Column, Bool>,".to_owned());
    lines.push("    column_not_null: Map<Column, Bool>".to_owned());
    lines.push("  }".to_owned());
    lines.push("  init {".to_owned());
    lines.push(format!(
        "    schema_version = {}",
        system.database.initial_schema
    ));
    for (key, column) in &columns {
        let member = column_member(key);
        lines.push(format!("    column_exists[{member}] = {}", column.present));
        lines.push(format!(
            "    column_backfilled[{member}] = {}",
            column.backfilled
        ));
        lines.push(format!(
            "    column_not_null[{member}] = {}",
            column.not_null
        ));
    }
    lines.push("  }".to_owned());

    for migration in &system.migrations {
        let action = format!("migrate_{}", safe(&migration.name));
        let metadata = quote_meta(
            "DB-MIGRATION",
            &format!(
                "{}: schema {} -> {}",
                migration.name, migration.from_schema, migration.to_schema
            ),
        );
        lines.push(format!("  action {action}() {metadata} {{"));
        lines.push(format!(
            "    requires schema_version == {}",
            migration.from_schema
        ));
        for operation in &migration.ops {
            let member = column_member(&operation.column);
            match operation.op.as_str() {
                "add" => {
                    lines.push(format!("    column_exists[{member}] = true"));
                    lines.push(format!("    column_backfilled[{member}] = false"));
                    lines.push(format!(
                        "    column_not_null[{member}] = {}",
                        operation.nullability.as_deref() == Some("not_null")
                    ));
                }
                "backfill" => {
                    lines.push(format!("    requires column_exists[{member}]"));
                    lines.push(format!("    column_backfilled[{member}] = true"));
                }
                "set_not_null" => {
                    lines.push(format!("    requires column_exists[{member}]"));
                    lines.push(format!("    requires column_backfilled[{member}]"));
                    lines.push(format!("    column_not_null[{member}] = true"));
                }
                "drop" => {
                    lines.push(format!("    column_exists[{member}] = false"));
                    lines.push(format!("    column_backfilled[{member}] = false"));
                    lines.push(format!("    column_not_null[{member}] = false"));
                }
                "rename" => {
                    if let Some(target) = operation.columns.first() {
                        let target = column_member(target);
                        lines.push(format!("    requires column_exists[{member}]"));
                        lines.push(format!("    column_exists[{member}] = false"));
                        lines.push(format!("    column_backfilled[{member}] = false"));
                        lines.push(format!("    column_not_null[{member}] = false"));
                        lines.push(format!("    column_exists[{target}] = true"));
                        lines.push(format!("    column_backfilled[{target}] = true"));
                        lines.push(format!(
                            "    column_not_null[{target}] = column_not_null[{member}]"
                        ));
                    }
                }
                "split" => {
                    lines.push(format!("    requires column_exists[{member}]"));
                    lines.push(format!("    column_exists[{member}] = false"));
                    lines.push(format!("    column_backfilled[{member}] = false"));
                    lines.push(format!("    column_not_null[{member}] = false"));
                    for target in &operation.columns {
                        let target = column_member(target);
                        lines.push(format!("    column_exists[{target}] = true"));
                        lines.push(format!("    column_backfilled[{target}] = true"));
                        lines.push(format!("    column_not_null[{target}] = false"));
                    }
                }
                "merge" => {
                    for source in &operation.columns {
                        let source = column_member(source);
                        lines.push(format!("    requires column_exists[{source}]"));
                        lines.push(format!("    column_exists[{source}] = false"));
                        lines.push(format!("    column_backfilled[{source}] = false"));
                        lines.push(format!("    column_not_null[{source}] = false"));
                    }
                    lines.push(format!("    column_exists[{member}] = true"));
                    lines.push(format!("    column_backfilled[{member}] = true"));
                    lines.push(format!("    column_not_null[{member}] = false"));
                }
                _ => {}
            }
        }
        lines.push(format!("    schema_version = {}", migration.to_schema));
        lines.push("  }".to_owned());
    }
    if system.migrations.is_empty() {
        lines.push(format!(
            "  action observe_schema_{}() \"DB-SNAPSHOT: static compatibility snapshot\" {{",
            system.database.initial_schema
        ));
        lines.push(format!(
            "    requires schema_version == {}",
            system.database.initial_schema
        ));
        lines.push(format!(
            "    schema_version = {}",
            system.database.initial_schema
        ));
        lines.push("  }".to_owned());
    }

    if rules.contains("not_null_after_backfill") {
        for key in columns.keys() {
            let member = column_member(key);
            let name = invariant_name(&["db_not_null_after_backfill", &key.0, &key.1]);
            lines.push(format!(
                "  invariant {name} {} {{ not column_not_null[{member}] or (column_exists[{member}] and column_backfilled[{member}]) }}",
                quote_meta(
                    "DB-NOT-NULL",
                    &format!(
                        "{} can be not_null only after it exists and is backfilled",
                        column_label(key)
                    )
                )
            ));
        }
    }
    for environment in &system.environments {
        for entry in &environment.artifacts {
            let Some(artifact) = artifacts.get(entry.artifact.as_str()) else {
                continue;
            };
            let window = entry.schema_window.unwrap_or(environment.schema_window);
            let name_parts = entry_name_parts(entry);
            for (capability, rule, prefix, id, verb) in [
                (
                    "reads",
                    "all_active_reads_exist",
                    "db_read",
                    "DB-COMPAT-READ",
                    "read",
                ),
                (
                    "writes",
                    "all_active_writes_exist",
                    "db_write",
                    "DB-COMPAT-WRITE",
                    "write",
                ),
            ] {
                if !rules.contains(rule) && !rules.contains("removed_only_after_unused") {
                    continue;
                }
                for column in artifact.capabilities.get(capability).into_iter().flatten() {
                    let mut parts = vec![
                        prefix.to_owned(),
                        environment.name.clone(),
                        entry.role.clone(),
                        entry.artifact.clone(),
                    ];
                    parts.extend(name_parts.clone());
                    parts.extend([column.0.clone(), column.1.clone()]);
                    let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
                    let name = invariant_name(&refs);
                    let text = format!(
                        "{} artifact {} in {} must not {} missing column {}",
                        entry.role,
                        entry.artifact,
                        environment.name,
                        verb,
                        column_label(column)
                    );
                    lines.push(format!(
                        "  invariant {name} {} {{ {} }}",
                        quote_meta(id, &text),
                        compatibility_expression(window, &column_member(column))
                    ));
                }
            }
        }
    }
    let final_schema = system
        .migrations
        .last()
        .map_or(system.database.initial_schema, |migration| {
            migration.to_schema
        });
    lines.push(format!("  terminal {{ schema_version == {final_schema} }}"));
    lines.push("}".to_owned());
    lines.join("\n")
}
