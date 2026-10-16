// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1275: `preserve progress` inside an inline `implements` block is rejected.
//!
//! The inline seam checks safety refinement only, so it used to accept the
//! block and report `implements.result: "refines"` (check exit 0, verify
//! `verified`) for a mapping whose progress obligation the standalone
//! `fslc refine` reports as `progress_lost`. Every command that loads the
//! document must now reject it at the `preserve` keyword, and the migration
//! target (the same mapping in a standalone refinement file) must keep
//! checking progress.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/inline_implements_preserve_progress";
const INLINE: &str = "req_inline_progress.fsl";
/// `preserve` keyword inside the `implements` block of [`INLINE`].
const PRESERVE_LINE: u64 = 13;
const PRESERVE_COLUMN: u64 = 5;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .join(FIXTURE_DIR)
}

fn run(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(fixture_dir())
        .output()
        .expect("run native fslc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `chain` prints a table before its JSON report.
    let json = stdout
        .find("\n{")
        .map_or(stdout.as_ref(), |index| &stdout[index + 1..]);
    let value = serde_json::from_str(json).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stdout={stdout}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_rejected(envelope: &Value) {
    assert_eq!(envelope["result"], "error", "{envelope:#}");
    assert_eq!(envelope["kind"], "parse", "{envelope:#}");
    assert_eq!(envelope["loc"]["line"], PRESERVE_LINE, "{envelope:#}");
    assert_eq!(envelope["loc"]["column"], PRESERVE_COLUMN, "{envelope:#}");
    let message = envelope["message"].as_str().expect("error message");
    assert!(
        message.contains("preserve progress") && message.contains("fslc refine"),
        "message must name the construct and the standalone route: {message}"
    );
    assert!(
        envelope.get("implements").is_none(),
        "no seam verdict may be reported: {envelope:#}"
    );
}

#[test]
fn check_rejects_inline_preserve_progress() {
    let (output, status) = run(&["check", INLINE]);
    assert_eq!(status, 2, "{output:#}");
    assert_rejected(&output);
}

#[test]
fn verify_rejects_inline_preserve_progress() {
    let (output, status) = run(&[
        "verify",
        INLINE,
        "--depth",
        "8",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_rejected(&output);
}

#[test]
fn scenarios_rejects_inline_preserve_progress() {
    let (output, status) = run(&["scenarios", INLINE, "--depth", "4"]);
    assert_eq!(status, 2, "{output:#}");
    assert_rejected(&output);
}

#[test]
fn chain_layer_fails_on_inline_preserve_progress() {
    let (output, status) = run(&["chain", "fsl-project.toml"]);
    assert_ne!(status, 0, "{output:#}");
    assert_ne!(output["result"], "verified", "{output:#}");
    let layer = &output["layers"][0];
    assert_eq!(layer["layer"], "requirements", "{output:#}");
    assert_eq!(layer["status"], "failed", "{output:#}");
    assert_rejected(&layer["detail"]);
}

/// The LSP publishes `source_diagnostic::diagnostics_with_model`.
#[test]
fn source_diagnostics_reject_inline_preserve_progress() {
    let directory = fixture_dir();
    let path = directory.join(INLINE);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let diagnostics = fslc_rust::source_diagnostic::diagnostics(
        &source,
        path.to_str().expect("UTF-8 fixture path"),
        &fsl_core::FsResolver::new(&directory),
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("preserve progress"))
        .unwrap_or_else(|| panic!("no preserve progress diagnostic: {diagnostics:?}"));
    assert_eq!(diagnostic.kind, "parse");
    assert!(diagnostic.located);
    assert_eq!(
        diagnostic.span.python_loc()["line"],
        Value::from(PRESERVE_LINE)
    );
}

/// Migration target: the same mapping in a standalone refinement file still
/// checks progress (and reports the loss the inline seam used to hide).
#[test]
fn standalone_refinement_still_checks_preserve_progress() {
    let (output, status) = run(&[
        "refine",
        "req_plain.fsl",
        "policy.fsl",
        "progress_refines.fsl",
        "--depth",
        "8",
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed");
    assert_eq!(output["kind"], "progress_lost");
    assert_eq!(output["invariant"], "EveryClaimDecided");
}
