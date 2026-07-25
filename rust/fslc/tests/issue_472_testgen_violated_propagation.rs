// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #472: `testgen` (and `domain testgen`) must
//! propagate a genuine `violated`/`reachable_failed` counterexample from
//! the underlying `scenarios` machinery verbatim — verdict, exit code, and
//! trace — instead of re-wrapping it as an unrelated exit-2
//! `kind:"semantics"` generic error when `fsl_tools::validate_scenarios`
//! finds no `scenarios` array in what is actually a `verify`-shaped
//! envelope.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn scratch_output(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("fslc-issue-472-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir.join(name).display().to_string()
}

/// A committed corpus spec (`specs/cart_buggy.fsl`) with a genuine
/// `invariant` violation must produce `result:"violated"`/exit 1 with a
/// full trace from `testgen`, matching `verify`/`scenarios` on the same
/// spec — not exit 2 `kind:"semantics"` with the trace destroyed.
#[test]
fn testgen_propagates_a_violated_invariant_instead_of_a_generic_error() {
    let output_path = scratch_output("cart_buggy.py");
    let (testgen, status) = run(&[
        "testgen",
        "specs/cart_buggy.fsl",
        "--depth",
        "6",
        "-o",
        &output_path,
    ]);
    assert_eq!(status, 1, "{testgen:#}");
    assert_eq!(testgen["result"], "violated");
    assert_eq!(testgen["violation_kind"], "invariant");
    assert!(
        testgen["trace"]
            .as_array()
            .is_some_and(|trace| !trace.is_empty()),
        "trace must survive: {testgen:#}"
    );

    let (verify, verify_status) = run(["verify", "specs/cart_buggy.fsl", "--depth", "6"].as_ref());
    assert_eq!(verify_status, status);
    assert_eq!(verify["invariant"], testgen["invariant"]);
    assert_eq!(verify["violated_at_step"], testgen["violated_at_step"]);
}

/// A `leadsTo` violation must also propagate as `result:"violated"`/exit 1
/// with `violation_kind:"leadsTo"`, not a generic exit-2 error.
#[test]
fn testgen_propagates_a_leadsto_violation_instead_of_a_generic_error() {
    let path = "rust/fslc/tests/fixtures/testgen_leadsto_violation.fsl";
    let output_path = scratch_output("leadsto.py");
    let (testgen, status) = run(&["testgen", path, "--depth", "6", "-o", &output_path]);
    assert_eq!(status, 1, "{testgen:#}");
    assert_eq!(testgen["result"], "violated");
    assert_eq!(testgen["violation_kind"], "leadsTo");
}

/// `--strict` must abort as `reachable_failed`/exit 1 with
/// `unreached[].classification`, not a generic exit-2 error — and the
/// non-strict default (partial generation) on the same fixture must stay
/// exit 0, so this fix does not over-trigger on the lenient path.
#[test]
fn testgen_strict_propagates_reachable_failed_and_non_strict_stays_lenient() {
    let path = "rust/fslc/tests/fixtures/testgen_strict_unreached.fsl";

    let strict_output = scratch_output("strict.py");
    let (strict, strict_status) = run(&[
        "testgen",
        path,
        "--depth",
        "1",
        "--strict",
        "-o",
        &strict_output,
    ]);
    assert_eq!(strict_status, 1, "{strict:#}");
    assert_eq!(strict["result"], "reachable_failed");
    assert!(
        strict["unreached"]
            .as_array()
            .is_some_and(|unreached| !unreached.is_empty()),
        "{strict:#}"
    );

    let lenient_output = scratch_output("lenient.py");
    let (lenient, lenient_status) = run(&["testgen", path, "--depth", "1", "-o", &lenient_output]);
    assert_eq!(lenient_status, 0, "{lenient:#}");
    assert_ne!(lenient["result"], "reachable_failed");
}

/// `domain testgen` must inherit the same propagation from the generic
/// testgen path it wraps.
#[test]
fn domain_testgen_propagates_a_violated_result_from_the_generic_path() {
    let path = "rust/fslc/tests/fixtures/domain_origin_violation.fsl";
    let output_path = scratch_output("domain_violated.py");
    let (output, status) = run(&[
        "domain",
        "testgen",
        path,
        "--depth",
        "6",
        "-o",
        &output_path,
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated");
    assert_ne!(output["kind"], "semantics");
}

/// Regression control: an ordinary passing spec must keep returning
/// `result:"generated"`/exit 0 with real generated content, so the fix
/// does not over-trigger on the success path.
#[test]
fn testgen_still_generates_for_a_clean_spec() {
    let output_path = scratch_output("clean.py");
    let (output, status) = run(&[
        "testgen",
        "specs/cart_v1.fsl",
        "--depth",
        "4",
        "-o",
        &output_path,
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "generated");
}
