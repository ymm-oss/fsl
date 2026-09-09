// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native fail-closed contract for inline `implements` (#1002).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/inline_implements_fail_closed";
const CHAIN_FIXTURE: &str = "tests/fixtures/chain/requirements_broken_implements.fsl";
const BOUNDS_FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/inline_implements_bounds";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run(command: &str, arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; command={command}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// `ledger` writes Markdown, not JSON, so its exit code has to be read without
/// parsing stdout. Returns the parsed envelope only when there is one.
fn run_raw(command: &str, arguments: &[&str]) -> (Option<Value>, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    (
        serde_json::from_slice(&output.stdout).ok(),
        output.status.code().expect("native exit status"),
    )
}

/// detector (mutation C: disabling the fold leaves `check` green)
#[test]
fn check_folds_failed_inline_refinement_into_command_failure() {
    let (output, status) = run("check", &[CHAIN_FIXTURE]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed");
    assert_eq!(output["implements"]["result"], "refinement_failed");
    assert!(output["implements"]["violation"].is_object());

    let (impl_violated, impl_violated_status) =
        run("check", &[&format!("{FIXTURE_DIR}/impl_iv.fsl")]);
    assert_eq!(impl_violated_status, 1, "{impl_violated:#}");
    assert_eq!(impl_violated["result"], "impl_violated");
    assert_eq!(impl_violated["implements"]["result"], "impl_violated");
    assert_eq!(
        impl_violated["implements"]["violation"]["kind"],
        "invariant"
    );
}

/// detector (mutation: restoring the `--lemma` early return that skipped the
/// shared attachment). `run_verify_cli_from_source` returns before
/// `execute_cli_verification` when lemmas are present, so the seam has to be
/// folded on that path too; otherwise `--lemma` is a fourth way to pass a
/// broken seam with exit 0.
#[test]
fn lemma_path_cannot_bypass_failed_inline_refinement() {
    let (output, status) = run(
        "verify",
        &[
            CHAIN_FIXTURE,
            "--engine",
            "induction",
            "--lemma",
            "true",
            "--depth",
            "2",
            "--no-cache",
        ],
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed");
    assert_eq!(output["implements"]["result"], "refinement_failed");
    assert!(output["implements"]["violation"].is_object());
}

/// detector (mutation D: restoring `has_scope` suppression bypasses the seam)
#[test]
fn verify_with_bounds_cannot_bypass_failed_inline_refinement() {
    let broken = format!("{BOUNDS_FIXTURE_DIR}/impl_broken.fsl");
    let (output, status) = run(
        "verify",
        &[
            &broken,
            "--depth",
            "2",
            "--strict-tags",
            "--no-cache",
            "--instances",
            "Item=1",
        ],
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed");
    assert_eq!(output["implements"]["result"], "refinement_failed");
    assert_eq!(
        output["implements"]["violation"]["kind"],
        "stutter_changed_abs"
    );
    assert!(output.get("bounds_overrides").is_some());
}

/// detector (mutation: fold even when the primary verify verdict already failed)
#[test]
fn verify_preserves_primary_violation_when_impl_seam_also_fails() {
    let (output, status) = run(
        "verify",
        &[
            &format!("{FIXTURE_DIR}/impl_iv.fsl"),
            "--depth",
            "3",
            "--no-cache",
        ],
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["violation_kind"], "invariant");
    assert_eq!(output["trace_type"], "invariant");
    assert_eq!(output["implements"]["result"], "impl_violated");
    assert_eq!(output["implements"]["violation"]["kind"], "invariant");
    assert!(output["trace"].is_array());
    assert!(output["violated_at_step"].is_u64());
}

/// detector (mutation C, reached through the other commands that report a
/// verification verdict). The fold lives where the envelope is produced, so it
/// is not confined to `check`/`verify`. Measured against a binary built from
/// this branch's base: on `CHAIN_FIXTURE` these three commands exited 0 before
/// the fold and exit 1 after it, so this test fails if the fold is removed.
///
/// `mutate` is the one whose failure mode is a silent loss of coverage rather
/// than a wrong verdict: its baseline is no longer `verified`, so it re-emits
/// the baseline envelope and generates no mutants at all. Asserting the
/// baseline's `result` here is what makes that visible; the mutant count itself
/// is deliberately not pinned, because whether `mutate` should still explore a
/// spec whose seam is broken is a separate decision nobody has taken.
#[test]
fn other_verdict_reporting_commands_fail_closed_on_the_same_seam() {
    let (sweep, sweep_status) = run("sweep", &[CHAIN_FIXTURE]);
    assert_eq!(sweep_status, 1, "{sweep:#}");
    assert_eq!(sweep["result"], "sweep_failed");

    let (mutate, mutate_status) = run(
        "mutate",
        &[CHAIN_FIXTURE, "--depth", "3", "--max-mutants", "2"],
    );
    assert_eq!(mutate_status, 1, "{mutate:#}");
    assert_eq!(mutate["result"], "refinement_failed");
    assert_eq!(mutate["implements"]["result"], "refinement_failed");

    // Markdown on stdout; the exit code is the whole contract here.
    let (ledger, ledger_status) = run_raw("ledger", &[CHAIN_FIXTURE]);
    assert_eq!(ledger_status, 1, "ledger envelope: {ledger:?}");
}
