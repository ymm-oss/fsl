// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #519.
//!
//! `Monitor::new` used to default-fill any state component `init` left
//! unassigned (e.g. a `Map` key never written) instead of rejecting a
//! nondeterministic init the way the explicit engine's construction gate
//! already did. `replay` then compared an observed initial state — which
//! may legitimately differ on the free component, since BMC explores every
//! admissible value there — against that one arbitrary default, and
//! falsely reported `initial_state_mismatch` on a BMC-valid trace.
//!
//! The fix adds the same deterministic-init gate to `Monitor::new` itself,
//! and separately teaches every caller that already has its own complete
//! concrete initial state (a BMC witness's first state, or a replay
//! trace's own observed initial state) to build the monitor directly from
//! that state instead of demanding `Monitor::new`'s determinism.
//!
//! The controls below check both directions per the review requirement:
//! a BMC-valid initial state must stop being falsely reported as violated,
//! and a genuinely nonconformant trace must still be reported as such.

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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

/// The core #519 fix: a replay trace whose observed initial state assigns
/// a value BMC admits for a component `init` leaves free (`m[1]`) must be
/// accepted, not rejected as `initial_state_mismatch` against Monitor's
/// old arbitrary default for that component.
#[test]
fn replay_accepts_a_bmc_valid_initial_state_that_leaves_a_component_free() {
    let (output, status) = run(&[
        "replay",
        fixture("issue_519_partial_bool.fsl").to_str().unwrap(),
        "--trace",
        fixture("issue_519_valid_free_component.v1.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformant", "{output:#}");
    assert_eq!(output["final_state"]["m"]["1"], true, "{output:#}");
}

/// Required negative control: this fix must not turn into a blanket
/// "always accept the observed initial state" — a trace whose *later*
/// step does not match the real concrete transition must still be
/// reported nonconformant.
#[test]
fn replay_still_rejects_a_genuinely_wrong_post_action_state() {
    let (output, status) = run(&[
        "replay",
        fixture("issue_519_partial_bool.fsl").to_str().unwrap(),
        "--trace",
        fixture("issue_519_bad_post_action_state.v1.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(output["violation"]["kind"], "state_mismatch", "{output:#}");
}

/// Regression control: when `init` deterministically assigns every
/// component (no free component at all), `Monitor::new` still succeeds
/// directly and a genuinely wrong observed initial state is still caught
/// as `initial_state_mismatch` exactly as before this fix.
#[test]
fn replay_still_rejects_a_wrong_initial_state_when_init_is_fully_deterministic() {
    let (output, status) = run(&[
        "replay",
        fixture("issue_519_full_bool.fsl").to_str().unwrap(),
        "--trace",
        fixture("issue_519_full_wrong_initial.v1.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(
        output["violation"]["kind"], "initial_state_mismatch",
        "{output:#}"
    );
}

/// Positive control (from the issue): a fully deterministic
/// `forall`-initialized spec, replayed with a correct trace, remains
/// conformant.
#[test]
fn replay_still_accepts_a_correct_trace_when_init_is_fully_deterministic() {
    let (output, status) = run(&[
        "replay",
        fixture("issue_519_full_bool.fsl").to_str().unwrap(),
        "--trace",
        fixture("issue_519_full_correct.v1.json").to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformant", "{output:#}");
}

/// Named negative control from the issue itself: with no observed initial
/// state to fall back on at all (a `Legacy` trace carries none), a
/// nondeterministic init has nothing to build a concrete Monitor from and
/// must still fail Monitor/replay construction with `kind:"semantics"`,
/// exit 2 — this fix must not silently pick an initial state on its own.
#[test]
fn replay_still_fails_closed_with_no_observed_initial_state_to_fall_back_on() {
    let (output, status) = run(&[
        "replay",
        fixture("issue_519_partial_bool.fsl").to_str().unwrap(),
        "--trace",
        fixture("issue_519_legacy_no_initial.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not assign state variable")),
        "{output:#}"
    );
}

/// Sanity control: the explicit engine's own construction gate
/// (`check_deterministic_init`, which `Monitor::new` now reuses) is
/// unaffected by this fix and still fails closed the same way.
#[test]
fn explicit_engine_still_fails_closed_for_nondeterministic_init() {
    let (output, status) = run(&[
        "verify",
        fixture("issue_519_partial_bool.fsl").to_str().unwrap(),
        "--depth",
        "4",
        "--engine",
        "explicit",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
}
