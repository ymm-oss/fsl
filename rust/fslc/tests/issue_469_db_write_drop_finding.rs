// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #469: `fslc db check` reported a success
//! verdict with an empty `findings` array on a write-drop compatibility
//! violation because `rust/fsl-tools/src/db.rs` only ever probed the `reads`
//! capability when a migration drops a column, and `run_db_check` never
//! reconciled the top-level JSON `result` with a subsequently violated
//! `kernel` projection. Both the (bounded-depth) shallow case and a
//! deep-migration-history case that used to escape the kernel's hardcoded
//! depth-8 default must now report `result: "violated"` with a
//! `column_removed_while_still_written` finding, exactly as the frozen
//! Python reference does.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const READER_CONTROL: &str = "examples/db/unsafe_drop_column_with_worker.fsl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

#[test]
fn db_check_reports_a_write_drop_violation_with_a_populated_finding() {
    let writer = fixture("issue_469_unsafe_drop_column_with_writer.fsl");
    let (value, status) = run(&["db", "check", writer.to_str().expect("utf8 path")]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated", "{value}");

    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    let finding = &findings[0];
    assert_eq!(finding["kind"], "column_removed_while_still_written");
    assert_eq!(finding["failed_rule"], "all_active_writes_exist");
    assert_eq!(finding["environment"], "prod");
    assert_eq!(finding["artifact"], "worker_v1");
    assert_eq!(finding["artifact_version"], "worker_v1");
    assert_eq!(finding["witness"]["declared_capability"], "writes");
    assert!(
        finding["repair_candidates"]
            .as_array()
            .is_some_and(|candidates| !candidates.is_empty()),
        "{value}"
    );
}

#[test]
fn db_check_does_not_let_a_depth_bounded_kernel_pass_hide_a_write_drop_violation() {
    // Before the fix, this deep-migration-history variant escaped detection at
    // the hardcoded default kernel depth (8): the findings layer had no `writes`
    // branch, so `findings` stayed empty and the top-level `result` reported
    // `verified_under_assumptions` with exit 0 -- a confidently green false
    // negative. The findings layer is depth-independent, so this must be
    // caught unconditionally, without needing `--depth` at all.
    let writer_deep_history = fixture("issue_469_unsafe_drop_column_with_writer_deep_history.fsl");
    let (value, status) = run(&[
        "db",
        "check",
        writer_deep_history.to_str().expect("utf8 path"),
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated", "{value}");
    let findings = value["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "column_removed_while_still_written"),
        "{value}"
    );
}

#[test]
fn db_check_reads_control_is_unaffected_by_the_writes_branch() {
    // Negative control: the pre-existing `reads` branch must still be exactly
    // what it was before the fix, distinguished from `writes` findings by the
    // dedup key's `kind` field.
    let (value, status) = run(&["db", "check", READER_CONTROL]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated", "{value}");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    assert_eq!(findings[0]["kind"], "column_removed_while_still_read");
    assert_eq!(findings[0]["failed_rule"], "all_active_reads_exist");
}
