// SPDX-License-Identifier: Apache-2.0

//! #1439: Bool outer expressions must not hide ill-typed nested operands.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn rejects_nested(label: &str, predicate: &str, next_edition: bool) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1439_nested_expect_types")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create nested fixture directory");
    let (header, trace, keyword, line) = if next_edition {
        ("fsl 5\n", "scenario", "then", 11)
    } else {
        ("", "acceptance", "expect", 10)
    };
    let source = format!(
        "{header}requirements E {{\n  enum Color {{ Red, Blue }}\n  state {{ c: Color }}\n  init {{ c = Red }}\n  requirement REQ-1 \"paint\" {{\n    action paint(x: Color) {{ c = x }}\n  }}\n  {trace} AC-1 \"nested comparison\" {{\n    paint(Blue)\n    {keyword} {predicate}\n  }}\n}}\n"
    );
    std::fs::write(directory.join("nested.fsl"), source).expect("write nested fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["check", "nested.fsl"])
        .current_dir(&directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if next_edition {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run nested expectation check");
    std::fs::remove_dir_all(directory).expect("remove nested fixture directory");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: {error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        (
            output.status.code(),
            value["result"].as_str(),
            value["kind"].as_str()
        ),
        (Some(2), Some("error"), Some("semantics")),
        "{label}: {value:#}"
    );
    assert!(
        value["message"].as_str().is_some_and(|message| {
            message.contains("expression of type Int is not assignable to Named(\"Color\")")
        }),
        "{label}: {value:#}"
    );
    assert_eq!(
        value["loc"],
        json!({"line": line, "column": 5}),
        "{label}: {value:#}"
    );
}

#[test]
fn acceptance_conjunction_checks_nested_operands() {
    rejects_nested("acceptance-and", "true and c != 1", false);
}

#[test]
fn acceptance_negation_checks_nested_operands() {
    rejects_nested("acceptance-not", "not (c == 1)", false);
}

#[test]
fn acceptance_conditional_checks_its_condition() {
    rejects_nested("acceptance-if", "if c != 1 then true else false", false);
}

#[test]
fn acceptance_quantifier_checks_its_body() {
    rejects_nested(
        "acceptance-forall",
        "forall color: Color { color != 1 }",
        false,
    );
}

#[test]
fn fsl5_then_conjunction_checks_nested_operands() {
    rejects_nested("then-and", "true and c != 1", true);
}

#[test]
fn fsl5_then_negation_checks_nested_operands() {
    rejects_nested("then-not", "not (c == 1)", true);
}

#[test]
fn fsl5_then_conditional_checks_its_condition() {
    rejects_nested("then-if", "if c != 1 then true else false", true);
}

#[test]
fn fsl5_then_quantifier_checks_its_body() {
    rejects_nested("then-forall", "forall color: Color { color != 1 }", true);
}
