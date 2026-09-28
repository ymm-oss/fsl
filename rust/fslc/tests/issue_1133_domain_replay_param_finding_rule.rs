// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Calibration for #1133: `fslc domain replay` reported a *type-mismatched
//! parameter* as a guard rejection.
//!
//! #1116 made a value that is not of its declared type fail closed, which
//! closed a false `conformance_checked`. But it folded the conversion
//! failure into the same bare `false` the model's own refusal produces, so
//! `{"value":"garbage"}` on an integer input came out as
//! `kind:"command_rejected_by_model"` /
//! `failed_rule:"runtime_command_must_be_enabled_by_domain_model"` with the
//! repair *"change the implementation command path or update the FSL
//! decide/evolve model"* — telling a user whose log is ill-typed to edit a
//! model that is right. The conversion's own message
//! (`parameter 'value' must be an integer`) was computed and discarded.
//!
//! The fix keeps the `kind` (the enum in
//! `schemas/fslc/domain/finding.v0.schema.json` is closed) and splits the
//! cause onto a second `failed_rule`, carrying the discarded message in
//! `witness.parameter_error` — the one-kind/many-rules shape
//! `uncorrelated_async_completion` and, since #1117, `unknown_domain_event`
//! already use on this path.
//!
//! Baselines. Every test below is calibrated against **the commit before
//! this one** (`cbaf0fbb`), where each of these logs reports the guard rule
//! and no `parameter_error`; the earlier **v4.7.0** binary disagrees again,
//! because it forced every parameter to `Int` and reported several of these
//! logs as `conformance_checked`. The controls
//! (`a_guard_rejection_keeps_the_model_enablement_rule`,
//! `a_lifecycle_mismatch_keeps_its_own_rule`, `a_well_typed_log_still_conforms`)
//! hold on all three and pin the split against over-detection.
//!
//! Two tests are negative controls in the sense #1123 introduced here: they
//! assert what the repair text must **not** say, because the defect was
//! never a missing message but a confidently wrong one.

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

const PARAM_SPEC: &str = "rust/fslc/tests/fixtures/issue_1116_domain_replay_param_types.fsl";
const CORRELATION_SPEC: &str = "rust/fslc/tests/fixtures/issue_1116_declared_correlation.fsl";
const LIFECYCLE_SPEC: &str = "rust/fslc/tests/fixtures/issue_518_domain_replay.fsl";
/// The shipped example the identity-type and guard-rejection fixtures are
/// written against: `PaymentRequestId` there is an *implicit* identity type,
/// and `RequestPaymentCapture` has a guard a log can fail honestly.
const ORDER_SPEC: &str = "examples/domain/order_async_effect.fsl";

fn replay(spec: &str, logs: &str) -> (Value, i32) {
    run(&[
        "domain",
        "replay",
        spec,
        "--logs",
        &format!("rust/fslc/tests/fixtures/{logs}"),
    ])
}

