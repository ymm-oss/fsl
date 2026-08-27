// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic root-snapshot controls for #808's remaining domain commands.
//!
//! The shared FIFO oracle writes source A to the first inode, atomically
//! replaces the path with a second FIFO, and makes source B available only to
//! an illegal second open. Every test asserts that second open did not happen
//! before cleanup, then checks an observable unique to source A.

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
const SOURCE_A: &str = include_str!("fixtures/issue_641_domain_clean.fsl");
#[cfg(unix)]
const SOURCE_B: &str = include_str!("fixtures/domain_characterization/expressions_valid.fsl");

#[cfg(unix)]
fn run_domain_against_two_snapshot_fifo(subcommand: &str, args: &[&str]) -> (String, i32) {
    use std::process::Stdio;

    // The source fixture's historical `||` spelling is intentionally rejected
    // by `--edition next`. Use its canonical equivalent so the check test can
    // exercise edition enrichment rather than its negative-control error.
    let source_a = SOURCE_A.replace(" || ", " or ");
    let mut fixture = TwoSnapshotFifo::new(subcommand, &source_a, SOURCE_B);
    let path = fixture.fifo.to_string_lossy().into_owned();
    let mut child = ReapedChild::new(
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["domain", subcommand, &path])
            .args(args)
            .current_dir(fifo_snapshot::root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn native fslc against FIFO"),
    );
    let output = wait_for_output(&mut child);

    // This is the detector. Cleanup opens the replacement FIFO only after the
    // assertion, so a passing check proves the command did not reread `path`.
    fixture.assert_no_second_open();
    assert_eq!(
        fixture.release_writer(),
        WriterOutcome::Finished,
        "{subcommand}"
    );

    (
        String::from_utf8(output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid UTF-8 stdout: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        }),
        output.status.code().expect("exit status"),
    )
}

#[cfg(unix)]
fn run_domain_json_against_two_snapshot_fifo(subcommand: &str, args: &[&str]) -> (Value, i32) {
    let (stdout, status) = run_domain_against_two_snapshot_fifo(subcommand, args);
    let output = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stdout={stdout}"));
    (output, status)
}

/// Reverting `domain_scaffold_inputs_from_source` to a path-based Kernel load
/// performs a second open and makes this detector fail before source B is
/// released. The generated scaffold must also name source A's domain.
#[cfg(unix)]
#[test]
fn domain_generate_reads_one_fifo_snapshot() {
    let (output, status) =
        run_domain_json_against_two_snapshot_fifo("generate", &["--target", "typescript"]);

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "generated", "{output:#}");
    assert_eq!(output["domain"], "CleanDiagnosticDomain", "{output:#}");
}

/// Reverting `load_model_from_source` to `load_model(path)` performs a second
/// open and makes this detector fail before source B is released. An empty log
/// keeps the replay result source-independent except for its domain name.
#[cfg(unix)]
#[test]
fn domain_replay_reads_one_fifo_snapshot() {
    let log = std::env::temp_dir().join(format!(
        "fslc-issue-808-domain-replay-{}.json",
        std::process::id()
    ));
    std::fs::write(&log, "[]").expect("write empty replay log");
    let log_text = log.to_string_lossy().into_owned();
    let (output, status) =
        run_domain_json_against_two_snapshot_fifo("replay", &["--logs", &log_text]);
    std::fs::remove_file(&log).expect("remove empty replay log");

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
    assert_eq!(output["domain"], "CleanDiagnosticDomain", "{output:#}");
}

/// Reverting either `run_testgen_from_source` call or the Vitest adapter
/// scaffold call to a path-based form performs a second open and makes this
/// detector fail before source B is released. Raw delivery is part of the
/// public contract when no output path is supplied.
#[cfg(unix)]
#[test]
fn domain_testgen_reads_one_fifo_snapshot() {
    let (content, status) =
        run_domain_against_two_snapshot_fifo("testgen", &["--depth", "4", "--target", "vitest"]);

    assert_eq!(status, 0, "{content}");
    assert!(
        content.contains("Doc.CloseDoc"),
        "generated test must retain source A's command: {content}"
    );
    assert!(
        !content.contains("DomainExpressionCharacterization"),
        "generated test must not contain source B's domain: {content}"
    );
}

/// Reverting either `run_verify_from_source` or
/// `apply_domain_edition_from_source` to its path-based counterpart performs a
/// second open and makes this detector fail before source B is released.
#[cfg(unix)]
#[test]
fn domain_check_reads_one_fifo_snapshot() {
    let (output, status) =
        run_domain_json_against_two_snapshot_fifo("check", &["--depth", "4", "--edition", "next"]);

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified_under_assumptions", "{output:#}");
    assert_eq!(output["domain"], "CleanDiagnosticDomain", "{output:#}");
    assert_eq!(output["edition"], "next", "{output:#}");
}

/// Windows does not implement the FIFO read-count oracle above. This marker
/// prevents a passing non-Unix test run from being mistaken for its evidence.
#[cfg(not(unix))]
#[test]
fn fifo_domain_snapshot_control_is_unavailable_on_non_unix() {
    assert!(cfg!(not(unix)));
}
