// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1439 extension: every trace step argument is checked recursively before
//! replay, without forcing the expression's outer type to the parameter type.
//! The sixteen detectors fail on the pre-extension implementation's false
//! greens; five preservation controls protect valid steps and #1241 errors.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1439_trace_step_types")
            .join(label);
        std::fs::create_dir_all(&directory).expect("create per-test scratch directory");
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(label: &str, source: &str, next_edition: bool, arguments: &[&str]) -> (Value, i32) {
    let scratch = Scratch::new(label);
    std::fs::write(scratch.0.join("trace.fsl"), source).expect("write inline trace fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(arguments)
        .current_dir(&scratch.0)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if next_edition {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run native fslc");
    assert!(
        output.stderr.is_empty(),
        "{label}: unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: invalid JSON ({error}): stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn check(label: &str, source: &str, next_edition: bool) -> (Value, i32) {
    run(label, source, next_edition, &["check", "trace.fsl"])
}

/// The first authored step is always at 11:5; a second step is at 12:5.
fn published_trace(trace: &str, steps: &str, expectation: &str) -> String {
    format!(
        r#"requirements E {{
  enum Color {{ Red, Blue }}
  type N = 0..3
  state {{ c: Color, n: N }}
  init {{ c = Red n = 0 }}
  requirement REQ-1 "paint" {{
    action paint(x: Color) {{ requires x != c c = x }}
    action setn(v: N) {{ requires v > n n = v }}
  }}
  {trace} {{
{steps}
    expect {expectation}
  }}
}}
"#
    )
}

/// The first authored step is always at 8:5; a second step is at 9:5.
fn fsl5_trace(trace: &str, steps: &str, expectation: &str) -> String {
    format!(
        r"fsl 5
requirements R {{
  number Amount
  actor Employee
  entity Claim {{ amount: Amount = 0 lifecycle Draft -> Submitted }}
  event submit = Employee submits Claim with amount Draft -> Submitted
  {trace} {{
{steps}
    then {expectation}
  }}
}}
verify {{ instances Claim = 2 values Amount = 0..3 }}
"
    )
}

fn assert_type_error(output: &Value, status: i32, type_name: &str, line: u32) {
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{output:#}"
    );
    let mismatch = format!("expression of type Int is not assignable to Named(\"{type_name}\")");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains(&mismatch)),
        "{output:#}"
    );
    assert_eq!(
        output["loc"],
        json!({"line": line, "column": 5}),
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

fn assert_wrong_kind(output: &Value, status: i32) {
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("forbidden")),
        "{output:#}"
    );
    assert_eq!(output["trace_type"], "forbidden", "{output:#}");
    assert_eq!(output["id"], "FB-1", "{output:#}");
    assert_eq!(output["failed_step"], 0, "{output:#}");
    assert_eq!(
        output["step"],
        json!({"action": "paint", "args": [1]}),
        "{output:#}"
    );
    assert_eq!(output["step_results"], json!([]), "{output:#}");
    assert_eq!(
        output["wrong_kind_argument"],
        json!({"parameter": "x", "value": 1, "type": "Color"}),
        "{output:#}"
    );
    assert_eq!(
        output["message"],
        "argument 1 for 'x' of 'paint' is not a value of type 'Color' in forbidden, so no guard was evaluated",
        "{output:#}"
    );
    assert_eq!(
        output["loc"],
        json!({"line": 11, "column": 5}),
        "{output:#}"
    );
    assert!(
        output["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("malformed rather than rejected")),
        "{output:#}"
    );
    assert!(output.get("accepted_trace").is_none(), "{output:#}");
    assert!(output.get("out_of_scope_argument").is_none(), "{output:#}");
}

#[test]
fn acceptance_step_rejects_ill_typed_color_condition() {
    let source = published_trace(
        "acceptance AC-1 \"ill-typed step\"",
        "    paint(if c != 1 then Blue else Red)",
        "c == Blue",
    );
    let (output, status) = check("acceptance-color", &source, false);
    assert_type_error(&output, status, "Color", 11);
}

