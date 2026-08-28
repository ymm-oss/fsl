// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic root-snapshot controls for #932's `check` commands.
//!
//! Source A is written to the first FIFO inode, then the pathname is
//! atomically replaced with a FIFO that yields source B only to an illegal
//! second open. The assertion runs before fixture cleanup can open the
//! replacement FIFO.

#[cfg(unix)]
#[path = "support/fifo_snapshot.rs"]
mod fifo_snapshot;

#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(unix)]
use fifo_snapshot::{ReapedChild, TwoSnapshotFifo, WriterOutcome, wait_for_output};
#[cfg(unix)]
use serde_json::Value;

#[cfg(unix)]
const CHECK_SOURCE_A: &str = include_str!("fixtures/vacuous_leadsto.fsl");
#[cfg(unix)]
const CHECK_SOURCE_B: &str = include_str!("fixtures/explicit_vacuous.fsl");
#[cfg(unix)]
const DB_CHECK_SOURCE_A: &str = include_str!("../../../examples/db/safe_add_nullable_column.fsl");
#[cfg(unix)]
const DB_CHECK_SOURCE_B: &str = include_str!("../../../examples/db/safe_rename_preservation.fsl");

#[cfg(unix)]
fn run_against_two_snapshot_fifo(
    command: &[&str],
    options: &[&str],
    source_a: &str,
    source_b: &str,
) -> (String, i32) {
    let mut fixture = TwoSnapshotFifo::new("check", source_a, source_b);
    let path = fixture.fifo.to_string_lossy().into_owned();
    let mut child = ReapedChild::new(
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(command)
            .arg(&path)
            .args(options)
            .current_dir(fifo_snapshot::root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn native fslc against FIFO"),
    );
    let output = wait_for_output(&mut child);

    // This detector must run before cleanup opens the replacement FIFO.
    fixture.assert_no_second_open();
    assert_eq!(
        fixture.release_writer(),
        WriterOutcome::Finished,
        "{command:?} {options:?}"
    );

    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid UTF-8 stdout: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (stdout, output.status.code().expect("exit status"))
}

/// Reverting `run_check_from_source` to any path-reading helper opens source
/// B and fails before cleanup. The result also pins the ordinary check output
/// to source A rather than accepting a generic successful envelope.
#[cfg(unix)]
#[test]
fn check_reads_one_fifo_snapshot() {
    let (stdout, status) =
        run_against_two_snapshot_fifo(&["check"], &[], CHECK_SOURCE_A, CHECK_SOURCE_B);
    let output: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stdout={stdout}"));

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "ok", "{output:#}");
    assert_eq!(output["spec"], "VacuousLeadstoFixture", "{output:#}");
}

/// Reverting either the surface loading or verification call in
/// `run_db_check_from_source` to a path-taking form opens source B and fails
/// before cleanup. The dbsystem name pins the successful projection to A.
#[cfg(unix)]
#[test]
fn db_check_reads_one_fifo_snapshot() {
    let (stdout, status) = run_against_two_snapshot_fifo(
        &["db", "check"],
        &["--depth", "4"],
        DB_CHECK_SOURCE_A,
        DB_CHECK_SOURCE_B,
    );
    let output: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stdout={stdout}"));

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified_under_assumptions", "{output:#}");
    assert_eq!(output["dbsystem"], "SafeAddNullableColumn", "{output:#}");
}

/// Reverting any source-taking call in `run_explain_from_source` to a
/// path-taking form opens source B and fails before cleanup. The complete
/// envelope is still pinned to source A rather than a generic success.
#[cfg(unix)]
#[test]
fn explain_reads_one_fifo_snapshot() {
    let (stdout, status) =
        run_against_two_snapshot_fifo(&["explain"], &[], CHECK_SOURCE_A, CHECK_SOURCE_B);
    let output: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stdout={stdout}"));

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "explained", "{output:#}");
    assert_eq!(output["spec"], "VacuousLeadstoFixture", "{output:#}");
}

/// Reverting any source-taking call in `run_html_report_from_source` opens
/// source B and fails before cleanup. With no output path, HTML is delivered
/// as raw stdout, so the rendered source-A spec name is the content oracle.
#[cfg(unix)]
#[test]
fn html_reads_one_fifo_snapshot() {
    let (html, status) =
        run_against_two_snapshot_fifo(&["html"], &[], CHECK_SOURCE_A, CHECK_SOURCE_B);

    assert_eq!(status, 0, "{html}");
    assert!(
        html.contains("VacuousLeadstoFixture"),
        "HTML must be rendered from source A: {html}"
    );
}

/// Windows does not implement the FIFO read-count oracle above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_check_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
