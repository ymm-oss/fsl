// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use fsl_core::{
    FsResolver, PublicKernelVersion, build_model, parse_kernel_source_with_file,
    public_kernel_contract_for_version,
};
use fsl_syntax::{ActionItem, Expr, LValue, SpecItem, Statement};

const SAFE: &str =
    include_str!("../../../examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl");
const UNSAFE: &str = include_str!("../../../examples/db/unsafe_not_null_before_backfill.fsl");
const RENAME: &str = include_str!("../../../examples/db/safe_rename_preservation.fsl");
const SPLIT: &str = include_str!("../../../examples/db/unsafe_lossy_split_preservation.fsl");
const MERGE: &str = include_str!("../../../examples/db/unsafe_lossy_merge_preservation.fsl");
const SAFE_PATH: &str = "examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl";
const UNSAFE_PATH: &str = "examples/db/unsafe_not_null_before_backfill.fsl";

fn run_cli(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn expr_signature(expr: &Expr) -> String {
    match expr {
        Expr::Num(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::Var(name) => name.clone(),
        Expr::Index(base, index) => {
            format!("{}[{}]", expr_signature(base), expr_signature(index))
        }
        Expr::Binary { op, left, right } => {
            format!("({} {op} {})", expr_signature(left), expr_signature(right))
        }
        other => panic!("unexpected DB expression: {other:?}"),
    }
}

fn lvalue_signature(target: &LValue) -> String {
    match target {
        LValue::Var(name) => name.clone(),
        LValue::Index(name, index) => format!("{name}[{}]", expr_signature(index)),
        other @ LValue::Field(..) => panic!("unexpected DB lvalue: {other:?}"),
    }
}

fn item_signature(item: &ActionItem) -> String {
    match item {
        ActionItem::Requires(expr, _) => format!("requires {}", expr_signature(expr)),
        ActionItem::Statement(Statement::Assign { target, value, .. }) => {
            format!("{} = {}", lvalue_signature(target), expr_signature(value))
        }
        other => panic!("unexpected DB action item: {other:?}"),
    }
}

#[test]
fn direct_lowering_preserves_catalog_shape_and_source_order() {
    let kernel = parse_kernel_source_with_file(SAFE, &FsResolver::new("."), SAFE_PATH)
        .expect("lower dbsystem directly");
    let syntax = kernel.syntax();

    let actions = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            SpecItem::Action { name, items, .. } => Some((name.as_str(), items.len())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        [
            ("migrate_add_display_name", 5),
            ("migrate_backfill_display_name", 4),
            ("migrate_require_display_name", 5),
            ("migrate_drop_legacy_name", 5),
        ]
    );

    let init = syntax
        .items
        .iter()
        .find_map(|item| match item {
            SpecItem::Init { statements, .. } => Some(statements),
            _ => None,
        })
        .expect("generated init");
    assert_eq!(init.len(), 10);

    let add = syntax
        .items
        .iter()
        .find_map(|item| match item {
            SpecItem::Action { name, items, .. } if name == "migrate_add_display_name" => {
                Some(items)
            }
            _ => None,
        })
        .expect("add migration");
    assert!(matches!(add.first(), Some(ActionItem::Requires(..))));
    assert!(matches!(add.last(), Some(ActionItem::Statement(..))));

    let read_origin = kernel
        .origins()
        .targets()
        .find(|(target, _)| target.starts_with("property:invariant:db_readQqDbSepqQ"))
        .and_then(|(_, origins)| origins.first())
        .expect("read compatibility origin");
    let secondary_names = read_origin
        .secondary
        .iter()
        .filter_map(|site| site.declaration_path.last().map(String::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(secondary_names.contains("all_active_reads_exist"));
    assert!(secondary_names.contains("removed_only_after_unused"));

    let terminal = syntax.items.last().expect("terminal item");
    assert!(matches!(terminal, SpecItem::Terminal { .. }));
    build_model(kernel).expect("directly lowered catalog builds a checked model");
}

#[test]
fn direct_lowering_preserves_structural_transform_shapes() {
    for (source, path, expected_name, expected) in [
        (
            RENAME,
            "examples/db/safe_rename_preservation.fsl",
            "migrate_rename_legacy_name",
            &[
                "requires (schema_version == 0)",
                "requires column_exists[col_users_legacy_name]",
                "column_exists[col_users_legacy_name] = false",
                "column_backfilled[col_users_legacy_name] = false",
                "column_not_null[col_users_legacy_name] = false",
                "column_exists[col_users_display_name] = true",
                "column_backfilled[col_users_display_name] = true",
                "column_not_null[col_users_display_name] = column_not_null[col_users_legacy_name]",
                "schema_version = 1",
            ][..],
        ),
        (
            SPLIT,
            "examples/db/unsafe_lossy_split_preservation.fsl",
            "migrate_split_full_name",
            &[
                "requires (schema_version == 0)",
                "requires column_exists[col_users_full_name]",
                "column_exists[col_users_full_name] = false",
                "column_backfilled[col_users_full_name] = false",
                "column_not_null[col_users_full_name] = false",
                "column_exists[col_users_first_name] = true",
                "column_backfilled[col_users_first_name] = true",
                "column_not_null[col_users_first_name] = false",
                "column_exists[col_users_last_name] = true",
                "column_backfilled[col_users_last_name] = true",
                "column_not_null[col_users_last_name] = false",
                "schema_version = 1",
            ][..],
        ),
        (
            MERGE,
            "examples/db/unsafe_lossy_merge_preservation.fsl",
            "migrate_merge_name",
            &[
                "requires (schema_version == 0)",
                "requires column_exists[col_users_first_name]",
                "column_exists[col_users_first_name] = false",
                "column_backfilled[col_users_first_name] = false",
                "column_not_null[col_users_first_name] = false",
                "requires column_exists[col_users_last_name]",
                "column_exists[col_users_last_name] = false",
                "column_backfilled[col_users_last_name] = false",
                "column_not_null[col_users_last_name] = false",
                "column_exists[col_users_display_name] = true",
                "column_backfilled[col_users_display_name] = true",
                "column_not_null[col_users_display_name] = false",
                "schema_version = 1",
            ][..],
        ),
    ] {
        let kernel = parse_kernel_source_with_file(source, &FsResolver::new("."), path)
            .expect("lower structural migration directly");
        let actions = kernel
            .syntax()
            .items
            .iter()
            .filter_map(|item| match item {
                SpecItem::Action { name, items, .. } => Some((name.as_str(), items)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actions.len(), 1, "{path}");
        assert_eq!(actions[0].0, expected_name, "{path}");
        assert_eq!(
            actions[0].1.iter().map(item_signature).collect::<Vec<_>>(),
            expected,
            "{path}"
        );
    }
}

#[test]
fn direct_lowering_publishes_authored_db_origins() {
    let kernel = parse_kernel_source_with_file(UNSAFE, &FsResolver::new("."), UNSAFE_PATH)
        .expect("lower dbsystem directly");
    let model = build_model(kernel.clone()).expect("build dbsystem model");
    let contract = public_kernel_contract_for_version(
        &kernel,
        &model,
        UNSAFE_PATH,
        "db",
        PublicKernelVersion::V2,
    )
    .expect("publish v2");

    let action_origin = kernel
        .origins()
        .primary_for("action:migrate_add_required_email")
        .expect("migration action origin");
    let action_site = action_origin
        .primary
        .as_ref()
        .expect("migration source site");
    assert_eq!(action_site.source_file.as_deref(), Some(UNSAFE_PATH));
    assert_eq!(action_site.span.expect("migration span").start.line, 10);
    assert!(action_origin.generated);

    let property_target =
        "property:invariant:db_not_null_after_backfillQqDbSepqQusersQqDbSepqQemail";
    let property_origin = kernel
        .origins()
        .primary_for(property_target)
        .expect("not-null property origin");
    assert_eq!(
        property_origin
            .primary
            .as_ref()
            .and_then(|site| site.span)
            .expect("rule span")
            .start
            .line,
        28
    );
    assert!(property_origin.secondary.iter().any(|site| {
        site.span
            .is_some_and(|span| span.start.line == 6 && span.start.column == 7)
    }));

    let origins = contract["provenance"]["origins"]
        .as_array()
        .expect("published origins");
    assert!(origins.iter().any(|origin| {
        origin["assurance"] == "generated_from_source"
            && origin["primary"]["source"]["value"] == UNSAFE_PATH
    }));
}

#[test]
fn direct_lowering_negative_control_rejects_not_null_before_backfill() {
    let (value, status) = run_cli(&["verify", UNSAFE_PATH, "--depth", "2", "--no-cache"]);

    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    assert_eq!(value["violation_kind"], "invariant");
    assert_eq!(
        value["invariant"],
        "db_not_null_after_backfill__users__email"
    );
    assert_eq!(value["requirement"]["id"], "DB-NOT-NULL");
    assert_eq!(value["loc"], serde_json::json!({"line": 28, "column": 5}));
    assert_eq!(
        value["last_action"]["loc"],
        serde_json::json!({"line": 10, "column": 3})
    );
    assert_eq!(value["last_action"]["name"], "migrate_add_required_email");
    assert_eq!(
        value["trace"][1]["action"]["name"],
        "migrate_add_required_email"
    );
    assert_eq!(value["violated_at_step"], 1);
}

#[test]
fn direct_lowering_keeps_explain_names_replayable() {
    let (value, status) = run_cli(&["explain", UNSAFE_PATH, "--depth", "2"]);

    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "explained");
    let action_names = value["skeleton"]["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .map(|action| action["name"].as_str().expect("action name"))
        .collect::<Vec<_>>();
    assert_eq!(action_names, ["migrate_add_required_email"]);
    let property_names = value["skeleton"]["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|property| property["name"].as_str().expect("property name"))
        .collect::<Vec<_>>();
    assert!(property_names.contains(&"db_not_null_after_backfill__users__email"));
    assert_eq!(
        property_names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        property_names.len(),
        "display names must remain unique"
    );
}