#[test]
fn forbidden_setup_step_rejects_ill_typed_color_condition() {
    let source = published_trace(
        "forbidden FB-1 \"ill-typed setup\"",
        "    paint(if c != 1 then Blue else Red)\n    paint(Blue)",
        "rejected",
    );
    let (output, status) = check("forbidden-color-setup", &source, false);
    assert_type_error(&output, status, "Color", 11);
}

/// S1: a malformed condition selects Red, whose guard rejection must not
/// satisfy FB-1. Both BASE and the expectation-only fix return ok / exit 0.
#[test]
fn forbidden_final_step_rejects_ill_typed_color_condition_s1() {
    let source = published_trace(
        "forbidden FB-1 \"ill-typed arg selects rejected value\"",
        "    paint(if c != 1 then Red else Blue)",
        "rejected",
    );
    let (output, status) = check("forbidden-color-final-s1", &source, false);
    assert_type_error(&output, status, "Color", 11);
}

#[test]
fn fsl5_scenario_call_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "scenario AC-1 \"ill-typed call\"",
        "    submit(0, if claim_stage[0] != 1 then 1 else 2)",
        "claim_stage[0] == Submitted",
    );
    let (output, status) = check("scenario-call", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_scenario_input_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "scenario AC-1 \"ill-typed phrase input\"",
        "    Employee submits claim1 with amount (if claim_stage[0] != 1 then 1 else 2)",
        "claim1 is Submitted",
    );
    let (output, status) = check("scenario-input", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_scenario_marked_input_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "scenario AC-1 \"ill-typed marked input\"",
        "    Employee が claim1 を amount (if claim_stage[0] != 1 then 1 else 2) で submits",
        "claim1 is Submitted",
    );
    let (output, status) = check("scenario-marked-input", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_forbidden_call_setup_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed setup call\"",
        "    submit(0, if claim_stage[0] != 1 then 1 else 2)\n    submit(0, 1)",
        "rejected",
    );
    let (output, status) = check("forbidden-call-setup", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_forbidden_call_final_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed final call\"",
        "    submit(0, 1)\n    submit(0, if claim_stage[0] != 1 then 1 else 2)",
        "rejected",
    );
    let (output, status) = check("forbidden-call-final", &source, true);
    assert_type_error(&output, status, "ClaimStage", 9);
}

#[test]
fn fsl5_forbidden_input_setup_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed setup input\"",
        "    Employee submits claim1 with amount (if claim_stage[0] != 1 then 1 else 2)\n    Employee submits claim1 with amount 1",
        "rejected",
    );
    let (output, status) = check("forbidden-input-setup", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_forbidden_input_final_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed final input\"",
        "    Employee submits claim1 with amount 1\n    Employee submits claim1 with amount (if claim_stage[0] != 1 then 1 else 2)",
        "rejected",
    );
    let (output, status) = check("forbidden-input-final", &source, true);
    assert_type_error(&output, status, "ClaimStage", 9);
}

#[test]
fn fsl5_forbidden_marked_setup_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed setup marked input\"",
        "    Employee が claim1 を amount (if claim_stage[0] != 1 then 1 else 2) で submits\n    Employee が claim1 を amount 1 で submits",
        "rejected",
    );
    let (output, status) = check("forbidden-marked-setup", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_forbidden_marked_final_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "forbidden FB-1 \"ill-typed final marked input\"",
        "    Employee が claim1 を amount 1 で submits\n    Employee が claim1 を amount (if claim_stage[0] != 1 then 1 else 2) で submits",
        "rejected",
    );
    let (output, status) = check("forbidden-marked-final", &source, true);
    assert_type_error(&output, status, "ClaimStage", 9);
}

