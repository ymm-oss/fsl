// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Dual-engine negative control for #502: `reachable(r, a, a)` must give the
//! same verdict under the default (BMC) engine and `--engine explicit`
//! (the solver-free concrete Monitor's BFS oracle), on the same spec, for
//! the same reason. Before the fix, `rust/fsl-runtime/src/lib.rs`'s
//! concrete `relation_reachable` started its BFS frontier at `[source]`
//! and checked `current == target` before traversing any edge, so it
//! reported `reachable(r, a, a)` as trivially true for *any* relation
//! (including an empty one) -- a free zero-hop step the symbolic
//! evaluator's non-reflexive convention never took. `docs/LANGUAGE.md`'s
//! relation section now states the contract explicitly: `reachable(r, a,
//! a)` is true only via a real path of one or more edges.
//!
//! Only the empty-relation shape distinguishes the two conventions: a
//! self-loop or a multi-hop cycle back to `a` is `reachable` either way, so
//! those two shapes are regression controls (the fix must not stop
//! detecting a *real* cycle), not part of the discriminating evidence.
//! Before the fix, all three shapes disagreed between engines in some
//! form -- the empty and self-loop fixtures disagreed on
//! `violated_at_step` (0 vs. 1: the reflexive concrete Monitor found *any*
//! reachable state, including init, already violating), and the cycle
//! fixture made the default engine's own trace-replay consistency check
//! fail outright (`kind:"internal"`, `"trace state mismatch at step 2"`,
//! exit 3) because BMC's own concrete replay of its symbolic counterexample
//! used the same disagreeing concrete evaluator.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
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

fn verify(fixture: &str, depth: &str, engine: Option<&str>) -> (Value, i32) {
    let path = format!("rust/fslc/tests/fixtures/{fixture}");
    let mut arguments = vec!["verify", &path, "--depth", depth, "--no-cache"];
    if let Some(engine) = engine {
        arguments.extend(["--engine", engine]);
    }
    run_cli(&arguments)
}

/// Assert the default (BMC) and `--engine explicit` verdicts for the same
/// fixture agree exactly on `result`, `violation_kind`, and
/// `violated_at_step` -- a dual-engine control, not two single-engine
/// assertions that could each independently pass against a wrong but
/// *consistent* convention.
fn assert_engines_agree(fixture: &str, depth: &str, expected_violated_at_step: i64) {
    let (bmc, bmc_status) = verify(fixture, depth, None);
    let (explicit, explicit_status) = verify(fixture, depth, Some("explicit"));

    assert_eq!(bmc_status, 1, "bmc status for {fixture}: {bmc:#}");
    assert_eq!(
        explicit_status, 1,
        "explicit status for {fixture}: {explicit:#}"
    );
    assert_eq!(bmc["result"], "violated", "{fixture}: {bmc:#}");
    assert_eq!(explicit["result"], "violated", "{fixture}: {explicit:#}");
    assert_eq!(
        bmc["violation_kind"], explicit["violation_kind"],
        "{fixture}: bmc={bmc:#} explicit={explicit:#}"
    );
    assert_eq!(bmc["violation_kind"], "invariant", "{fixture}: {bmc:#}");
    assert_eq!(
        bmc["violated_at_step"], explicit["violated_at_step"],
        "{fixture}: bmc={bmc:#} explicit={explicit:#}"
    );
    assert_eq!(
        bmc["violated_at_step"], expected_violated_at_step,
        "{fixture}: {bmc:#}"
    );
}

/// The discriminating shape: an empty relation. Non-reflexive `reachable`
/// keeps the invariant true at init (no path exists yet), so both engines
/// must agree the first violation is the first `link` action, not init
/// itself.
#[test]
fn reachable_is_non_reflexive_for_an_empty_relation_on_both_engines() {
    assert_engines_agree("issue_502_reachable_empty.fsl", "1", 1);
}

/// Regression control: a genuine self-loop is reachable either way.
#[test]
fn reachable_still_detects_a_genuine_self_loop_on_both_engines() {
    assert_engines_agree("issue_502_reachable_selfloop.fsl", "1", 1);
}

/// Regression control: a genuine multi-hop cycle back to the source is
/// reachable either way.
#[test]
fn reachable_still_detects_a_multi_hop_cycle_through_the_source_on_both_engines() {
    assert_engines_agree("issue_502_reachable_cycle.fsl", "2", 2);
}
