// SPDX-License-Identifier: Apache-2.0

//! #1439: unary operators must check operand types even when concrete replay
//! skips the operand. Seven detectors and three well-typed preservation controls.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// Keep the invariant at 10:3, the step at 12:5 and expect at 13:5.
fn source(invariant: &str, trace: Option<(&str, &str, &str)>) -> String {
    let trace = trace.map_or_else(String::new, |(kind, step, expectation)| {
        format!(
            "  {kind} CASE-1 \"unary control\" {{\n    {step}\n    expect {expectation}\n  }}\n"
        )
    });
    format!(
        r#"requirements Unary {{
  enum Color {{ Red, Blue }}
  type N = -3..3
  state {{ c: Color, n: N }}
  init {{ c = Red n = 1 }}
  requirement REQ-1 "unary operands" {{
    action paint(x: Color) {{ requires x != c c = x }}
    action setn(v: N) {{ n = v }}
  }}
  invariant I {{ {invariant} }}
{trace}}}
"#
    )
}

fn check(label: &str, source: &str) -> (Value, i32) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1439_unary_operand_types")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create per-test unary fixture directory");
    std::fs::write(directory.join("unary.fsl"), source).expect("write inline unary fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "unary.fsl"])
        .current_dir(&directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .output()
        .expect("run native unary operand check");
    assert!(
        output.stderr.is_empty(),
        "{label}: unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: invalid JSON ({error}): stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (envelope, output.status.code().expect("native exit status"))
}

fn assert_type_error(output: &Value, status: i32, mismatch: &str, line: u32, column: u32) {
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{output:#}"
    );
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains(mismatch)),
        "{output:#}"
    );
    assert_eq!(
        output["loc"],
        json!({"line": line, "column": column}),
        "{output:#}"
    );
}

fn assert_ok(output: &Value, status: i32) {
    assert_eq!(
        (status, output["result"].as_str()),
        (0, Some("ok")),
        "{output:#}"
    );
    assert!(output.get("kind").is_none(), "{output:#}");
    assert!(output.get("message").is_none(), "{output:#}");
}

const NOT_MISMATCH: &str = "expression of type Int is not assignable to Bool";
const MINUS_MISMATCH: &str = "expression of type Named(\"Color\") is not assignable to Int";

#[test]
fn step_argument_rejects_not_integer_in_unselected_branch() {
    let source = source(
        "true",
        Some((
            "forbidden",
            "paint(if true then Red else (if not 1 then Red else Blue))",
            "rejected",
        )),
    );
    let (output, status) = check("not-unselected", &source);
    assert_type_error(&output, status, NOT_MISMATCH, 12, 5);
}

#[test]
fn step_argument_rejects_enum_minus_in_unselected_branch() {
    let source = source(
        "true",
        Some((
            "forbidden",
            "paint(if true then Red else (if -Red > 0 then Red else Blue))",
            "rejected",
        )),
    );
    let (output, status) = check("minus-unselected", &source);
    assert_type_error(&output, status, MINUS_MISMATCH, 12, 5);
}

#[test]
fn step_argument_rejects_not_integer_behind_short_circuited_or() {
    let source = source(
        "true",
        Some((
            "forbidden",
            "paint(if c == Red or not 1 then Red else Blue)",
            "rejected",
        )),
    );
    let (output, status) = check("not-or-step", &source);
    assert_type_error(&output, status, NOT_MISMATCH, 12, 5);
}

#[test]
fn expectation_rejects_not_integer_behind_short_circuited_or() {
    let source = source(
        "true",
        Some(("acceptance", "paint(Blue)", "c == Blue or not 1")),
    );
    let (output, status) = check("not-or-expect", &source);
    assert_type_error(&output, status, NOT_MISMATCH, 13, 5);
}

#[test]
fn invariant_rejects_not_integer() {
    let source = source("c == Red or c == Blue or not 1", None);
    let (output, status) = check("not-invariant", &source);
    assert_type_error(&output, status, NOT_MISMATCH, 10, 3);
}

#[test]
fn invariant_rejects_enum_minus() {
    let source = source("c == Red or c == Blue or -Red > 0", None);
    let (output, status) = check("minus-invariant", &source);
    assert_type_error(&output, status, MINUS_MISMATCH, 10, 3);
}

#[test]
fn expectation_rejects_not_integer_behind_short_circuited_and() {
    // The outer not makes replay succeed when the invalid operand is skipped;
    // an ordinary false expectation must not masquerade as a type rejection.
    let source = source(
        "true",
        Some(("acceptance", "paint(Blue)", "not (false and not 1)")),
    );
    let (output, status) = check("not-and-expect", &source);
    assert_type_error(&output, status, NOT_MISMATCH, 13, 5);
}

#[test]
fn well_typed_not_operand_still_passes() {
    let source = source(
        "not (c == Blue) or c == Blue",
        Some(("acceptance", "paint(Blue)", "not (c == Red)")),
    );
    let (output, status) = check("valid-not", &source);
    assert_ok(&output, status);
}

#[test]
fn well_typed_nested_minus_operand_still_passes() {
    let source = source("-(-1) == 1", Some(("acceptance", "setn(-(-1))", "n == 1")));
    let (output, status) = check("valid-nested-minus", &source);
    assert_ok(&output, status);
}

#[test]
fn well_typed_numeric_state_minus_operand_still_passes() {
    let source = source(
        "- n >= -3 and - n <= 3",
        Some(("acceptance", "setn(- n)", "n == -1")),
    );
    let (output, status) = check("valid-state-minus", &source);
    assert_ok(&output, status);
}
