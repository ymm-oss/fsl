// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #518: `fslc domain replay` must actually drive a
//! Monitor over the lowered domain/effect model instead of only tracking a
//! `(effect, correlation_id)` bookkeeping set. Each of the four detection
//! categories `docs/DESIGN-domain.md`/`docs/DESIGN-effect.md` promise must
//! independently produce `result:"nonconformant"`/exit 1 with the
//! documented finding `kind` — proving detection, not just a new field
//! existing (the class of thing #468 already established: "the detector
//! exists but detects nothing" is a defect in its own right).

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

const SPEC: &str = "rust/fslc/tests/fixtures/issue_518_domain_replay.fsl";

fn finding_kinds(output: &Value) -> Vec<&str> {
    output["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| finding["kind"].as_str().expect("finding.kind"))
        .collect()
}

fn replay(logs: &str) -> (Value, i32) {
    run(&[
        "domain",
        "replay",
        SPEC,
        "--logs",
        &format!("rust/fslc/tests/fixtures/{logs}"),
    ])
}

/// Detection category 1/4: a command the model's own guard rejects
/// (`SendMsg` twice in a row) must surface as `command_rejected_by_model`,
/// not be silently accepted.
#[test]
fn detects_a_rejected_command() {
    let (output, status) = replay("issue_518_rejected_command.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    assert_eq!(finding_kinds(&output), ["command_rejected_by_model"]);
    assert_eq!(output["steps_checked"], 2);
}

/// Detection category 2/4 (the one lane that already worked before this
/// fix): a completion with no prior request must still be flagged.
#[test]
fn detects_a_completion_without_a_prior_request() {
    let (output, status) = replay("issue_518_completion_without_request.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    let kinds = finding_kinds(&output);
    assert!(
        kinds.contains(&"uncorrelated_async_completion"),
        "{output:#}"
    );
}

/// Detection category 3/4: an irreversible effect committing twice for the
/// same correlation id must be flagged, even though the pending set is
/// legitimately refilled by an intervening `effect_request`.
#[test]
fn detects_a_duplicate_irreversible_effect_commit() {
    let (output, status) = replay("issue_518_duplicate_irreversible.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    assert!(
        finding_kinds(&output).contains(&"duplicate_irreversible_effect_commit"),
        "{output:#}"
    );
}

/// Detection category 4/4: a completion observed after the aggregate moved
/// into a state the model's own `evolve ... requires` rejects (delivery
/// completing after the message was cancelled) must be flagged.
#[test]
fn detects_a_lifecycle_ordering_mismatch() {
    let (output, status) = replay("issue_518_lifecycle_mismatch.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    assert!(
        finding_kinds(&output).contains(&"effect_completion_rejected_by_model"),
        "{output:#}"
    );
}

/// Every finding must satisfy the published fsl-domain finding schema:
/// `schema_version`, `fsl`, `result`, `kind`, `severity`, `domain`,
/// `failed_rule`, `guarantee_kind`, `evidence`, `witness`,
/// `repair_candidates`, `assumptions` all present with the right shapes.
#[test]
fn findings_satisfy_the_published_finding_schema_shape() {
    let (output, _status) = replay("issue_518_rejected_command.jsonl");
    let finding = &output["findings"][0];
    assert_eq!(finding["schema_version"], "fsl-domain-finding.v0");
    assert_eq!(finding["fsl"], "fsl-domain-effect.v0");
    assert_eq!(finding["result"], "violated");
    assert_eq!(finding["severity"], "error");
    assert!(finding["failed_rule"].is_string());
    assert_eq!(finding["guarantee_kind"], "runtime_observed");
    assert_eq!(finding["evidence"]["kind"], "runtime_replay");
    assert_eq!(finding["evidence"]["formal_proof"], false);
    assert!(finding["witness"].is_object());
    assert!(finding["repair_candidates"].is_array());
    assert!(finding["assumptions"].is_array());
}

/// Regression control: a conformant log over the same model must still
/// report `conformance_checked`/exit 0, so driving the Monitor does not
/// over-trigger on the success path.
#[test]
fn replay_still_reports_conformance_checked_for_a_clean_log() {
    let (output, status) = replay("issue_518_clean.jsonl");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked");
    assert_eq!(output["findings"].as_array().unwrap().len(), 0);
    assert_eq!(output["steps_checked"], 2);
}

/// `final_state` and `assumptions` must be populated from the same Monitor
/// and assumption computation `domain analyze` already uses, not `{}`/`[]`.
#[test]
fn replay_populates_final_state_and_assumptions() {
    let (output, _status) = replay("issue_518_clean.jsonl");
    let final_state = output["final_state"].as_object().expect("final_state");
    assert!(!final_state.is_empty(), "{output:#}");

    let (analyze, _status) = run(&["domain", "analyze", SPEC]);
    assert_eq!(output["assumptions"], analyze["assumptions"]);
}
