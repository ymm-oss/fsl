// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Calibration for #1116: `fslc domain replay` coerced *every* runtime-log
//! parameter to `Int` and turned anything it could not parse into `0`. That
//! is a soundness defect in two directions at once:
//!
//! * a value that is not of the declared type was accepted as `0`, so
//!   `{"value":"garbage"}` on an integer input replayed as `value = 0` and
//!   the run reported `conformance_checked`/exit 0 — false conformance, and
//!   the evidence class `docs/DESIGN-assurance-classes.md` attaches to
//!   `domain replay` was therefore overstated;
//! * an enum or `Bool` input could never be replayed at all, because the
//!   forced `Int` never belongs to the parameter's declared domain, so every
//!   such command surfaced as `command_rejected_by_model` regardless of what
//!   the log said.
//!
//! Each test below fails on the pre-fix binary (the first group by reporting
//! `conformance_checked`, the second by reporting `command_rejected_by_model`)
//! and the negative controls keep the fix from over-detecting.

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

const SPEC: &str = "rust/fslc/tests/fixtures/issue_1116_domain_replay_param_types.fsl";

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

fn assert_rejected(logs: &str) {
    let (output, status) = replay(logs);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(finding_kinds(&output), ["command_rejected_by_model"]);
}

fn assert_conformant(logs: &str) -> Value {
    let (output, status) = replay(logs);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
    assert!(finding_kinds(&output).is_empty(), "{output:#}");
    output
}

/// The headline false conformance: a string that is not an integer must not
/// replay as `0`. Before the fix this reported `conformance_checked`/exit 0
/// with `account_score = 0`.
#[test]
fn a_non_numeric_string_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_garbage_string.jsonl");
}

/// Same defect through a JSON object rather than a string.
#[test]
fn an_object_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_object.jsonl");
}

/// A parameter the target action does not declare is a log that does not
/// match the model, not a parameter to invent a value for.
#[test]
fn an_undeclared_parameter_is_not_accepted() {
    assert_rejected("issue_1116_undeclared_param.jsonl");
}

/// An enum input written with the domain-level member name must replay.
/// Before the fix it became `Int(0)`, which never belongs to the declared
/// enum domain, so it always surfaced as `command_rejected_by_model`.
#[test]
fn an_enum_input_replays_from_its_domain_member_name() {
    let output = assert_conformant("issue_1116_enum_input_bare_member.jsonl");
    assert_eq!(output["final_state"]["account_tier"], "Tier_Premium");
}

/// The Kernel spelling the lowered model itself uses is accepted too, so a
/// log written against either vocabulary replays.
#[test]
fn an_enum_input_replays_from_its_kernel_member_name() {
    let output = assert_conformant("issue_1116_enum_input_kernel_member.jsonl");
    assert_eq!(output["final_state"]["account_tier"], "Tier_Premium");
}

/// And the qualified `Type.Member` spelling.
#[test]
fn an_enum_input_replays_from_its_qualified_member_name() {
    let output = assert_conformant("issue_1116_enum_input_qualified_member.jsonl");
    assert_eq!(output["final_state"]["account_tier"], "Tier_Premium");
}

/// Negative control for the three spellings above: a token that is not a
/// member of the declared enum is still rejected.
#[test]
fn an_unknown_enum_member_is_not_accepted() {
    assert_rejected("issue_1116_enum_input_unknown_member.jsonl");
}

/// A `Bool` input must replay as `true`, not as the `Int(1)` that the old
/// coercion produced and that no `Bool` domain contains.
#[test]
fn a_bool_input_replays_as_a_boolean() {
    let output = assert_conformant("issue_1116_bool_input_true.jsonl");
    assert_eq!(output["final_state"]["account_active"], true);
}

/// Negative control for the `Bool` lane.
#[test]
fn a_non_boolean_string_is_not_accepted_for_a_bool_input() {
    assert_rejected("issue_1116_bool_input_garbage_string.jsonl");
}

/// Over-detection control: a well-typed integer input still replays.
#[test]
fn a_well_typed_integer_input_still_replays() {
    let output = assert_conformant("issue_1116_int_input_ok.jsonl");
    assert_eq!(output["final_state"]["account_score"], 3);
}

/// Over-detection control on the shipped corpus: `order_async_effect`'s log
/// carries `"payment_request_id":"p1"` for `PaymentRequestId`, an *implicit*
/// identity type the domain never declares. `lower_domain` synthesizes it as
/// an `external` placeholder domain, so an opaque runtime token keeps the
/// documented placeholder mapping and the example stays conformant.
#[test]
fn the_shipped_async_effect_example_stays_conformant() {
    let (output, status) = run(&[
        "domain",
        "replay",
        "examples/domain/order_async_effect.fsl",
        "--logs",
        "examples/domain/order_async_effect_replay.jsonl",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
    assert!(finding_kinds(&output).is_empty(), "{output:#}");
}

/// Over-detection control on the guard lane: a command the model's own guard
/// rejects (`RequestPaymentCapture` before `ApproveOrder`) must still be
/// `command_rejected_by_model`, so the fix does not relabel guard rejections.
#[test]
fn a_guard_rejected_command_is_still_reported_as_rejected() {
    let (output, status) = run(&[
        "domain",
        "replay",
        "examples/domain/order_async_effect.fsl",
        "--logs",
        "rust/fslc/tests/fixtures/issue_1116_guard_rejected_command.jsonl",
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(finding_kinds(&output), ["command_rejected_by_model"]);
}
