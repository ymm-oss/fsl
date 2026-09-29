// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Calibration for #1116: `fslc domain replay` coerced *every* runtime-log
//! parameter to `Int` and turned anything it could not parse into `0`. That
//! is a soundness defect in two directions at once:
//!
//! * a value that is not of the declared type was accepted as `0`, so
//!   `{"value":"garbage"}` on an integer input replayed as `value = 0` and
//!   the run reported `conformance_checked`/exit 0 — false conformance, and
//!   the evidence class `docs/design/DESIGN-assurance-classes.md` attaches to
//!   `domain replay` was therefore overstated;
//! * an enum or `Bool` input could never be replayed at all, because the
//!   forced `Int` never belongs to the parameter's declared domain, so every
//!   such command surfaced as `command_rejected_by_model` regardless of what
//!   the log said.
//!
//! Review of the first fix found three more disagreements of the same
//! shape. An *implicit identity* parameter still folded an object, an
//! array, and `null` onto its placeholder — the false conformance, left
//! open for the parameter shape real logs carry most. Routing `Bool`
//! through `parse_param_value` inherited that function's integer spelling,
//! so `{"flag":1}` newly replayed as `true`. And the correlation id filled
//! in when `params` omits the correlation field was converted from the
//! stringified pairing key, so against a declared numeric type one value
//! got two verdicts depending on which field carried it.
//!
//! Each test states the binary it is calibrated against in its own doc
//! comment, because the three baselines disagree:
//!
//! * **v4.7.0** (`fslc 4.7.0`, the last release) — every value forced to
//!   `Int`;
//! * **the first #1116 commit** — declared types honoured, identity types
//!   and `Bool` not yet;
//! * **here**.
//!
//! Two tests are pure controls that hold on all three: an undeclared
//! parameter (`an_undeclared_parameter_is_not_accepted`, rejected by the
//! monitor on every binary because the action has no such parameter to
//! bind) and a guard-rejected command. The remaining controls keep each
//! narrowing from over-detecting.

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
/// replay as `0`. Calibration: v4.7.0 reports `conformance_checked`/exit 0
/// here, with `account_score = 0`.
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
///
/// Control, not calibration: this is rejected on v4.7.0 too, which never
/// bound the extra parameter to anything the action declares. It is here so
/// the fail-closed path has a test, not because it distinguishes binaries.
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

// ---------------------------------------------------------------------------
// Declared numeric inputs: the four remaining JSON shapes v4.7.0 accepted.
//
// These are the *breaking* half of #1116. Each one replayed as a number on
// v4.7.0 -- `"2"` and `2.0` as 2, `true` as 1, `null` as 0 -- all inside
// `Score`'s `0..3` domain, so all four reported `conformance_checked`/exit 0.
// A declared numeric parameter now takes a JSON integer and nothing else.
// ---------------------------------------------------------------------------

/// Calibration: v4.7.0 parsed the string `"2"` and replayed `value = 2`.
#[test]
fn a_numeric_string_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_numeric_string.jsonl");
}

/// Calibration: v4.7.0 truncated the float `2.0` and replayed `value = 2`.
#[test]
fn a_float_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_float.jsonl");
}

/// Calibration: v4.7.0 read `true` as 1 and replayed `value = 1`.
#[test]
fn a_boolean_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_bool.jsonl");
}

/// Calibration: v4.7.0 read `null` as its 0 default and replayed
/// `value = 0` -- an observation the log does not contain at all.
#[test]
fn a_null_is_not_accepted_for_an_integer_input() {
    assert_rejected("issue_1116_int_input_null.jsonl");
}

// ---------------------------------------------------------------------------
// Implicit identity inputs: the placeholder is for opaque *tokens*.
//
// `PaymentRequestId` in `examples/domain/order_async_effect.fsl` is only
// referenced, never declared, so `lower_domain` synthesizes it as an
// `external` placeholder domain and a token such as `"p1"` has no declared
// numeric meaning. That exception is about numbers, not about structure: an
// object, an array, and `null` are not identifiers, and folding them onto
// the placeholder reported `conformance_checked`/exit 0 on **both** v4.7.0
// and the first #1116 commit -- the same false conformance that fix closed
// for every declared type, left open for the parameter shape most real logs
// carry.
// ---------------------------------------------------------------------------

