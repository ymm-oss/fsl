// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #515: `fslc domain check` must fold a nested
//! kernel's `violated` (or any other non-`verified`/`proved` result) into
//! the top-level verdict, exit code, and evidence — instead of hardcoding
//! `result:"verified_under_assumptions"`/`formal_result:"verified"`/exit 0
//! regardless of what the embedded `kernel.result` actually says.

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

const BROKEN: &str = "rust/fslc/tests/fixtures/issue_515_domain_broken_invariant.fsl";
const CLEAN: &str = "rust/fslc/tests/fixtures/issue_515_domain_clean_invariant.fsl";

/// The headline false green: a domain whose aggregate invariant the kernel
/// finds violated must return `result:"violated"`/exit 1 from `domain
/// check`, not `verified_under_assumptions`/exit 0. Verified against the
/// same fixture's `verify` result so the two never disagree.
#[test]
fn domain_check_folds_a_violated_kernel_into_the_top_level_verdict() {
    // sanity: the fixture's raw kernel path really is violated.
    let (verify, verify_status) = run(&["verify", BROKEN, "--depth", "4"]);
    assert_eq!(verify_status, 1, "{verify:#}");
    assert_eq!(verify["result"], "violated");

    let (check, status) = run(&["domain", "check", BROKEN, "--depth", "4"]);
    assert_eq!(status, 1, "{check:#}");
    assert_eq!(check["result"], "violated");
    assert_eq!(check["formal_result"], "violated");
    assert_eq!(check["kernel"]["result"], "violated");
    assert_eq!(check["kernel"]["invariant"], "neverClosed");
}

/// The fix must survive an engine switch: `--engine induction` reaches the
/// same `check_domain` call with a differently-shaped kernel result, and
/// must not regress to the old hardcoded green.
#[test]
fn domain_check_folds_a_violated_kernel_under_induction_too() {
    let (check, status) = run(&["domain", "check", BROKEN, "--engine", "induction"]);
    assert_eq!(status, 1, "{check:#}");
    assert_eq!(check["result"], "violated");
    assert_eq!(check["kernel"]["result"], "violated");
}

/// Replayable evidence (AGENTS.md: "Do not allowlist verdict, location,
/// assurance, or exit-code differences") must survive the stable-kernel
/// projection on the violated path, not just the bare verdict string.
#[test]
fn domain_check_preserves_violation_evidence_in_the_nested_kernel() {
    let (check, _status) = run(&["domain", "check", BROKEN, "--depth", "4"]);
    let kernel = &check["kernel"];
    assert!(kernel["loc"].is_object(), "{check:#}");
    assert!(kernel["violated_at_step"].is_u64(), "{check:#}");
    assert!(kernel["blame"].is_object(), "{check:#}");
    assert!(kernel["last_action"].is_object(), "{check:#}");
    assert!(
        kernel["trace"].as_array().is_some_and(|t| !t.is_empty()),
        "{check:#}"
    );
}

/// Regression control (both directions of the negative control, per
/// AGENTS.md's bug-fixing altitude guidance): a genuinely provable
/// aggregate invariant must still return `verified_under_assumptions`/exit
/// 0, so folding the kernel verdict through does not over-trigger.
#[test]
fn domain_check_still_reports_verified_for_a_genuinely_clean_domain() {
    let (check, status) = run(&["domain", "check", CLEAN, "--depth", "4"]);
    assert_eq!(status, 0, "{check:#}");
    assert_eq!(check["result"], "verified_under_assumptions");
    assert_eq!(check["formal_result"], "verified");
    assert_eq!(check["kernel"]["result"], "verified");
}
