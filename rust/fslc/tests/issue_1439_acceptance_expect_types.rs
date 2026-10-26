// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1439: acceptance expressions and FSL 5 scenario expressions must receive
//! the same recursive type checking as invariants, before concrete replay.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1439_acceptance_expect_types")
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

fn check(label: &str, predicate: &str, next_edition: bool) -> (Value, i32) {
    let scratch = Scratch::new(label);
    let (header, trace, expectation) = if next_edition {
        ("fsl 5\n", "scenario", "then")
    } else {
        ("", "acceptance", "expect")
    };
    // Keep expect at 10:5 and then at 11:5: diagnostics must point to the
    // authored keyword, not the trace declaration or a placeholder location.
    let source = format!(
        r#"{header}requirements E {{
  enum Color {{ Red, Blue }}
  state {{ c: Color }}
  init {{ c = Red }}
  requirement REQ-1 "paint" {{
    action paint(x: Color) {{ c = x }}
  }}
  {trace} AC-1 "type comparison" {{
    paint(Blue)
    {expectation} {predicate}
  }}
}}
"#
    );
    std::fs::write(scratch.0.join("comparison.fsl"), source).expect("write inline fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["check", "comparison.fsl"])
        .current_dir(&scratch.0)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if next_edition {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run native fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: invalid JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_type_error(output: &Value, status: i32, line: u32) {
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{output:#}"
    );
    let message = output["message"].as_str().expect("type error message");
    assert!(
        message.contains("expression of type Int is not assignable to Named(\"Color\")"),
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

/// Detector: replay alone treats an enum/integer inequality as true.
#[test]
fn acceptance_expect_rejects_enum_integer_inequality() {
    let (output, status) = check("acceptance-inequality", "c != 1", false);
    assert_type_error(&output, status, 10);
}

/// Detector: exit 2 alone is insufficient; a false replay result has kind
/// acceptance, whereas the malformed expression must have kind semantics.
#[test]
fn acceptance_expect_rejects_enum_integer_equality_as_a_type_error() {
    let (output, status) = check("acceptance-equality", "c == 1", false);
    assert_type_error(&output, status, 10);
}

/// Preservation control: the post-state predicate is well typed and true,
/// but false in init, so it must not become a persistent invariant.
#[test]
fn acceptance_expect_accepts_matching_enum_comparison() {
    let (output, status) = check("acceptance-matching-enum", "c == Blue", false);
    assert_ok(&output, status);
}

/// Detector: the gated scenario spelling must not retain the same false green.
#[test]
fn fsl5_scenario_then_rejects_enum_integer_inequality() {
    let (output, status) = check("scenario-inequality", "c != 1", true);
    assert_type_error(&output, status, 11);
}

/// Detector: a scenario's wrong-kind equality must be a type error, not a
/// concrete acceptance failure that happens to return the same exit code.
#[test]
fn fsl5_scenario_then_rejects_enum_integer_equality_as_a_type_error() {
    let (output, status) = check("scenario-equality", "c == 1", true);
    assert_type_error(&output, status, 11);
}

/// Preservation control for the gated syntax and post-state-only semantics.
#[test]
fn fsl5_scenario_then_accepts_matching_enum_comparison() {
    let (output, status) = check("scenario-matching-enum", "c == Blue", true);
    assert_ok(&output, status);
}
