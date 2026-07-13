// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

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

#[test]
fn native_cli_checks_a_repository_spec_without_python() {
    let (value, status) = run_cli(&["check", "specs/cart_v1.fsl"]);
    assert_eq!(status, 0);
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "ok");
    assert_eq!(value["spec"], "ShoppingCart");
}

#[test]
fn native_cli_preserves_bmc_outcomes() {
    let (verified, status) = run_cli(&[
        "verify",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(verified["result"], "verified");
    assert_eq!(verified["completeness"], "bounded");

    let (violated, status) = run_cli(&[
        "verify",
        "specs/cart_buggy.fsl",
        "--depth",
        "5",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["violation_kind"], "invariant");
    assert_eq!(violated["invariant"], "NoNegativeStock");
}

#[test]
fn native_cli_preserves_induction_outcomes() {
    let (proved, status) = run_cli(&[
        "verify",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(proved["result"], "proved");
    assert_eq!(proved["engine"], "induction");
    assert_eq!(proved["completeness"], "unbounded");

    let (cti, status) = run_cli(&[
        "verify",
        "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(cti["result"], "unknown_cti");
    assert_eq!(cti["invariant"], "Sync");
    assert_eq!(cti["completeness"], "bounded");
}