#[test]
fn acceptance_numeric_step_rejects_ill_typed_color_condition() {
    let source = published_trace(
        "acceptance AC-1 \"ill-typed numeric argument\"",
        "    setn(if c != 1 then 2 else 0)",
        "n == 2",
    );
    let (output, status) = check("acceptance-number", &source, false);
    assert_type_error(&output, status, "Color", 11);
}

#[test]
fn fsl5_marked_with_input_rejects_ill_typed_stage_condition() {
    let source = fsl5_trace(
        "scenario AC-1 \"ill-typed hybrid input\"",
        "    Employee が claim1 を submits with amount (if claim_stage[0] != 1 then 1 else 2)",
        "claim1 is Submitted",
    );
    let (output, status) = check("scenario-marked-with", &source, true);
    assert_type_error(&output, status, "ClaimStage", 8);
}

#[test]
fn fsl5_relation_role_input_rejects_ill_typed_stage_condition() {
    let source = r#"fsl 5
requirements R {
  actor Agent
  entity Agent
  entity Ticket { lifecycle Open -> Assigned }
  Ticket belongs to Agent as assignee via take
  event take = Agent takes Ticket Open -> Assigned
  scenario AC-1 "ill-typed role input" {
    Agent takes t1 with assignee (if ticket_stage[0] != 1 then 1 else 0)
    then t1 is Assigned
  }
}
verify { instances Ticket = 2 instances Agent = 2 }
"#;
    let (output, status) = check("scenario-relation-role", source, true);
    assert_type_error(&output, status, "TicketStage", 9);
}

#[test]
fn acceptance_step_checks_unselected_conditional_branch() {
    let source = published_trace(
        "acceptance AC-1 \"ill-typed unselected branch\"",
        "    paint(if true then Blue else (if c != 1 then Red else Blue))",
        "c == Blue",
    );
    let (output, status) = check("acceptance-unselected-branch", &source, false);
    assert_type_error(&output, status, "Color", 11);
}

/// Preservation: these expressions must pass even without the new validator.
#[test]
fn well_typed_trace_arguments_preserve_const_enum_number_and_bool() {
    let source = r#"requirements E {
  enum Color { Red, Blue }
  type N = 0..3
  const TWO = 2
  state { c: Color, n: N, b: Bool }
  init { c = Red n = 0 b = false }
  requirement REQ-1 "typed steps" {
    action paint(x: Color) { requires x != c c = x }
    action setn(v: N) { n = v }
    action flag(v: Bool) { b = v }
  }
  acceptance AC-1 "const enum number Bool and legal nested expressions" {
    paint(if c != Blue then Blue else Red)
    setn(TWO)
    setn(1)
    flag(true)
    expect c == Blue and n == 1 and b
  }
  forbidden FB-1 "well-typed setup and final" {
    paint(if c == Red then Blue else Red)
    paint(if c != Red then Blue else Red)
    expect rejected
  }
}
"#;
    let (output, status) = check("typed-published-arguments", source, false);
    assert_ok(&output, status);
}

const FSL5_WELL_TYPED: &str = r#"fsl 5
requirements R {
  number Amount
  const ONE = 1
  actor Employee
  entity Claim { amount: Amount = 0 lifecycle Draft -> Submitted }
  event submit = Employee submits Claim with amount Draft -> Submitted
  scenario AC-CALL "typed call" { submit(0, ONE) then claim_stage[0] == Submitted }
  scenario AC-PHRASE "typed phrase" {
    Employee submits claim1 with amount (if claim_stage[0] == Draft then ONE else 2)
    then claim1 is Submitted
  }
  scenario AC-MARKED "typed marked phrase" {
    Employee が claim1 を amount ONE で submits
    then claim1 is Submitted
  }
  scenario AC-HYBRID "typed hybrid" {
    Employee が claim1 を submits with amount ONE
    then claim1 is Submitted
  }
  forbidden FB-CALL "typed calls" {
    submit(0, ONE)
    submit(0, ONE)
    then rejected
  }
  forbidden FB-PHRASE "typed phrases" {
    Employee submits claim1 with amount ONE
    Employee submits claim1 with amount ONE
    then rejected
  }
  forbidden FB-MARKED "typed marked phrases" {
    Employee が claim1 を amount ONE で submits
    Employee が claim1 を amount ONE で submits
    then rejected
  }
}
verify { instances Claim = 2 values Amount = 0..3 }
"#;

