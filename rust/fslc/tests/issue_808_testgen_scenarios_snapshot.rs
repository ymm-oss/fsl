// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic CLI read-count control for #808's generic `testgen` and
//! `scenarios` command entries (PR 3). Reuses the `TwoSnapshotFifo` oracle
//! extracted for `run_verify` (PR 1, #811) in
//! `support/fifo_snapshot.rs`.
//!
//! Unlike the `run_verify` control, source A and source B here are both
//! *valid* FSL documents that declare distinct action names. `testgen` and
//! `scenarios` never return `result: "error"` for either source, so a
//! status-only or `result != "error"` check cannot tell a correct single
//! read apart from a second read that landed on a different-but-still-valid
//! snapshot; only content bound to A proves the fix.

#[cfg(unix)]
#[path = "support/fifo_snapshot.rs"]
mod fifo_snapshot;

#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use fifo_snapshot::{ReapedChild, TwoSnapshotFifo, WriterOutcome, wait_for_output};
#[cfg(unix)]
use serde_json::Value;

#[cfg(unix)]
const READY_FIXTURE: &str = r"
spec ReadyFixture {
  state { pending: Bool, done: Bool }
  init { pending = false  done = false }

  action arrive() {
    requires not pending
    pending = true
  }

  action finish() {
    requires pending
    pending = false
    done = true
  }

  leadsTo Served { pending ~> done }
}
";

#[cfg(unix)]
const ALT_FIXTURE: &str = r"
spec AltFixture {
  state { count: Int }
  init { count = 0 }

  action increment() {
    count = count + 1
  }

  action decrement() {
    requires count > 0
    count = count - 1
  }
}
";

#[cfg(unix)]
const SPEND_FIXTURE: &str = r"
spec SpendFixture {
  state { balance: Int }
  init { balance = 0 }

  invariant NonNegativeBalance { balance >= 0 }

  action spend() {
    balance = balance - 1
  }
}
";

#[cfg(unix)]
const GROW_FIXTURE: &str = r"
spec GrowFixture {
  state { total: Int }
  init { total = 0 }

  action grow() {
    total = total + 1
  }
}
";

#[cfg(unix)]
fn spawn_against_two_snapshot_fifo(
    label: &str,
    source_a: &str,
    source_b: &str,
    cli_args: &[&str],
) -> (TwoSnapshotFifo, std::process::Output) {
    use std::process::Stdio;

    let mut fixture = TwoSnapshotFifo::new(label, source_a, source_b);
    let path = fixture.fifo.to_string_lossy().into_owned();
    let mut args: Vec<&str> = Vec::new();
    let mut remaining = cli_args.iter();
    args.push(remaining.next().expect("command name"));
    args.push(&path);
    args.extend(remaining);
    let mut child = ReapedChild::new(
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(&args)
            .current_dir(fifo_snapshot::root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn native fslc against FIFO"),
    );
    let output = wait_for_output(&mut child);

    // This is the correctness oracle. Cleanup opens B only after this point.
    fixture.assert_no_second_open();
    assert_eq!(fixture.release_writer(), WriterOutcome::Finished, "{label}");
    (fixture, output)
}

#[cfg(unix)]
fn run_against_two_snapshot_fifo(
    label: &str,
    source_a: &str,
    source_b: &str,
    cli_args: &[&str],
) -> (Value, i32) {
    let (_fixture, output) = spawn_against_two_snapshot_fifo(label, source_a, source_b, cli_args);
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// `testgen` prints its generated source raw to stdout (no JSON envelope)
/// when `-o`/`--output` is not given, matching every other codegen command's
/// `raw_delivery_allowed` behavior. Assert on that raw text directly instead
/// of attempting to parse it as JSON.
#[cfg(unix)]
fn run_raw_against_two_snapshot_fifo(
    label: &str,
    source_a: &str,
    source_b: &str,
    cli_args: &[&str],
) -> (String, i32) {
    let (_fixture, output) = spawn_against_two_snapshot_fifo(label, source_a, source_b, cli_args);
    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid UTF-8 stdout: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (stdout, output.status.code().expect("exit status"))
}

/// `scenarios` must build its `action_coverage`/`leadsTo` scenario set from
/// the single captured root snapshot. Reverting
/// `run_scenarios_mode_from_source`'s `load_model_from_source` call back to
/// `load_model(path)` makes this observe `AltFixture`'s `increment`/
/// `decrement` actions instead of `ReadyFixture`'s `arrive`/`finish`.
#[cfg(unix)]
#[test]
fn scenarios_reads_one_fifo_snapshot() {
    let (output, status) = run_against_two_snapshot_fifo(
        "scenarios",
        READY_FIXTURE,
        ALT_FIXTURE,
        &["scenarios", "--depth", "4", "--deadlock", "warn"],
    );
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["spec"], "ReadyFixture", "{output:#}");
    let covered_actions: std::collections::BTreeSet<_> = output["scenarios"]
        .as_array()
        .expect("scenarios array")
        .iter()
        .filter(|scenario| scenario["kind"] == "action_coverage")
        .filter_map(|scenario| scenario["action"].as_str())
        .collect();
    assert_eq!(
        covered_actions,
        std::collections::BTreeSet::from(["arrive", "finish"]),
        "must cover only source A's actions, not source B's: {output:#}"
    );
    assert!(
        output["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .any(|scenario| scenario["kind"] == "leadsTo" && scenario["property"] == "Served"),
        "must retain source A's leadsTo response scenario: {output:#}"
    );
}

/// The BMC fallback branch (a genuine violation) must also derive from the
/// single captured snapshot. Reverting the fallback's
/// `run_verify_from_source` call back to `run_verify(path, ...)` makes this
/// observe `GrowFixture` (unviolated, exits 0) instead of `SpendFixture`'s
/// `NonNegativeBalance` violation.
#[cfg(unix)]
#[test]
fn scenarios_violation_fallback_reads_one_fifo_snapshot() {
    let (output, status) = run_against_two_snapshot_fifo(
        "scenarios-violation",
        SPEND_FIXTURE,
        GROW_FIXTURE,
        &["scenarios", "--depth", "4", "--deadlock", "warn"],
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(
        output["spec"], "SpendFixture",
        "the BMC fallback must report source A's spec, not source B's: {output:#}"
    );
    assert_eq!(
        output["invariant"], "NonNegativeBalance",
        "the BMC fallback must report source A's invariant: {output:#}"
    );
}

/// `testgen` must generate its pytest content from the same single captured
/// snapshot as `scenarios`. Any re-read of the root path after the first
/// (whether inside `run_testgen` or inside the `run_scenarios_mode_from_source`
/// it calls) makes this observe source B's actions in the generated file.
#[cfg(unix)]
#[test]
fn testgen_reads_one_fifo_snapshot() {
    let (content, status) = run_raw_against_two_snapshot_fifo(
        "testgen",
        READY_FIXTURE,
        ALT_FIXTURE,
        &[
            "testgen",
            "--depth",
            "4",
            "--target",
            "pytest",
            "--deadlock",
            "warn",
        ],
    );
    assert_eq!(status, 0, "{content}");
    assert!(
        content.contains("arrive") && content.contains("finish"),
        "generated test must reference source A's actions: {content}"
    );
    assert!(
        !content.contains("increment") && !content.contains("decrement"),
        "generated test must not reference source B's actions: {content}"
    );
}

/// Windows does not implement the FIFO read-count control above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_testgen_scenarios_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
