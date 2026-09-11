// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for #961: the "no user invariants" warning must fire for
//! specs with no safety-bearing declarations, including those that only declare
//! `reachable`, and must be suppressed only when invariant / trans / forbidden /
//! `implements` safety declarations are present.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const MESSAGE: &str = "spec declares no user invariants (only implicit type bounds are checked)";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/issue_961/{name}")
}

fn run_check(path: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", path])
        .current_dir(root())
        .output()
        .expect("run native fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; path={path}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn has_no_user_invariants_warning(output: &Value) -> bool {
    output["warnings"].as_array().is_some_and(|warnings| {
        warnings
            .iter()
            .any(|warning| warning.get("message").and_then(Value::as_str) == Some(MESSAGE))
    })
}

#[test]
fn no_safety_declarations_emits_warning() {
    let (output, status) = run_check(&fixture("no_safety.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        has_no_user_invariants_warning(&output),
        "expected warning for declaration-free spec: {output:#}"
    );
}

/// Before #961, `reachable` alone incorrectly suppressed this warning.
#[test]
fn reachable_only_still_emits_warning() {
    let (output, status) = run_check(&fixture("no_safety_reachable.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        has_no_user_invariants_warning(&output),
        "reachable must not suppress the no-user-invariants warning: {output:#}"
    );
}

#[test]
fn invariant_suppresses_warning() {
    let (output, status) = run_check(&fixture("invariant_only.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        !has_no_user_invariants_warning(&output),
        "invariant must suppress the warning: {output:#}"
    );
}

#[test]
fn trans_suppresses_warning() {
    let (output, status) = run_check(&fixture("trans_only.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        !has_no_user_invariants_warning(&output),
        "trans must suppress the warning: {output:#}"
    );
}

#[test]
fn forbidden_suppresses_warning() {
    let (output, status) = run_check(&fixture("forbidden_only.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        !has_no_user_invariants_warning(&output),
        "forbidden must suppress the warning: {output:#}"
    );
}

#[test]
fn implements_suppresses_warning() {
    let (output, status) = run_check(&fixture("implements_only.fsl"));
    assert_eq!(status, 0, "{output:#}");
    assert!(
        !has_no_user_invariants_warning(&output),
        "implements must suppress the warning: {output:#}"
    );
}
