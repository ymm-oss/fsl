// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use serde_json::{Value, json};

type ImportedColumn = (String, String, bool);
type ImportedTable = (String, Vec<ImportedColumn>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbImport {
    pub source_format: String,
    pub source: String,
    pub warnings: Vec<Value>,
}

fn clean(value: &str) -> &str {
    value.trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '_')
}

fn render_catalog(name: &str, tables: &[ImportedTable]) -> String {
    let mut output = format!("dbsystem {name} {{\n  database imported {{\n    schema 0\n");
    let mut references = Vec::new();
    for (table, raw_columns) in tables {
        let _ = writeln!(output, "    table {table} {{");
        let mut columns = raw_columns.clone();
        columns.sort_by(|left, right| left.0.cmp(&right.0));
        for (column, kind, not_null) in &columns {
            let nullable = if *not_null { "not_null" } else { "nullable" };
            let _ = writeln!(
                output,
                "      column {column}: {kind} present backfilled {nullable};"
            );
            references.push(format!("{table}.{column}"));
        }
        output.push_str("    }\n");
    }
    references.sort();
    let refs = references.join(", ");
    let _ = write!(
        output,
        "  }}\n\n  artifact imported_artifact {{\n    reads {refs};\n    writes {refs};\n  }}\n\n  environment imported {{\n    schema 0..0;\n    active imported_artifact when schema 0..0;\n    supported imported_artifact when schema 0..0;\n  }}\n\n  check compatibility {{\n    rule all_active_reads_exist;\n    rule all_active_writes_exist;\n    rule removed_only_after_unused;\n    rule not_null_after_backfill;\n  }}\n}}\n"
    );
    output
}

#[derive(Clone, Debug)]
struct SqlColumn {
    table: String,
    name: String,
    kind: String,
    present: bool,
    backfilled: bool,
    not_null: bool,
}

#[allow(clippy::too_many_lines)]
fn import_sql(text: &str, name: &str) -> DbImport {
    let mut columns = Vec::new();
    let mut migrations = Vec::new();
    let mut warnings = Vec::new();
    for statement in text
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let words = statement.split_whitespace().collect::<Vec<_>>();
        let upper = statement.to_ascii_uppercase();
        if upper.starts_with("CREATE TABLE") {
            let Some(open) = statement.find('(') else {
                continue;
            };
            let Some(close) = statement.rfind(')') else {
                continue;
            };
            let table = clean(statement["CREATE TABLE".len()..open].trim()).to_owned();
            for definition in statement[open + 1..close].split(',') {
                let parts = definition.split_whitespace().collect::<Vec<_>>();
                if parts.len() < 2
                    || matches!(
                        parts[0].to_ascii_uppercase().as_str(),
                        "PRIMARY" | "FOREIGN" | "UNIQUE" | "CONSTRAINT"
                    )
                {
                    continue;
                }
                columns.push(SqlColumn {
                    table: table.clone(),
                    name: clean(parts[0]).to_owned(),
                    kind: parts[1].to_owned(),
                    present: true,
                    backfilled: true,
                    not_null: definition.to_ascii_uppercase().contains("NOT NULL"),
                });
            }
        } else if words.len() >= 7
            && upper.starts_with("ALTER TABLE")
            && words[3].eq_ignore_ascii_case("ADD")
            && words[4].eq_ignore_ascii_case("COLUMN")
        {
            let table = clean(words[2]).to_owned();
            let column = clean(words[5]).to_owned();
            columns.push(SqlColumn {
                table: table.clone(),
                name: column.clone(),
                kind: words[6].to_owned(),
                present: false,
                backfilled: false,
                not_null: upper.contains("NOT NULL"),
            });
            let index = migrations.len() + 1;
            let nullable = if upper.contains("NOT NULL") {
                "not_null"
            } else {
                "nullable"
            };
            migrations.push(format!("\n  migration m{index}_add_column from {} to {index} {{\n    add {table}.{column} {nullable};\n  }}\n",index-1));
        } else if words.len() >= 5
            && upper.starts_with("UPDATE")
            && words[2].eq_ignore_ascii_case("SET")
        {
            let table = clean(words[1]);
            let column = clean(words[3]);
            let index = migrations.len() + 1;
            migrations.push(format!("\n  migration m{index}_backfill from {} to {index} {{\n    backfill {table}.{column};\n  }}\n",index-1));
        } else if words.len() >= 8
            && upper.starts_with("ALTER TABLE")
            && words[3].eq_ignore_ascii_case("RENAME")
            && words[4].eq_ignore_ascii_case("COLUMN")
            && words[6].eq_ignore_ascii_case("TO")
        {
            let table = clean(words[2]).to_owned();
            let old = clean(words[5]).to_owned();
            let new = clean(words[7]).to_owned();
            let kind = columns
                .iter()
                .find(|item| item.table == table && item.name == old)
                .map_or_else(|| "Value".to_owned(), |item| item.kind.clone());
            columns.push(SqlColumn {
                table: table.clone(),
                name: new.clone(),
                kind,
                present: false,
                backfilled: false,
                not_null: false,
            });
            let index = migrations.len() + 1;
            migrations.push(format!("\n  migration m{index}_rename_column from {} to {index} {{\n    rename {table}.{old} to {table}.{new};\n  }}\n",index-1));
        } else {
            warnings.push(json!({"kind":"unsupported_sql","statement":statement,"message":"SQL importer supports CREATE TABLE, ALTER TABLE ADD/DROP/RENAME COLUMN, and UPDATE ... SET for backfill"}));
        }
    }
    if columns.is_empty() {
        columns.push(SqlColumn {
            table: "imported".to_owned(),
            name: "id".to_owned(),
            kind: "Int".to_owned(),
            present: true,
            backfilled: true,
            not_null: true,
        });
        warnings.push(json!({"kind":"empty_import","message":"no supported table definition was found; emitted a placeholder schema"}));
    }
    let mut output = format!("dbsystem {name} {{\n  database imported {{\n    schema 0\n");
    let mut table_names = columns
        .iter()
        .map(|item| item.table.as_str())
        .collect::<Vec<_>>();
    table_names.sort_unstable();
    table_names.dedup();
    for table in table_names {
        let _ = writeln!(output, "    table {table} {{");
        let mut items = columns
            .iter()
            .filter(|item| item.table == table)
            .collect::<Vec<_>>();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        for item in items {
            let presence = if item.present { "present" } else { "absent" };
            let backfill = if item.backfilled { " backfilled" } else { "" };
            let nullable = if item.not_null {
                "not_null"
            } else {
                "nullable"
            };
            let _ = writeln!(
                output,
                "      column {}: {} {presence}{backfill} {nullable};",
                item.name, item.kind
            );
        }
        output.push_str("    }\n");
    }
    output.push_str("  }\n");
    for migration in &migrations {
        output.push_str(migration);
    }
    let mut final_refs = columns
        .iter()
        .filter(|item| item.present)
        .map(|item| format!("{}.{}", item.table, item.name))
        .collect::<Vec<_>>();
    for migration in &migrations {
        if let Some(rename) = migration
            .split("    rename ")
            .nth(1)
            .and_then(|item| item.split(';').next())
            && let Some((old, new)) = rename.split_once(" to ")
        {
            final_refs.retain(|item| item != old);
            final_refs.push(new.to_owned());
        }
        if let Some(add) = migration
            .split("    add ")
            .nth(1)
            .and_then(|item| item.split_whitespace().next())
        {
            final_refs.push(add.trim_end_matches(';').to_owned());
        }
    }
    final_refs.sort();
    final_refs.dedup();
    let refs = final_refs.join(", ");
    let schema = migrations.len();
    let _ = write!(
        output,
        "\n  artifact imported_artifact {{\n    reads {refs};\n    writes {refs};\n  }}\n\n  environment imported {{\n    schema 0..{schema};\n    active imported_artifact when schema {schema}..{schema};\n    supported imported_artifact when schema {schema}..{schema};\n  }}\n\n  check compatibility {{\n    rule all_active_reads_exist;\n    rule all_active_writes_exist;\n    rule removed_only_after_unused;\n    rule not_null_after_backfill;\n  }}\n}}\n"
    );
    DbImport {
        source_format: "sql-ddl-minimal.v0".to_owned(),
        source: output,
        warnings,
    }
}

