// SPDX-License-Identifier: Apache-2.0

//! Negative control for #486: native `vacuous_implication`
//! (`rust/fsl-runtime/src/lib.rs::verification_warnings`) must unwrap a
//! `forall`-quantified implication before checking antecedent reachability,
//! not only a bare top-level `Binary{op: "=>"}`. This is the shape
//! `docs/DESIGN-vacuity.md:22` documents as the primary case. Before the
//! fix, `issue_486_vacuous_forall_implication.fsl` verified clean (no
//! `vacuous_implication` warning at all, so `--vacuity error` never fired)
//! because the top-level expression of the invariant is a `forall`, not a
//! `=>`. Reverting the fix reproduces that: `assert_eq!(status, 2)` below
//! fails (`verified`/exit 0 instead of `error`/exit 2).

use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE: &str = "rust/fslc/tests/fixtures/issue_486_vacuous_forall_implication.fsl";

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

#[test]
fn a_forall_wrapped_implication_is_flagged_vacuous_by_default() {
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
            .any(|warning| warning["kind"] == "vacuous_implication"
                && warning["name"] == "NeverActivated"),
        "expected a vacuous_implication warning for NeverActivated: {output:#}"
    );
}

/// Before #486, this forall-wrapped implication was invisible to
/// `verification_warnings` (its top-level expression is `forall`, not
/// `=>`), so `--vacuity error` had nothing to select and this hollow
/// invariant passed with `result:"verified"`/exit 0. If this regresses,
/// the assertions below fail.
#[test]
fn a_forall_wrapped_implication_fails_closed_under_vacuity_error() {
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
    assert_eq!(output["kind"], "vacuous_implication");
    assert_eq!(output["findings"][0]["name"], "NeverActivated");
}

/// Same fixture on `--engine explicit`: `verification_warnings` is shared
/// across engines (a single, engine-agnostic diagnostic pass over the
/// solver-free concrete Monitor's BFS), so the fix must not be BMC-only.
#[test]
fn a_forall_wrapped_implication_fails_closed_on_the_explicit_engine_too() {
    let (output, status) = run_cli(&[
        "verify",
        FIXTURE,
        "--engine",
        "explicit",
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
    assert_eq!(output["kind"], "vacuous_implication");
    assert_eq!(output["findings"][0]["name"], "NeverActivated");
}
