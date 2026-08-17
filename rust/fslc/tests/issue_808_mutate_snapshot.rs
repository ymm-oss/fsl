// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic CLI read-count control for #808's `mutate` command.
//!
//! `mutate` is the most dangerous of the four re-reading commands #808
//! identifies: it runs a baseline `verify`, then separately reloads the
//! Kernel model, the raw source, and the surface document to enumerate and
//! score mutants. If those reads observe different file contents, the
//! reported kill-rate mixes one snapshot's baseline with another snapshot's
//! mutant set -- and kill-rate is the project's primary hollow-spec
//! detector (a low kill-rate flags a hollow spec; `--vacuity` alone misses
//! dead-ghost tautologies), so a torn read can hide a real hollow spec
//! behind a healthy-looking number computed against the wrong content.

mod support;

#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use serde_json::Value;

#[cfg(unix)]
fn mutate_against_two_snapshot_fifo() -> (Value, i32) {
    use std::process::Stdio;

    let source_a = include_str!("fixtures/issue_808_mutate_snapshot_a.fsl").to_owned();
    let source_b = include_str!("fixtures/issue_808_mutate_snapshot_b.fsl").to_owned();
    let mut fixture = support::TwoSnapshotFifo::new("issue-808-mutate", source_a, source_b);
    let path = fixture.path().to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["mutate", &path, "--depth", "4", "--no-cache"])
        .current_dir(support::root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native fslc against FIFO");
    let output = support::wait_for_output(&mut child);

    // This is the correctness oracle. Cleanup opens B only after this point.
    fixture.assert_no_second_open();
    fixture.release_writer();

    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// This is intentionally Unix-only: Windows lacks the FIFO read-count oracle.
/// The non-Unix marker below makes a green Windows result explicitly not
/// claim that this CLI-level control ran there (#806).
#[cfg(unix)]
#[test]
fn mutate_reads_one_fifo_snapshot_for_baseline_and_mutant_enumeration() {
    let (output, status) = mutate_against_two_snapshot_fifo();
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "mutated", "{output:#}");

    // `status`/`result` alone cannot distinguish which snapshot `mutate`
    // actually enumerated mutants from: both fixture A and fixture B verify
    // cleanly as a baseline. Fixture A's `NonNegative` invariant kills the
    // mutant that removes `dec`'s `requires` guard; sibling fixture B is
    // identical except it omits that invariant, so the exact same mutant
    // survives there instead (0.0 kill-rate). This is fixture-content
    // evidence that `mutate` scored mutants against source A, not B.
    let dec_requires_removed = output["mutants"]
        .as_array()
        .expect("mutants array")
        .iter()
        .find(|mutant| mutant["op"] == "requires_remove" && mutant["target"] == "dec requires #1")
        .expect("dec requires_remove mutant present");
    assert_eq!(
        dec_requires_removed["status"], "killed",
        "mutate must score mutants against source A, not source B read by a second open: {output:#}"
    );
    assert_eq!(dec_requires_removed["killed_by"], "NonNegative");
    assert_eq!(
        output["summary"]["kill_rate"].as_f64(),
        Some(0.4615),
        "mutate's kill-rate must reflect source A alone, not a mix with source B: {output:#}"
    );
}

/// Windows does not implement the FIFO read-count control above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_source_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