fn assert_order_rejected(logs: &str) {
    let (output, status) = run(&[
        "domain",
        "replay",
        "examples/domain/order_async_effect.fsl",
        "--logs",
        &format!("rust/fslc/tests/fixtures/{logs}"),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(finding_kinds(&output), ["command_rejected_by_model"]);
}

/// Calibration: `conformance_checked`/exit 0 on v4.7.0 *and* on the first
/// #1116 commit.
#[test]
fn an_object_is_not_accepted_for_an_identity_input() {
    assert_order_rejected("issue_1116_identity_input_object.jsonl");
}

/// Calibration: `conformance_checked`/exit 0 on v4.7.0 *and* on the first
/// #1116 commit.
#[test]
fn an_array_is_not_accepted_for_an_identity_input() {
    assert_order_rejected("issue_1116_identity_input_array.jsonl");
}

/// Calibration: `conformance_checked`/exit 0 on v4.7.0 *and* on the first
/// #1116 commit.
#[test]
fn a_null_is_not_accepted_for_an_identity_input() {
    assert_order_rejected("issue_1116_identity_input_null.jsonl");
}

/// Over-detection control for the three above: the opaque token the
/// placeholder exists for still replays.
#[test]
fn an_opaque_token_still_replays_for_an_identity_input() {
    let (output, status) = run(&[
        "domain",
        "replay",
        "examples/domain/order_async_effect.fsl",
        "--logs",
        "rust/fslc/tests/fixtures/issue_1116_identity_input_token.jsonl",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
    assert!(finding_kinds(&output).is_empty(), "{output:#}");
}

// ---------------------------------------------------------------------------
// `Bool` inputs are read from `true`/`false` only.
//
// `parse_param_value` also reads an integer as `v != 0`, for `fslc replay`'s
// hand-written mapped-action inputs. Routing `domain replay` through it
// inherited that, so the first #1116 commit accepted `{"flag":1}` as
// `active = true` -- a relaxation inside a tightening, on the evidence path
// where the log is someone else's record and `docs/design/DESIGN-domain.md` spells
// a Boolean `true`/`false`.
// ---------------------------------------------------------------------------

/// Calibration: `conformance_checked`/exit 0 with `account_active = true` on
/// the first #1116 commit (v4.7.0 rejected it, for the unrelated reason that
/// it forced every value to `Int`).
#[test]
fn the_integer_one_is_not_accepted_for_a_bool_input() {
    assert_rejected("issue_1116_bool_input_integer_one.jsonl");
}

/// Same for the falsy spelling, so the narrowing is about the *type* and not
/// about which integer was written.
#[test]
fn the_integer_zero_is_not_accepted_for_a_bool_input() {
    assert_rejected("issue_1116_bool_input_integer_zero.jsonl");
}

// ---------------------------------------------------------------------------
// The correlation-id path and the `params` path decide alike.
//
// When an `effect_completion` omits the correlation field from its `params`,
// replay fills it in from the entry's `correlation_id`. The first #1116
// commit converted that through the *stringified* correlation value and
// re-parsed it as an integer first, so against a declared numeric type the
// JSON string `"1"` was accepted through `correlation_id` and rejected
// through `params` -- the same value, two verdicts. Both now read the JSON
// the log actually wrote.
// ---------------------------------------------------------------------------

const CORRELATION_SPEC: &str = "rust/fslc/tests/fixtures/issue_1116_declared_correlation.fsl";

fn replay_correlation(logs: &str) -> (Value, i32) {
    run(&[
        "domain",
        "replay",
        CORRELATION_SPEC,
        "--logs",
        &format!("rust/fslc/tests/fixtures/{logs}"),
    ])
}

/// Calibration: `conformance_checked`/exit 0 on v4.7.0 *and* on the first
/// #1116 commit, where the string was re-parsed into a number before
/// conversion.
#[test]
fn a_numeric_string_correlation_id_is_not_accepted_for_a_declared_numeric_field() {
    let (output, status) =
        replay_correlation("issue_1116_correlation_string_via_correlation_id.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(
        finding_kinds(&output),
        ["effect_completion_rejected_by_model"]
    );
}

/// The same value through `params`, which the first #1116 commit already
/// rejected. The two must agree; this pins the direction they were aligned
/// in (strict, matching `params`).
#[test]
fn a_numeric_string_in_params_is_not_accepted_for_a_declared_numeric_field() {
    let (output, status) = replay_correlation("issue_1116_correlation_string_via_params.jsonl");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "nonconformant", "{output:#}");
    assert_eq!(
        finding_kinds(&output),
        ["effect_completion_rejected_by_model"]
    );
}

/// Over-detection control: a correlation id the log writes as a JSON number
/// still fills a declared numeric field, so aligning on the strict side did
/// not cost the well-typed case.
#[test]
fn a_numeric_correlation_id_still_fills_a_declared_numeric_field() {
    let (output, status) = replay_correlation("issue_1116_correlation_number.jsonl");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "conformance_checked", "{output:#}");
    assert!(finding_kinds(&output).is_empty(), "{output:#}");
}