fn import_prisma(text: &str, name: &str) -> DbImport {
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if !line.starts_with("model ") {
            continue;
        }
        let table = clean(
            line.trim_start_matches("model ")
                .split_whitespace()
                .next()
                .unwrap_or("imported"),
        )
        .to_owned();
        let mut fields = Vec::new();
        for field in lines.by_ref() {
            let field = field.trim();
            if field.starts_with('}') {
                break;
            }
            if field.is_empty() || field.starts_with("//") {
                continue;
            }
            let parts = field.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 2
                || parts[1].ends_with("[]")
                || field.contains("@relation")
                || field.starts_with("@@")
            {
                warnings.push(json!({"kind":"unsupported_prisma","statement":field,"message":"relation, model attribute, or malformed field is not imported"}));
                continue;
            }
            let raw = parts[1];
            let kind = match raw.trim_end_matches('?') {
                "Int" | "BigInt" => "Int",
                "String" | "DateTime" => "Text",
                "Boolean" => "Bool",
                "Float" | "Decimal" | "Json" | "Bytes" => "Value",
                _ => {
                    warnings.push(json!({"kind":"unsupported_prisma","statement":field,"message":"unsupported Prisma scalar type"}));
                    continue;
                }
            };
            fields.push((parts[0].to_owned(), kind.to_owned(), !raw.ends_with('?')));
        }
        if !fields.is_empty() {
            tables.push((table, fields));
        }
    }
    if tables.is_empty() {
        tables.push((
            "imported".to_owned(),
            vec![("id".to_owned(), "Int".to_owned(), true)],
        ));
        warnings.push(json!({"kind":"empty_import","message":"no supported Prisma model fields were found; emitted a placeholder schema"}));
    }
    DbImport {
        source_format: "prisma-schema-minimal.v0".to_owned(),
        source: render_catalog(name, &tables),
        warnings,
    }
}

/// Import a minimal SQL DDL or Prisma schema into the fsl-db source form.
#[must_use]
pub fn import_db(text: &str, name: &str, source_format: &str) -> DbImport {
    if source_format == "prisma" {
        import_prisma(text, name)
    } else {
        import_sql(text, name)
    }
}
