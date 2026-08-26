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
    for name in ["accept_real_type", "accept_alias_condition"] {
        let path = fixture(name);
        let (value, status) = run_cli(&["check", &path]);
        assert_eq!(status, 0, "{name}: {value}");
        assert_eq!(value["result"], "ok", "{name}: {value}");
    }
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
