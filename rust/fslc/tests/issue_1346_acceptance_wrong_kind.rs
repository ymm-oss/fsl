// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1346: step arguments are not enum ordinals. Acceptance retains its
//! rejection and exit status, but diagnoses malformed arguments like forbidden.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn run_spec(source: &str, name: &str) -> (Value, i32) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.codex/tmp")
        .join(format!("fslc-issue-1346-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    std::fs::write(directory.join("spec.fsl"), source).expect("write spec");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "spec.fsl"])
        .current_dir(&directory)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    std::fs::remove_dir_all(directory).expect("remove scratch directory");
    (value, output.status.code().expect("exit status"))
}

fn shop(step: &str) -> String {
    format!(
        r#"requirements Shop {{
  type Amount = 0..100
  enum Color {{ Red, Blue }}
  enum Size {{ Small, Large }}
  state {{ balance: 0..100, color: Color }}
  init {{ balance = 50  color = Red }}
  requirement REQ-1 "withdraw" {{
    action withdraw(amount: Amount) {{ requires amount <= balance  balance = balance - amount }}
  }}
  requirement REQ-2 "paint" {{
    action paint(c: Color) {{ requires c != color  color = c }}
  }}
  acceptance AC-1 "paint blue" {{
    {step}
    expect color == Blue
  }}
}}
"#
    )
}

fn cases(step: &str) -> String {
    format!(
        r#"requirements CaseScope {{
  entity Case
  enum St {{ Waiting, Accepted, Responded }}
  state {{ cases: Map<Case, St> }}
  init {{ forall c: Case {{ cases[c] = Waiting }} }}
  requirement REQ-1 "accept" {{
    action accept(c: Case) {{ requires cases[c] == Waiting  cases[c] = Accepted }}
  }}
  requirement REQ-2 "respond" {{
    action respond(c: Case) {{ cases[c] = Responded }}
  }}
  acceptance AC-1 "respond" {{
    {step}
    expect cases[0] == Responded
  }}
}}
verify {{ instances Case = 3 }}
"#
    )
}

/// Detector: removing acceptance's wrong-kind diagnostic leaves the rejection
/// intact but loses the object, message and hint. The table mirrors #1241.
#[test]
fn wrong_kind_arguments_are_diagnosed_at_the_acceptance_step() {
    for (source, line, parameter, value, ty, name) in [
        (shop("paint(1)"), 14, "c", json!(1), "Color", "enum-int"),
        (
            shop("paint(true)"),
            14,
            "c",
            json!(true),
            "Color",
            "enum-bool",
        ),
        (
            shop("paint(Large)"),
            14,
            "c",
            json!("Large"),
            "Color",
            "enum-other-enum",
        ),
        (
            cases("respond(Waiting)"),
            13,
            "c",
            json!("Waiting"),
            "Case",
            "entity-enum",
        ),
        (
            cases("respond(true)"),
            13,
            "c",
            json!(true),
            "Case",
            "entity-bool",
        ),
        (
            shop("withdraw(Red)"),
            14,
            "amount",
            json!("Red"),
            "Amount",
            "range-enum",
        ),
        (
            shop("withdraw(false)"),
            14,
            "amount",
            json!(false),
            "Amount",
            "range-bool",
        ),
    ] {
        let (output, status) = run_spec(&source, name);
        assert_eq!(status, 2, "{name}: {output:#}");
        assert_eq!(output["result"], "error", "{name}: {output:#}");
        assert_eq!(output["kind"], "acceptance", "{name}: {output:#}");
        assert_eq!(output["trace_type"], "acceptance", "{name}: {output:#}");
        assert_eq!(output["id"], "AC-1", "{name}: {output:#}");
        assert_eq!(output["failed_step"], 0, "{name}: {output:#}");
        assert_eq!(
            output["wrong_kind_argument"],
            json!({"parameter": parameter, "value": value, "type": ty}),
            "{name}: exit={status}, output={output:#}"
        );
        let message = output["message"].as_str().expect("message");
        assert_eq!(
            message,
            format!(
                "argument {value} for '{parameter}' of '{}' is not a value of type '{ty}' in acceptance, so no guard was evaluated",
                output["step"]["action"].as_str().expect("step action")
            )
        );
        let hint = output["hint"].as_str().expect("hint");
        assert!(hint.contains("malformed rather than rejected"), "{hint}");
        assert!(
            hint.contains(&format!("Change the step to a value of '{ty}'.")),
            "{hint}"
        );
        assert_eq!(
            output["loc"],
            json!({"line": line, "column": 5}),
            "{name}: {output:#}"
        );
        assert!(output.get("out_of_scope_argument").is_none(), "{output:#}");
    }
}

/// Detector: replay must locate a wrong-kind argument after a valid prefix at
/// the second step, not at the acceptance declaration or the first step.
#[test]
fn wrong_kind_after_an_enabled_step_is_located_at_the_second_step() {
    let source = shop("paint(Blue)\n    paint(true)");
    let (output, status) = run_spec(&source, "second-step");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
    assert_eq!(output["failed_step"], 1, "{output:#}");
    assert_eq!(
        output["loc"],
        json!({"line": 15, "column": 5}),
        "{output:#}"
    );
    assert_eq!(
        output["wrong_kind_argument"],
        json!({"parameter": "c", "value": true, "type": "Color"}),
        "{output:#}"
    );
    assert_eq!(
        output["step"],
        json!({"action": "paint", "args": [true]}),
        "{output:#}"
    );
    assert_eq!(
        output["message"],
        "argument true for 'c' of 'paint' is not a value of type 'Color' in acceptance, so no guard was evaluated",
        "{output:#}"
    );
}

/// Positive control: the enum member, not its integer index, enables the step
/// and satisfies the final expectation.
#[test]
fn enum_member_satisfies_acceptance() {
    let (output, status) = run_spec(&shop("paint(Blue)"), "enum-member");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "ok", "{output:#}");
}

/// Preservation control: correct kinds refused by a guard keep their generic
/// not-enabled acceptance failure, with no wrong-kind diagnostic.
#[test]
fn guard_refusal_preserves_the_complete_baseline_output() {
    let (output, status) = run_spec(&shop("paint(Red)"), "guard-refusal");
    assert_eq!(status, 2, "{output:#}");
    // Complete JSON observed with base 5a539bbe. No fields are excluded.
    // Version values describe the current build/link, not the trace diagnostic;
    // derive those expected values so this preservation control survives releases.
    assert_eq!(
        output,
        json!({
            "fsl": "1.0",
            "result": "error",
            "kind": "acceptance",
            "id": "AC-1",
            "text": "paint blue",
            "failed_step": 0,
            "step": {"action": "paint", "args": ["Red"]},
            "step_results": [],
            "loc": {"line": 14, "column": 5},
            "trace_type": "acceptance",
            "versions": {
                "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
                "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
                "solver": {"name": "z3", "backend": "native-z3", "version": fsl_solver_z3::version()}
            }
        }),
        "{output:#}"
    );
}
