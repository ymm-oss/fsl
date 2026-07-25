// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #507: the SQL importer did not implement
//! documented `ALTER TABLE ... DROP COLUMN`, and a malformed `CREATE TABLE`
//! could bypass the required `unsupported_sql` warning entirely.

fn warning_kinds(warnings: &[serde_json::Value]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| warning["kind"].as_str().expect("warning kind"))
        .collect()
}

// `docs/DESIGN-db.md` "Importer Boundary" documents DROP COLUMN as
// supported, but no branch ever produced a `drop` migration op for it.
#[test]
fn import_sql_supports_drop_column() {
    const SQL: &str = "CREATE TABLE t (id INT, doomed TEXT);\nALTER TABLE t DROP COLUMN doomed;";
    let imported = fsl_tools::import_db(SQL, "DropProbe", "sql");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported.source.contains("drop t.doomed irreversible;"),
        "{}",
        imported.source
    );
    // The dropped column must not remain in the final artifact's
    // reads/writes, otherwise the generated dbsystem itself would fail
    // `all_active_reads_exist`.
    let artifact = imported
        .source
        .split("artifact imported_artifact {")
        .nth(1)
        .expect("artifact block");
    assert!(!artifact.contains("doomed"), "{artifact}");
    assert!(artifact.contains("t.id"), "{artifact}");

    // The generated source must itself check cleanly.
    let system = fsl_syntax::parse_db_system(&imported.source).expect("parse generated dbsystem");
    let result = fsl_tools::check_db(&system).expect("check_db");
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

// A `CREATE TABLE` with an unbalanced/missing parenthesis used to be
// silently skipped (`continue`), producing neither a table nor a warning.
#[test]
fn import_sql_warns_on_malformed_create_table() {
    const SQL: &str = "CREATE TABLE good (id INT);\nCREATE TABLE broken (id INT;";
    let imported = fsl_tools::import_db(SQL, "BrokenProbe", "sql");
    assert_eq!(warning_kinds(&imported.warnings), ["unsupported_sql"]);
    assert!(
        imported.source.contains("table good"),
        "{}",
        imported.source
    );
    assert!(!imported.source.contains("broken"), "{}", imported.source);
}

// Control: well-formed `CREATE TABLE`/`ADD COLUMN`/`RENAME COLUMN` must
// still import cleanly with no warnings (no regression from the DROP
// COLUMN and malformed-CREATE-TABLE fixes).
#[test]
fn import_sql_well_formed_statements_still_import_cleanly() {
    const SQL: &str = "CREATE TABLE users (id INT NOT NULL);\nALTER TABLE users ADD COLUMN nickname TEXT;\nALTER TABLE users RENAME COLUMN nickname TO display_name;";
    let imported = fsl_tools::import_db(SQL, "RenameProbe", "sql");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported.source.contains("display_name"),
        "{}",
        imported.source
    );
}
