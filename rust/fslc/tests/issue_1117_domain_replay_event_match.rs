// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #1117: a `domain_event` row in a `fslc domain
//! replay` log must be checked against the event the model actually raised
//! at that point, not merely against the set of declared event names. Before
//! this fix, `command SetN` followed by `domain_event KindSet` — an event the
//! command's `decide` does not emit — returned `conformance_checked`/exit 0,
//! so the promise in `docs/DESIGN-domain.md` that the finite log matches the
//! model was unbacked for every `domain_event` row. (Unquoted on purpose:
//! `tools/check-design-citation-headings.py` reads a quoted phrase next to a
//! DESIGN path as a section citation, and this one names a promise in the
//! prose, not a heading.)
//!
//! The matching rule under test: lowering rewrites the one-hot
//! `event_<Event>` flag of *every* declared event on *every* emitting
//! transition, so "the event the model just raised" is a fact the model
//! itself carries. A `domain_event` row conforms exactly when the flag for
//! its event is true in the Monitor state left by the most recent accepted
//! transition (`command` or `effect_completion`); `init` leaves every flag
//! false, so a row with no preceding transition never conforms.
//!
//! The PR #1123 review added the second half: the *same* mismatch has three
//! causes, and the finding has to tell them apart. A rejected preceding
//! transition raises nothing (so the emitting command is right there in the
//! log), an event only a saga step/timeout/compensation emits can never be
//! matched by any log (so the model is not the thing to change), and only
//! the remaining case is an ordering or model gap. Telling the second case
//! to "add the event to the emitting decide/effect" is an instruction to
//! break a correct model, so the tests below pin what the repair must not
//! say as well as what it must.

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
/// The example the #1123 review replayed against. It is the shipped example
/// that carries all three unsteppable saga constructs at once — a step, a
/// step timeout, and a compensation — so the classification is pinned
/// against a spec users actually read, not a fixture written to suit it.
const SAGA_SPEC: &str = "examples/domain/order_fulfillment_saga.fsl";

fn replay(logs: &str) -> (Value, i32) {
    replay_spec(SPEC, logs)
}

fn replay_spec(spec: &str, logs: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "domain",
            "replay",
            spec,
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

fn repair_text(finding: &Value) -> String {
    finding["repair_candidates"]
        .as_array()
        .expect("repair_candidates array")
        .iter()
        .map(|candidate| {
            candidate["description"]
                .as_str()
                .expect("repair description")
        })
        .collect::<Vec<_>>()
        .join(" | ")
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

/// #1123 review, 1/2: a `command` the model *rejected* leaves the occurrence
/// flags where the last accepted transition left them, so the event row after
/// it is a mismatch — but the command that emits that event is right there in
/// the log. "Record it only after a command whose decide emits it" would be
/// pointing at the row above, and "add it to the emitting decide" would be
/// telling the reader to edit a `decide` that already emits it. The repair
/// must name the rejection instead.
#[test]
fn a_rejected_command_is_named_instead_of_blaming_the_model() {
    let (output, status) = replay("issue_1117_event_after_rejected_command.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(
        findings(&output),
        [
            (
                "command_rejected_by_model",
                "runtime_command_must_be_enabled_by_domain_model"
            ),
            (
                "unknown_domain_event",
                "runtime_event_emitted_by_preceding_transition"
            )
        ],
        "{output:#}"
    );
    let finding = &output["findings"][1];
    assert_eq!(
        finding["witness"]["rejected_preceding_transition"], "command Msg.CancelMsg",
        "the witness must name the rejected transition, {output:#}"
    );
    assert_eq!(
        finding["witness"]["emitting_model_constructs"],
        serde_json::json!(["decide Msg.CancelMsg"]),
        "{output:#}"
    );
    let repair = repair_text(finding);
    assert!(
        repair.contains("the model rejected the preceding command Msg.CancelMsg"),
        "the repair must point at the rejection, not at the model: {repair}"
    );
    assert!(
        repair.contains("do not add it to the model"),
        "the repair must not ask for a model change that would break a correct model: {repair}"
    );
    assert!(
        !repair.contains("only after a command whose decide emits it"),
        "the pre-#1123 wording contradicts the log it is printed against: {repair}"
    );
}

/// #1123 review, 2/2: an event only a saga step emits can never have its flag
/// raised by a replayed log, because `command`/`effect_completion` are the
/// only row kinds that step the Monitor. The finding must say that the model
/// is *not* the thing to change, and carries its own `failed_rule` so a
/// consumer can separate "log out of order" from "log unmatchable by
/// construction".
#[test]
fn a_saga_emitted_event_does_not_ask_for_a_model_change() {
    let (output, status) = replay_spec(SAGA_SPEC, "issue_1117_saga_emitted_event.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(
        findings(&output),
        [(
            "unknown_domain_event",
            "runtime_event_reachable_by_replayed_transition"
        )],
        "{output:#}"
    );
    let finding = &output["findings"][0];
    assert_eq!(
        finding["witness"]["emitting_model_constructs"],
        serde_json::json!(["saga OrderFulfillment step ReserveInventory"]),
        "{output:#}"
    );
    assert_eq!(
        finding["witness"]["reachable_by_replayed_transition"],
        false
    );
    let repair = repair_text(finding);
    assert!(
        repair.contains("raised only by saga OrderFulfillment step ReserveInventory"),
        "the repair must name the saga step: {repair}"
    );
    assert!(
        repair.contains("must not be changed"),
        "the repair must say the model is correct here: {repair}"
    );
    assert!(
        !repair.contains("add InventoryReservationRequested to"),
        "no repair may propose breaking the correct model: {repair}"
    );
}

/// A saga *compensation* and a step *timeout* reach the same conclusion by a
/// different construct, so the classification is not a one-off match on
/// `step`.
#[test]
fn saga_compensation_and_timeout_events_are_classified_the_same_way() {
    for (logs, log_event, construct) in [
        (
            "issue_1117_saga_compensation_event.jsonl",
            "InventoryReleaseRequested",
            "saga OrderFulfillment compensation PaymentFailed after InventoryReserved",
        ),
        (
            "issue_1117_saga_timeout_event.jsonl",
            "PaymentCaptureTimedOut",
            "saga OrderFulfillment step CapturePayment timeout",
        ),
    ] {
        let (output, status) = replay_spec(SAGA_SPEC, logs);
        assert_eq!(status, 1, "{output:#}");
        assert_eq!(
            findings(&output),
            [(
                "unknown_domain_event",
                "runtime_event_reachable_by_replayed_transition"
            )],
            "{output:#}"
        );
        let constructs = output["findings"][0]["witness"]["emitting_model_constructs"]
            .as_array()
            .expect("emitting_model_constructs")
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        assert!(
            constructs.iter().any(|name| name == construct),
            "{constructs:?} must name {construct}, {output:#}"
        );
        assert!(
            !repair_text(&output["findings"][0]).contains(&format!("add {log_event} to")),
            "{output:#}"
        );
    }
}