/// The single finding of a one-finding nonconformant replay.
fn only_finding(spec: &str, logs: &str) -> Value {
    let (output, status) = replay(spec, logs);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{output:#}");
    findings[0].clone()
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
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The headline: a value that is not of its declared type is not a guard
/// rejection, and the envelope says so. Calibration: `cbaf0fbb` reports
/// `runtime_command_must_be_enabled_by_domain_model` here.
#[test]
fn a_type_mismatch_takes_its_own_failed_rule() {
    let finding = only_finding(PARAM_SPEC, "issue_1116_int_input_garbage_string.jsonl");
    assert_eq!(finding["kind"], "command_rejected_by_model", "{finding:#}");
    assert_eq!(
        finding["failed_rule"], "runtime_command_parameters_match_declared_types",
        "{finding:#}"
    );
}

/// Option 2 of the issue: the conversion computed a message and threw it
/// away. Calibration: `cbaf0fbb`'s witness is `{"log": …}` with no
/// `parameter_error` at all.
#[test]
fn the_discarded_conversion_message_reaches_the_witness() {
    let finding = only_finding(PARAM_SPEC, "issue_1116_int_input_garbage_string.jsonl");
    assert_eq!(
        finding["witness"]["parameter_error"], "parameter 'value' must be an integer",
        "{finding:#}"
    );
    assert!(
        finding["witness"]["log"].is_object(),
        "the log row the finding is about must stay in the witness: {finding:#}"
    );
}

/// The acceptance criterion stated as one test: the two logs differ in the
/// envelope, not only in the prose. Both keep the same `kind`, because the
/// schema's `kind` enum is closed and this change does not touch it.
#[test]
fn a_type_mismatch_and_a_guard_rejection_differ_in_the_envelope() {
    let mismatch = only_finding(PARAM_SPEC, "issue_1116_int_input_garbage_string.jsonl");
    let rejection = only_finding(ORDER_SPEC, "issue_1116_guard_rejected_command.jsonl");
    assert_eq!(mismatch["kind"], rejection["kind"], "kind is unchanged");
    assert_ne!(
        mismatch["failed_rule"], rejection["failed_rule"],
        "the two causes must not share a failed_rule: {mismatch:#} / {rejection:#}"
    );
    assert!(
        rejection["witness"].get("parameter_error").is_none(),
        "a guard rejection converted every parameter, so it has no conversion error: {rejection:#}"
    );
}

/// Negative control. The repair the defect produced named the model's
/// transition rules; nothing about them was consulted for this row, so
/// naming them sends the reader to edit a correct `decide`/`evolve`.
#[test]
fn the_type_mismatch_repair_must_not_ask_for_a_transition_rule_change() {
    let finding = only_finding(PARAM_SPEC, "issue_1116_int_input_garbage_string.jsonl");
    let repair = repair_text(&finding);
    assert!(
        !repair.contains(
            "change the implementation command path or update the FSL decide/evolve model"
        ),
        "the guard-rejection repair must not be printed for a conversion failure: {repair}"
    );
    assert!(
        !repair.contains("decide") && !repair.contains("evolve"),
        "no repair may point at the transition rules, which were never evaluated: {repair}"
    );
    assert!(
        repair.contains("parameter 'value' must be an integer"),
        "the repair must carry the reason the value was refused: {repair}"
    );
    assert!(
        repair.contains("command Account.SetScore"),
        "the repair must name the row it is about: {repair}"
    );
}

/// The same split for the other two conversion failures #1116 introduced: a
/// `Bool` input spelled as an integer, and an enum member that does not
/// exist. Calibration: `cbaf0fbb` reports the guard rule for both.
#[test]
fn every_conversion_failure_takes_the_signature_rule() {
    for (spec, logs) in [
        (PARAM_SPEC, "issue_1116_bool_input_integer_one.jsonl"),
        (PARAM_SPEC, "issue_1116_enum_input_unknown_member.jsonl"),
        (PARAM_SPEC, "issue_1116_int_input_object.jsonl"),
        (ORDER_SPEC, "issue_1116_identity_input_null.jsonl"),
        (ORDER_SPEC, "issue_1116_identity_input_object.jsonl"),
    ] {
        let finding = only_finding(spec, logs);
        assert_eq!(
            finding["failed_rule"], "runtime_command_parameters_match_declared_types",
            "{logs}: {finding:#}"
        );
        assert!(
            finding["witness"]["parameter_error"].is_string(),
            "{logs}: {finding:#}"
        );
    }
}

/// A parameter the command does not declare is the same disagreement — the
/// log does not match the declared signature — and the monitor never saw it
/// either. Calibration: rejected on every binary, but under the guard rule
/// before this commit.
#[test]
fn an_undeclared_parameter_takes_the_signature_rule() {
    let finding = only_finding(PARAM_SPEC, "issue_1116_undeclared_param.jsonl");
    assert_eq!(
        finding["failed_rule"], "runtime_command_parameters_match_declared_types",
        "{finding:#}"
    );
    assert_eq!(
        finding["witness"]["parameter_error"],
        "action 'account_set_score' has no parameter 'extra'",
        "{finding:#}"
    );
}

/// Over-detection control: a command the model refuses on its guard, with
/// every parameter converting cleanly, keeps the rule and the repair it has
/// always had. Holds on v4.7.0, `cbaf0fbb`, and here.
#[test]
fn a_guard_rejection_keeps_the_model_enablement_rule() {
    let finding = only_finding(ORDER_SPEC, "issue_1116_guard_rejected_command.jsonl");
    assert_eq!(
        finding["failed_rule"], "runtime_command_must_be_enabled_by_domain_model",
        "{finding:#}"
    );
    assert_eq!(
        repair_text(&finding),
        "change the implementation command path or update the FSL decide/evolve model",
        "{finding:#}"
    );
}

/// The `effect_completion` branch had the same collapse, onto
/// `effect_completion_matches_pending_lifecycle` — a rule about *ordering*,
/// printed for a value that never reached the lifecycle. Calibration:
/// `cbaf0fbb` reports the lifecycle rule here.
#[test]
fn an_effect_completion_type_mismatch_is_not_a_lifecycle_mismatch() {
    for logs in [
        "issue_1116_correlation_string_via_params.jsonl",
        "issue_1116_correlation_string_via_correlation_id.jsonl",
    ] {
        let finding = only_finding(CORRELATION_SPEC, logs);
        assert_eq!(
            finding["kind"], "effect_completion_rejected_by_model",
            "{logs}: {finding:#}"
        );
        assert_eq!(
            finding["failed_rule"], "effect_completion_parameters_match_declared_types",
            "{logs}: {finding:#}"
        );
        assert_eq!(
            finding["witness"]["parameter_error"], "parameter 'ref_id' must be an integer",
            "{logs}: {finding:#}"
        );
        assert_eq!(finding["effect"], "Deliver", "{logs}: {finding:#}");
    }
}

/// Negative control for the completion branch: the lifecycle repair must not
/// be printed for a conversion failure, and neither must a transition-rule
/// edit.
#[test]
fn the_completion_type_mismatch_repair_must_not_blame_the_lifecycle() {
    let finding = only_finding(
        CORRELATION_SPEC,
        "issue_1116_correlation_string_via_params.jsonl",
    );
    let repair = repair_text(&finding);
    assert!(
        !repair.contains("ordering matches the fsl-effect lifecycle"),
        "the ordering is not what disagrees: {repair}"
    );
    assert!(
        !repair.contains("decide") && !repair.contains("evolve"),
        "no repair may point at the transition rules: {repair}"
    );
    assert!(
        repair.contains("effect_completion Deliver/Delivered"),
        "the repair must name the row it is about: {repair}"
    );
}

/// Over-detection control: a completion the model refuses because the
/// request lifecycle really does not allow it keeps the lifecycle rule.
/// Holds on v4.7.0, `cbaf0fbb`, and here.
#[test]
fn a_lifecycle_mismatch_keeps_its_own_rule() {
    let finding = only_finding(LIFECYCLE_SPEC, "issue_518_lifecycle_mismatch.jsonl");
    assert_eq!(
        finding["kind"], "effect_completion_rejected_by_model",
        "{finding:#}"
    );
    assert_eq!(
        finding["failed_rule"], "effect_completion_matches_pending_lifecycle",
        "{finding:#}"
    );
    assert!(
        finding["witness"].get("parameter_error").is_none(),
        "{finding:#}"
    );
}

/// Over-detection control: a well-typed log still reports nothing, on both
/// specs. Holds on `cbaf0fbb` and here.
#[test]
fn a_well_typed_log_still_conforms() {
    for (spec, logs) in [
        (PARAM_SPEC, "issue_1116_int_input_ok.jsonl"),
        (CORRELATION_SPEC, "issue_1116_correlation_number.jsonl"),
        (LIFECYCLE_SPEC, "issue_518_clean.jsonl"),
    ] {
        let (output, status) = replay(spec, logs);
        assert_eq!(status, 0, "{logs}: {output:#}");
        assert_eq!(
            output["result"], "conformance_checked",
            "{logs}: {output:#}"
        );
    }
}

/// Over-detection control on the shipped corpus the issue names: the async
/// effect example replays clean, unchanged.
#[test]
fn the_shipped_async_effect_log_still_conforms() {
    let (output, status) = run(&[
        "domain",
        "replay",
        "examples/domain/order_async_effect.fsl",
        "--logs",
        "examples/domain/order_async_effect_replay.jsonl",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
}
