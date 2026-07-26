// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #554: `fslc mutate` re-emits its baseline
//! `verify` envelope verbatim when the baseline does not verify, but derived
//! the exit code from `result == "error"` alone. Every other non-success
//! result therefore fell through to 0, so a spec whose baseline is already
//! `violated` returned `result:"violated"` with exit 0 — a mutation score is
//! meaningless over a spec that already fails, and a gate reading only the
//! exit code saw a pass. `docs/LANGUAGE.md`'s exit-code table maps `violated`
//! to 1 with no per-command exemption, and `scenarios`/`testgen` re-emit the
//! same envelope and already exit 1.
//!
//! The status now comes from `mutate_exit_status`, a total match over the
//! result vocabulary a mutation run can carry.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const VIOLATED_BASELINE: &str = "examples/gallery/errors/violated_invariant_counter.fsl";
const HEALTHY_SPEC: &str = "specs/cart_v1.fsl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
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

// --- negative: a violated baseline must not exit 0 ------------------------

#[test]
fn a_violated_baseline_exits_one() {
    let (value, status) = run(&["mutate", VIOLATED_BASELINE, "--max-mutants", "3"]);
    assert_eq!(value["result"], "violated", "{value}");
    assert_eq!(status, 1, "{value}");
}

#[test]
fn mutate_and_testgen_agree_on_the_same_violated_baseline() {
    // Both commands re-emit the baseline `verify` envelope; `testgen` already
    // exited 1, so a disagreement here is the defect rather than a judgement
    // call about what `mutate` should do.
    let (mutated, mutate_status) = run(&["mutate", VIOLATED_BASELINE, "--max-mutants", "3"]);
    let (generated, testgen_status) = run(&["testgen", VIOLATED_BASELINE, "--depth", "3"]);
    assert_eq!(mutated["result"], generated["result"], "{mutated}");
    assert_eq!(mutate_status, testgen_status);
}

#[test]
fn an_unreadable_external_mutant_file_is_a_spec_error() {
    // Reachable only once the baseline verifies, so it is a second site of the
    // same defect: it returned `result:"error"` with exit 0 before the status
    // became a total function of `result`.
    let (value, status) = run(&[
        "mutate",
        HEALTHY_SPEC,
        "--max-mutants",
        "0",
        "--from",
        "does/not/exist.jsonl",
    ]);
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "io", "{value}");
    assert_eq!(status, 2, "{value}");
}

#[test]
fn a_spec_that_does_not_load_still_exits_two() {
    // The #484 behavior this commit must not disturb.
    let (value, status) = run(&[
        "mutate",
        "examples/gallery/errors/parse_missing_expression.fsl",
        "--max-mutants",
        "3",
    ]);
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "parse", "{value}");
    assert_eq!(status, 2, "{value}");
}

// --- positive: a healthy spec still scores and exits 0 --------------------

#[test]
fn a_healthy_spec_still_mutates_and_exits_zero() {
    let (value, status) = run(&["mutate", HEALTHY_SPEC, "--max-mutants", "3"]);
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["summary"]["total"], 3, "{value}");
}
