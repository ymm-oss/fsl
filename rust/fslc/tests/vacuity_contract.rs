// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #465: native `--vacuity` must select over the full
//! documented 7-kind lane set (`docs/LANGUAGE.md` §15,
//! `fsl_core::VACUITY_KINDS` -- `vacuity_probe_truncated` joined the other
//! six in issue #729), not just the two kinds spelled `vacuous_*`.
//! This file exercises the `vacuous_leadsto` lane end to end through the CLI
//! (`warn`/`error`/`ignore`) — the lane that was entirely missing from
//! native `verification_warnings` before the fix.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (serde_json::Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

const FIXTURE: &str = "rust/fslc/tests/fixtures/vacuous_leadsto.fsl";

#[test]
fn vacuous_leadsto_warns_by_default() {
    let (output, status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
    let warnings = output["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning["kind"] == "vacuous_leadsto"),
        "expected a vacuous_leadsto warning: {output:#}"
    );
}

/// Before #465, `vacuous_leadsto` was entirely unimplemented in native
/// `verification_warnings`, so `--vacuity error` had nothing to select and
/// this hollow leadsTo passed with `result:"verified"`/exit 0. If this
/// regresses, the assertions below fail.
#[test]
fn vacuous_leadsto_fails_closed_under_vacuity_error() {
    let (output, status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "vacuous_leadsto");
    assert_eq!(output["trace_type"], "vacuity");
}

#[test]
fn vacuous_leadsto_is_suppressed_under_vacuity_ignore() {
    let (output, status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--vacuity",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
    let warnings = output["warnings"].as_array().expect("warnings array");
    assert!(
        !warnings
            .iter()
            .any(|warning| warning["kind"] == "vacuous_leadsto"),
        "vacuous_leadsto must be suppressed: {output:#}"
    );
}

/// Regression control: a leadsTo whose trigger is genuinely reachable must
/// not be flagged, so `--vacuity error` still passes an ordinary spec.
#[test]
fn a_reachable_leadsto_trigger_is_not_flagged() {
    let (output, status) = run_cli(&[
        "verify",
        "specs/mutex_queue.fsl",
        "--depth",
        "6",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_ne!(output["result"], "error");
}
