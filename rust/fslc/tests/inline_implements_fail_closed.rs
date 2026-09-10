// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native fail-closed contract for inline `implements` (#1002).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/inline_implements_fail_closed";
const CHAIN_FIXTURE: &str = "tests/fixtures/chain/requirements_broken_implements.fsl";
const CHAIN_REFINES_FIXTURE: &str = "tests/fixtures/chain/requirements.fsl";
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

/// detector (mutation C, reached through the *default* `verify` path) **and**
/// the full-envelope control #1002's acceptance list asks for on both sides.
///
/// Two gaps this closes. First, every other `verify` test covering the failing
/// seam carried something extra --- `--lemma`, `--instances`, or a primary
/// invariant that was already violated --- and each enters
/// `run_verify_cli_from_source` on a different branch, so the ordinary
/// unscoped path had only `check`, a different command, standing in for it.
/// Second, the failing side asserted a hand-picked subset of fields; the valid
/// side already had a full-envelope comparison in `inline_implements_bounds.rs`.
///
/// The two fixtures differ **only** in the seam: a declaration name, a
/// two-line comment, and `BDone` -> `BOpen` inside the `map`. So every
/// top-level key is compared, and the five that differ are excluded one at a
/// time with the reason each was observed to differ --- not by naming a
/// category, and not from the type's field list:
///
/// * `result` and `implements` are what the change makes differ; they are
///   asserted explicitly above rather than skipped.
/// * `spec` carries the declaration name, which the fixtures deliberately do
///   not share.
/// * `cost` is wall-clock and solver timing.
/// * `deadlock` was observed to differ in exactly one leaf, `action.loc.line`,
///   by exactly the two lines the comment adds. That is checked as a relation
///   rather than waved through, and the rest of the trace is compared in full.
///
/// Each excluded key is asserted present on **both** sides, so an exclusion
/// cannot quietly become dead when a field is renamed or dropped.
#[test]
fn the_seam_is_the_only_envelope_difference_on_the_default_verify_path() {
    /// Each entry is excluded for a reason observed in the two envelopes, not
    /// for a category it belongs to; every one of them is asserted present on
    /// both sides below, so the list cannot go dead unnoticed.
    const EXCLUDED: [&str; 5] = ["cost", "deadlock", "implements", "result", "spec"];

    let arguments = ["--depth", "3", "--no-cache"];
    let (refines, refines_status) = run(
        "verify",
        &[&[CHAIN_REFINES_FIXTURE][..], &arguments[..]].concat(),
    );
    let (failing, failing_status) = run("verify", &[&[CHAIN_FIXTURE][..], &arguments[..]].concat());

    // The regression #1002 asks for: an ordinary `verify` that succeeds stops
    // succeeding when the only thing that changes is the seam.
    assert_eq!(refines_status, 0, "{refines:#}");
    assert_eq!(refines["result"], "verified");
    assert_eq!(refines["implements"]["result"], "refines");
    assert_eq!(failing_status, 1, "{failing:#}");
    assert_eq!(failing["result"], "refinement_failed");
    assert_eq!(failing["implements"]["result"], "refinement_failed");
    assert!(failing["implements"]["violation"].is_object());

    let refines = refines.as_object().expect("verify envelope is an object");
    let failing = failing.as_object().expect("verify envelope is an object");

    let mut refines_keys: Vec<&str> = refines.keys().map(String::as_str).collect();
    let mut failing_keys: Vec<&str> = failing.keys().map(String::as_str).collect();
    refines_keys.sort_unstable();
    failing_keys.sort_unstable();
    assert_eq!(
        refines_keys, failing_keys,
        "the seam must not add or remove a top-level key"
    );

    for key in EXCLUDED {
        assert!(
            refines.contains_key(key) && failing.contains_key(key),
            "exclusion `{key}` is dead: it is not present on both sides"
        );
    }

    for key in refines_keys {
        if EXCLUDED.contains(&key) {
            continue;
        }
        assert_eq!(
            refines[key], failing[key],
            "`{key}` differs, and only the seam was supposed to"
        );
    }

    // `deadlock` is excluded only for its one observed leaf, so compare the
    // rest of it exactly and check that leaf as a relation.
    let mut refines_deadlock = refines["deadlock"].clone();
    let mut failing_deadlock = failing["deadlock"].clone();
    let refines_line = strip_action_locations(&mut refines_deadlock);
    let failing_line = strip_action_locations(&mut failing_deadlock);
    assert_eq!(
        refines_deadlock, failing_deadlock,
        "the deadlock trace differs somewhere other than the action locations"
    );
    assert!(
        !refines_line.is_empty() && refines_line.len() == failing_line.len(),
        "no action locations were found to compare: {refines_line:?} {failing_line:?}"
    );
    for (before, after) in refines_line.iter().zip(failing_line.iter()) {
        assert_eq!(
            after - before,
            2,
            "the failing fixture's two-line comment should shift every action \
             location by exactly 2, not by {}",
            after - before
        );
    }
}

/// Removes every `action.loc` from a deadlock trace in place and returns the
/// `line` each one carried, in order.
fn strip_action_locations(deadlock: &mut Value) -> Vec<i64> {
    let mut lines = Vec::new();
    let Some(trace) = deadlock.get_mut("trace").and_then(Value::as_array_mut) else {
        return lines;
    };
    for step in trace {
        let Some(action) = step.get_mut("action").and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(location) = action.remove("loc") {
            lines.push(
                location["line"]
                    .as_i64()
                    .expect("action location carries a line number"),
            );
        }
    }
    lines
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
