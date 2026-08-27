// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::process::Command;

use serde_json::{Value, json};

const FIXTURES: &str = "rust/fslc/tests/fixtures/issue_832";

fn run_cli(args: &[&str]) -> (Value, i32) {
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

fn fixture(name: &str) -> String {
    format!("{FIXTURES}/{name}.fsl")
}

#[test]
fn compose_core_error_origin_preserves_the_existing_message_shape() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let path = root.join(fixture("shape_a"));
    let source = std::fs::read_to_string(&path).expect("shape A source");
    let resolver = fsl_core::FsResolver::new(path.parent().expect("fixture directory"));
    let error = fsl_core::parse_kernel_source_with_file(
        &source,
        &resolver,
        path.strip_prefix(root)
            .expect("relative fixture path")
            .display()
            .to_string(),
    )
    .expect_err("shape A must fail");
    let diagnostic = fslc_rust::spec_load::SemanticDiagnostic::from_core_error(&error);

    assert_eq!(diagnostic.message, "unknown type 'core.NoSuchType' at 7:5");
    assert_eq!(diagnostic.loc, Some(json!({"line": 7, "column": 5})));
}

fn assert_rejected_by_check_and_verify(name: &str, kind: &str, message: &str, loc: &Value) {
    let path = fixture(name);
    for args in [
        vec!["check", path.as_str()],
        vec!["verify", path.as_str(), "--depth", "1", "--no-cache"],
    ] {
        let command = args[0];
        let (value, status) = run_cli(&args);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(value["result"], "error", "{command}: {value}");
        assert_eq!(value["kind"], kind, "{command}: {value}");
        assert_eq!(value["message"], message, "{command}: {value}");
        assert_eq!(&value["loc"], loc, "{command}: {value}");
    }
}

/// Rejecting detector A: a declared alias must not authorize an absent type.
#[test]
fn check_rejects_unknown_member_of_declared_alias() {
    assert_rejected_by_check_and_verify(
        "shape_a",
        "type",
        "unknown type 'core.NoSuchType' at 7:5",
        &json!({"line": 7, "column": 5}),
    );
}

/// Rejecting detector for the expression-binder path: an invariant's typed
/// `forall` reports its authored location instead of the `(1, 1)` placeholder.
#[test]
fn check_rejects_unknown_member_in_invariant_binder_at_authored_location() {
    assert_rejected_by_check_and_verify(
        "expr_binder",
        "type",
        "unknown type 'core.NoSuchType' at 4:5",
        &json!({"line": 4, "column": 5}),
    );
}

/// Rejecting detector for #831's remaining reachable fallback: an action
/// expression binder has its own source span and must not fall back to `1:1`.
/// The alias is deliberately on line 8, so a placeholder cannot pass.
#[test]
fn check_rejects_unknown_action_binder_alias_at_authored_location() {
    assert_rejected_by_check_and_verify(
        "action_expr_binder",
        "semantics",
        "unknown alias 'nonexistent' at 8:5",
        &json!({"line": 8, "column": 5}),
    );
}

/// The sync-action reference producer must carry the same real source span
/// into the envelope, rather than retaining it only in the message.
#[test]
fn check_rejects_unknown_sync_action_alias_at_authored_location() {
    assert_rejected_by_check_and_verify(
        "sync_unknown_alias",
        "semantics",
        "unknown alias 'nonexistent' at 8:3",
        &json!({"line": 8, "column": 3}),
    );
}

/// Rejecting detector for the shared typed-binder gate: requirements process
/// lowering must not let an unknown invariant binder pass `check` and fail only
/// when `verify` tries to enumerate its finite domain. The binder is authored
/// on line 8, where the invariant declaration owns the property diagnostic.
#[test]
fn check_rejects_unknown_requirements_binder_at_authored_property_location() {
    assert_rejected_by_check_and_verify(
        "requirements_missing_binder",
        "semantics",
        "invalid model expression: unknown type 'Missing' at 8:3",
        &json!({"line": 8, "column": 3}),
    );
}

/// Rejecting detector B: `init if` conditions receive the same name/type gate
/// as assignment right-hand sides.
#[test]
fn check_rejects_undeclared_alias_shaped_init_condition() {
    assert_rejected_by_check_and_verify(
        "shape_b",
        "semantics",
        "invalid init statement: public Kernel cannot type identifier 'nonexistent' at 7:5",
        &json!({"line": 7, "column": 5}),
    );
}

/// Accepting controls for both corrected positions.
#[test]
fn check_accepts_real_alias_type_and_declared_alias_condition() {
    for name in [
        "accept_real_type",
        "accept_alias_condition",
        "accept_action_expr_binder",
    ] {
        let path = fixture(name);
        let (value, status) = run_cli(&["check", &path]);
        assert_eq!(status, 0, "{name}: {value}");
        assert_eq!(value["result"], "ok", "{name}: {value}");
    }
}

/// Accepting control for the shared gate: a process entity is a real lowered
/// type and remains valid as an invariant binder.
#[test]
fn check_accepts_real_requirements_process_binder() {
    let path = fixture("requirements_real_binder");
    let (value, status) = run_cli(&["check", &path]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok", "{value}");
}

/// Preservation control: the pre-existing assignment-RHS rejection remains
/// active; this is not a detector for either new fix.
#[test]
fn assignment_rhs_rejection_is_preserved() {
    assert_rejected_by_check_and_verify(
        "rhs_control",
        "semantics",
        "invalid init statement: public Kernel cannot type identifier 'nonexistent' at 7:5",
        &json!({"line": 7, "column": 5}),
    );
}

/// Negative location control: CLI scope overrides own no source construct, so
/// their placeholder `(1, 1)` must never be promoted into a public `loc`.
#[test]
fn sweep_scope_error_does_not_fabricate_source_location() {
    let (value, status) = run_cli(&[
        "sweep",
        "examples/e2e/3_design.fsl",
        "--instances",
        "Typo=1..1",
        "--depth",
        "1..1",
    ]);

    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "semantics", "{value}");
    assert_eq!(
        value["message"], "verify instances references undeclared entity 'Typo' at 1:1",
        "{value}"
    );
    assert!(value.get("loc").is_none(), "fabricated loc: {value}");
}
