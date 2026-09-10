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
/// * `cost` differs only in its elapsed-time leaves. Its counters --- solver
///   checks, conflicts, decisions, propagations, and every per-property check
///   count --- are compared, and only the timings are dropped.
///
///   ⚠️ An earlier version of this paragraph justified comparing those
///   counters by saying they "were observed identical", one sentence before
///   forbidding exactly that reasoning for `solver.memory_mb`. **Two standards
///   in one paragraph, and the weaker one was applied to the larger set.** The
///   counters are compared on two premises. The first is that each side is
///   *reproducible*: `random_seed` and `smt.random_seed` are pinned to a
///   constant, and no `timeout`, `rlimit`, `soft_timeout` or `max_memory` is
///   set in `fsl-solver-z3` or `fsl-solver` (**measured, by reading both
///   crates**), so re-running one fixture gives the same counters. That alone
///   is **not** enough for this assertion, which compares two *different*
///   fixtures.
///
///   The second premise is the one that carries the weight: that the two
///   fixtures generate the same primary solver queries in the same order, so
///   that equal counters are the expected outcome rather than a coincidence.
///   The fixtures differ only in the seam --- a declaration name, a two-line
///   comment, and `BDone` -> `BOpen` inside the `map` --- none of which changes
///   the primary invariant or its bound. ⚠️ **That is an argument, not a
///   measurement.** Nothing here demonstrates query-level equality, and if it
///   is false this assertion is pinning a coincidence.
///
///   Two further conditions are worth stating because they are not
///   established here: Z3's own defaults are assumed single-threaded and
///   unbounded (**not verified**), and the two sides are produced by **two
///   separate `fslc` child processes** of one build, not by one solver
///   process, so cross-process variation is in scope. A different Z3 build may
///   legitimately search differently.
///
///   ⚠️ **This comparison is not a detector for its own premises**, and an
///   earlier version of this comment claimed it was. Nondeterminism that
///   perturbs both sides equally, or that happens to land on equal counters,
///   leaves the assertion green. What the assertion establishes is that the
///   seam did not change these counters, on the runs that were executed --- not
///   that the search is deterministic.
///
///   `decisions` is among the compared counters and is worth naming: the
///   backend queries both `decisions` and `sat decisions`, and on the runs
///   observed here Z3 reported neither, so the leaf serialises as `null` and
///   the comparison is `null` against `null`. **That is a run observation, not
///   a property of the tree.** It stays compared, because a build that began
///   reporting it would be worth seeing --- though note the comparison is
///   between the two sides, so a build that reported the *same* non-null value
///   on both would pass unnoticed.
///
///   `solver.memory_mb` is the one exception, excluded for what it is rather
///   than for how it behaved: it is Z3's `memory`/`max memory` statistic, a
///   high-water reading of the process rather than a statistic of the search,
///   so it answers to the allocator and to whatever else the process did. It
///   was in fact observed identical across runs, and that is **not** why it is
///   excluded --- an observable that *can* vary with ambient state is not made
///   stable by agreeing. It is compared for presence and shape instead.
///   Presence cannot fail from ambient variation, because the field carries no
///   `skip_serializing_if` and is therefore always emitted; the shape
///   assertion does require that Z3 reported the statistic at all, which this
///   repository already depends on in `fsl-solver-z3`'s own tests.
/// * `deadlock` was observed to differ in exactly one leaf, `action.loc.line`,
///   by exactly the two lines the comment adds. That is checked as a relation
///   rather than waved through; `action.loc.column` was observed identical and
///   is compared, and the rest of the trace is compared in full.
///
/// Neither `cost` nor `deadlock` is dropped as a branch. A leaf that differs is
/// not a reason to stop looking at its siblings --- that is how a whole subtree
/// stops being checked for the sake of one timestamp.
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

    // The seam envelope itself, compared rather than sampled: the abstract it
    // refines against is the same on both sides, and `violation` is the only
    // key the failure adds.
    assert_eq!(refines["implements"]["abs"], failing["implements"]["abs"]);
    assert_eq!(sorted_keys(&refines["implements"]), ["abs", "result"]);
    assert_eq!(
        sorted_keys(&failing["implements"]),
        ["abs", "result", "violation"]
    );

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

    // `cost` is excluded only for its timings, so compare what is left of it.
    let mut refines_cost = refines["cost"].clone();
    let mut failing_cost = failing["cost"].clone();
    let refines_timings = strip_elapsed(&mut refines_cost);
    let failing_timings = strip_elapsed(&mut failing_cost);
    for cost in [&mut refines_cost, &mut failing_cost] {
        let solver = cost["solver"]
            .as_object_mut()
            .expect("cost carries a solver object");
        let memory = solver
            .remove("memory_mb")
            .expect("solver statistics carry memory_mb");
        assert!(
            memory.as_f64().is_some_and(|value| value > 0.0),
            "memory_mb is ambient, so it is checked for shape, not equality: {memory}"
        );
    }
    assert_eq!(
        refines_cost, failing_cost,
        "the seam changed a cost counter, not just a timing"
    );
    assert!(
        refines_timings > 0 && refines_timings == failing_timings,
        "no timings were found to drop: {refines_timings} {failing_timings}"
    );

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

/// The sorted key set of a JSON object, for comparing an envelope's shape
/// rather than a hand-picked subset of its fields.
fn sorted_keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// Removes every elapsed-time leaf from a `cost` object in place and returns
/// how many were removed, so an empty removal cannot pass as agreement.
fn strip_elapsed(cost: &mut Value) -> usize {
    fn visit(value: &mut Value, removed: &mut usize) {
        match value {
            Value::Object(map) => {
                map.retain(|key, _| {
                    let keep = !key.ends_with("elapsed_s");
                    if !keep {
                        *removed += 1;
                    }
                    keep
                });
                for nested in map.values_mut() {
                    visit(nested, removed);
                }
            }
            Value::Array(items) => {
                for item in items {
                    visit(item, removed);
                }
            }
            _ => {}
        }
    }
    let mut removed = 0;
    visit(cost, &mut removed);
    removed
}

/// Removes every `action.loc.line` from a deadlock trace in place, leaving the
/// rest of each location to be compared, and returns the lines in order.
fn strip_action_locations(deadlock: &mut Value) -> Vec<i64> {
    let mut lines = Vec::new();
    let Some(trace) = deadlock.get_mut("trace").and_then(Value::as_array_mut) else {
        return lines;
    };
    for step in trace {
        let Some(action) = step.get_mut("action").and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(mut location) = action.remove("loc") {
            let line = location["line"]
                .as_i64()
                .expect("action location carries a line number");
            // The column was observed identical on both sides, so it stays in
            // the compared part rather than leaving with the line.
            location
                .as_object_mut()
                .expect("loc is an object")
                .remove("line");
            action.insert("loc_without_line".to_owned(), location);
            lines.push(line);
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
