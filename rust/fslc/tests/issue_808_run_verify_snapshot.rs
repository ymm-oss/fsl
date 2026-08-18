// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic CLI read-count control for #808's `run_verify` foundation.

#[cfg(unix)]
#[path = "support/fifo_snapshot.rs"]
mod fifo_snapshot;

#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use fifo_snapshot::{TwoSnapshotFifo, wait_for_output};
#[cfg(unix)]
use serde_json::Value;

#[cfg(unix)]
fn verify_against_two_snapshot_fifo(engine: &str, edition: &str) -> (Value, i32) {
    use std::process::Stdio;

    let source_a = include_str!("fixtures/vacuous_leadsto.fsl");
    let source_b = "not valid FSL source";
    let mut fixture = TwoSnapshotFifo::new("verify", source_a, source_b);
    let path = fixture.fifo.to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            &path,
            "--depth",
            "4",
            "--engine",
            engine,
            "--edition",
            edition,
            "--no-cache",
        ])
        .current_dir(fifo_snapshot::root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native fslc against FIFO");
    let output = wait_for_output(&mut child);

    // This is the correctness oracle. Cleanup opens B only after this point.
    fixture.assert_no_second_open();
    fixture.release_writer();

    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; engine={engine}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// The BMC control also exercises source A's `leadsTo` property. Induction is
/// separately pinned because it has its own base/step and liveness paths.
///
/// This is intentionally Unix-only: Windows lacks the FIFO read-count oracle.
/// The non-Unix marker below makes a green Windows result explicitly not claim
/// that this CLI-level control ran there (#806).
#[cfg(unix)]
#[test]
fn verify_reads_one_fifo_snapshot_for_bmc_induction_and_liveness() {
    for (engine, edition, expected_result) in [
        ("bmc", "current", "verified"),
        ("induction", "next", "proved"),
    ] {
        let (output, status) = verify_against_two_snapshot_fifo(engine, edition);
        assert_eq!(status, 0, "{engine}: {output:#}");
        assert_eq!(
            output["result"], expected_result,
            "{engine} must verify source A, not invalid source B: {output:#}"
        );
        assert_eq!(
            output["leads_to"]["Served"]["checked_to_depth"], 4,
            "{engine} must execute source A's leadsTo check: {output:#}"
        );
        // `checked_to_depth` alone only echoes `--depth`; it says nothing
        // about the model that was actually loaded. The `vacuous_leadsto`
        // warning instead depends on the engine having evaluated source A's
        // `pending ~> done` trigger reachability, so it is content-bound
        // evidence that the liveness check ran against A, not against B.
        assert!(
            output["warnings"].as_array().is_some_and(|warnings| {
                warnings.iter().any(|warning| {
                    warning["kind"] == "vacuous_leadsto" && warning["name"] == "Served"
                })
            }),
            "{engine} must report source A's vacuous leadsTo trigger: {output:#}"
        );
    }
}

/// Windows does not implement the FIFO read-count control above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_source_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