#[test]
fn fsl5_well_typed_calls_phrases_and_named_instances_still_pass() {
    let (output, status) = check("typed-fsl5-arguments", FSL5_WELL_TYPED, true);
    assert_ok(&output, status);
    assert_eq!(output["edition"], "next", "{output:#}");
    assert_eq!(output["fsl_edition"], 5, "{output:#}");
}

#[test]
fn forbidden_final_literal_wrong_kind_keeps_existing_diagnostic() {
    let source = published_trace(
        "forbidden FB-1 \"wrong final kind\"",
        "    paint(1)",
        "rejected",
    );
    let (output, status) = check("literal-wrong-kind", &source, false);
    assert_wrong_kind(&output, status);
}

#[test]
fn forbidden_final_conditional_wrong_kind_keeps_existing_diagnostic() {
    let source = published_trace(
        "forbidden FB-1 \"wrong final kind\"",
        "    paint(if true then 1 else 2)",
        "rejected",
    );
    let (output, status) = check("conditional-wrong-kind", &source, false);
    assert_wrong_kind(&output, status);
}

#[test]
fn well_typed_step_arguments_keep_values_override_skips() {
    let (output, status) = run(
        "typed-values-override",
        FSL5_WELL_TYPED,
        true,
        &[
            "verify",
            "trace.fsl",
            "--depth",
            "0",
            "--values",
            "Amount=0..0",
        ],
    );
    assert_eq!(
        (status, output["result"].as_str()),
        (0, Some("verified")),
        "{output:#}"
    );
    assert!(output.get("kind").is_none(), "{output:#}");
    assert_eq!(output["completeness"], "bounded", "{output:#}");
    assert_eq!(output["checked_to_depth"], 0, "{output:#}");
    assert_eq!(
        output["bounds_overrides"],
        json!({"instances": {}, "values": {"Amount": [0, 0]}}),
        "{output:#}"
    );
    let cases = [
        ("acceptance", "AC-CALL"),
        ("acceptance", "AC-PHRASE"),
        ("acceptance", "AC-MARKED"),
        ("acceptance", "AC-HYBRID"),
        ("forbidden", "FB-CALL"),
        ("forbidden", "FB-PHRASE"),
        ("forbidden", "FB-MARKED"),
    ];
    let reference = "submit(0, 1): argument 1 for 'amount' is outside its domain";
    let skipped = cases
        .iter()
        .map(|(kind, id)| json!({"kind": kind, "id": id, "reference": reference}))
        .collect::<Vec<_>>();
    assert_eq!(
        output["requirement_traces"],
        json!({
            "skipped": skipped,
            "result": "not_evaluated",
            "reason": "bounds_override",
            "reasons": ["bounds_override"]
        }),
        "{output:#}"
    );
    let expected_warnings = cases
        .iter()
        .map(|(kind, id)| {
            json!({
                "kind": format!("{kind}_skipped"),
                "id": id,
                "reference": reference,
                "message": format!("{kind} '{id}' skipped: references values outside overridden bounds (Amount=0..0): {reference}")
            })
        })
        .collect::<Vec<_>>();
    let warnings = output["warnings"]
        .as_array()
        .expect("verification warning array")
        .iter()
        .filter(|warning| {
            matches!(
                warning["kind"].as_str(),
                Some("acceptance_skipped" | "forbidden_skipped")
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(warnings, expected_warnings, "{output:#}");
}
