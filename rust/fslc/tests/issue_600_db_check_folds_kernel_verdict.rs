// SPDX-License-Identifier: Apache-2.0

//! Negative control for #600: `fslc db check` must fold *every* non-passing
//! nested kernel verdict into its top-level `result`, not only `violated`.
//!
//! `run_db_check` set `result:"verified_under_assumptions"` whenever the
//! `dbsystem` compatibility rules produced no finding, then ran the kernel
//! `verify` and rewrote the top level only for `violated`. A kernel that came
//! back `unknown_cti`, `reachable_failed`, or `unknown_budget` therefore
//! produced an envelope reporting success alongside exit 1 -- the exit code
//! was right and the JSON lied about it, which is the half of the Verdict
//! Conservation Law that a gate reading the envelope (rather than the exit
//! code) sees.
//!
//! The `run_domain_check` sibling has folded the same way since #515, and its
//! status guard admits both definitive codes (`status != 0 && status != 1`)
//! rather than testing `== 2`. `db` now matches it on both counts.

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

fn fixture() -> String {
    "rust/fslc/tests/fixtures/issue_600_db_inconclusive_kernel.fsl".to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {args:?}`: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output
        .status
        .code()
        .unwrap_or_else(|| panic!("`fslc {args:?}` terminated by signal, no exit code"));
    (value, status)
}

/// The fixture has to keep producing an inconclusive kernel for this control
/// to mean anything. If a verifier improvement makes it `proved`, this test
/// fails loudly rather than passing vacuously.
#[test]
fn the_fixture_still_produces_an_inconclusive_kernel() {
    let path = fixture();
    let (verification, status) = run(&[
        "verify",
        &path,
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
    ]);
    assert_eq!(
        verification["result"], "unknown_cti",
        "fixture must still be non-inductive for #600's control to exercise \
         the fold; got {verification}"
    );
    assert_eq!(status, 1);
}

#[test]
fn db_check_folds_an_inconclusive_kernel_into_its_top_level_verdict() {
    let path = fixture();
    let (output, status) = run(&["db", "check", &path, "--engine", "induction"]);

    // The dbsystem layer itself is clean: this is purely the kernel verdict
    // folding through. Without that, the assertion below would be testing the
    // ordinary `violated` path that already worked.
    assert_eq!(
        output["findings"].as_array().map(Vec::len),
        Some(0),
        "fixture must stay finding-free at the dbsystem layer; got {output}"
    );
    assert_eq!(
        output["kernel"]["result"], "unknown_cti",
        "the nested kernel verdict must still be reported; got {output}"
    );

    assert_ne!(
        output["result"], "verified_under_assumptions",
        "an inconclusive kernel must not be reported as success (#600); got {output}"
    );
    assert_eq!(output["result"], "violated");
    assert_ne!(status, 0);
}

/// The other half of #600: the status guard. `run_domain_check` returns the
/// kernel envelope verbatim for any status that is neither 0 nor 1;
/// `run_db_check` tested `== 2` and so would have absorbed a `kind:"internal"`
/// (status 3) kernel envelope as a `dbsystem` verdict. A parse error (status
/// 2) must still come back verbatim, which is what this pins.
#[test]
fn db_check_returns_a_spec_error_envelope_verbatim() {
    let (output, status) = run(&["db", "check", "rust/fslc/tests/fixtures/does_not_exist.fsl"]);
    assert_eq!(output["result"], "error", "got {output}");
    assert_eq!(status, 2);
    assert_ne!(
        output["result"], "verified_under_assumptions",
        "a spec error must never be absorbed into a dbsystem verdict"
    );
}
