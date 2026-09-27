// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #1117: a `domain_event` row in a `fslc domain
//! replay` log must be checked against the event the model actually raised
//! at that point, not merely against the set of declared event names. Before
//! this fix, `command SetN` followed by `domain_event KindSet` — an event the
//! command's `decide` does not emit — returned `conformance_checked`/exit 0,
//! so `docs/DESIGN-domain.md`'s "the finite log matches the model" promise
//! was unbacked for every `domain_event` row.
//!
//! The matching rule under test: lowering rewrites the one-hot
//! `event_<Event>` flag of *every* declared event on *every* emitting
//! transition, so "the event the model just raised" is a fact the model
//! itself carries. A `domain_event` row conforms exactly when the flag for
//! its event is true in the Monitor state left by the most recent accepted
//! transition (`command` or `effect_completion`); `init` leaves every flag
//! false, so a row with no preceding transition never conforms.

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

const SPEC: &str = "rust/fslc/tests/fixtures/issue_518_domain_replay.fsl";

fn replay(logs: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "domain",
            "replay",
            SPEC,
            "--logs",
            &format!("rust/fslc/tests/fixtures/{logs}"),
        ])
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn findings(output: &Value) -> Vec<(&str, &str)> {
    output["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| {
            (
                finding["kind"].as_str().expect("finding.kind"),
                finding["failed_rule"].as_str().expect("failed_rule"),
            )
        })
        .collect()
}

/// The issue's first row: the command is accepted, but the logged event is
/// one its `decide` never emits. Declared-ness alone used to pass this.
#[test]
fn detects_an_event_the_preceding_command_does_not_emit() {
    let (output, status) = replay("issue_1117_event_not_emitted.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    assert_eq!(
        findings(&output),
        [(
            "unknown_domain_event",
            "runtime_event_emitted_by_preceding_transition"
        )],
        "{output:#}"
    );
    let witness = &output["findings"][0]["witness"];
    assert_eq!(witness["event"], "MsgCancelled");
    assert_eq!(
        witness["emitted_by_preceding_transition"],
        serde_json::json!(["MsgSent"]),
        "the witness must name what the model did raise, {output:#}"
    );
    assert_eq!(output["steps_checked"], 1);
}

/// The issue's second row: a `domain_event` with no preceding transition at
/// all. Every occurrence flag is still `init`'s `false`, so the log claims an
/// event the model never produced.
#[test]
fn detects_an_event_with_no_preceding_transition() {
    let (output, status) = replay("issue_1117_event_without_command.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant");
    assert_eq!(
        findings(&output),
        [(
            "unknown_domain_event",
            "runtime_event_emitted_by_preceding_transition"
        )],
        "{output:#}"
    );
    assert_eq!(
        output["findings"][0]["witness"]["emitted_by_preceding_transition"],
        serde_json::json!([]),
        "{output:#}"
    );
    assert_eq!(output["steps_checked"], 0);
}

/// Occurrence is one-hot and one-step: re-logging an earlier event after a
/// later command has stepped the model is a mismatch, not a replay of old
/// news.
#[test]
fn detects_a_stale_event_replayed_after_the_next_command() {
    let (output, status) = replay("issue_1117_stale_event_after_next_command.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(
        findings(&output),
        [(
            "unknown_domain_event",
            "runtime_event_emitted_by_preceding_transition"
        )],
        "{output:#}"
    );
    assert_eq!(
        output["findings"][0]["witness"]["emitted_by_preceding_transition"],
        serde_json::json!(["MsgCancelled"]),
        "{output:#}"
    );
}

/// An undeclared event name must keep its own `failed_rule`, so the two
/// rules sharing `kind:"unknown_domain_event"` stay distinguishable in the
/// finding (the same one-kind/many-rules shape
/// `uncorrelated_async_completion` already uses in this replay).
#[test]
fn an_undeclared_event_keeps_the_declaration_rule() {
    let (output, status) = replay("issue_1117_undeclared_event.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(
        findings(&output),
        [("unknown_domain_event", "runtime_event_declared_in_domain")],
        "{output:#}"
    );
}

/// Over-detection control 1/2: an outcome event raised by an
/// `effect_completion` — not by any command's `decide` — must still pass, so
/// the rule reads the model rather than reimplementing `decide ... emits`.
#[test]
fn an_effect_outcome_event_still_conforms() {
    let (output, status) = replay("issue_1117_effect_outcome_event.jsonl");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked");
    assert_eq!(output["findings"].as_array().expect("findings").len(), 0);
    assert_eq!(
        output["events_observed"],
        serde_json::json!(["Delivered", "MsgSent"]),
        "{output:#}"
    );
}

/// Over-detection control 2/2: the #518 clean log — the one the parity and
/// regression suites already pin — must keep reporting
/// `conformance_checked`/exit 0 with the same observed events.
#[test]
fn the_clean_log_still_conforms() {
    let (output, status) = replay("issue_518_clean.jsonl");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked");
    assert_eq!(output["findings"].as_array().expect("findings").len(), 0);
    assert_eq!(
        output["events_observed"],
        serde_json::json!(["Delivered", "MsgSent"]),
        "{output:#}"
    );
}
