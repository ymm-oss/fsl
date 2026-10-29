// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const LOGS: &str = "rust/fslc/tests/fixtures/error_envelope_empty_records.json";
const FIXTURES: &str = "rust/fslc/tests/fixtures";
const HINT: &str = "supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared";

fn run(command: &str, source: &str) -> (Value, i32) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_fslc"));
    if command == "native-check" {
        process.args(["check", source]);
    } else {
        process.args(["ai", command, source]);
    }
    process
        .current_dir(root)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_UNSTABLE");
    if command == "replay" {
        process.args(["--logs", LOGS]);
    }
    let output = process.output().expect("run native CLI");
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = serde_json::from_slice(&output.stdout).expect("CLI JSON envelope");
    (value, output.status.code().expect("CLI exit code"))
}

fn assert_rule_error(command: &str, fixture: &str, rule: &str, physical: &str) {
    let (actual, exit) = run(command, &format!("{FIXTURES}/{fixture}"));
    assert_eq!(exit, 2, "{command}: {actual}");
    let expected = json!({
        "fsl": "1.0", "result": "error", "kind": "semantics",
        "message": format!("unknown ai hard-contract rule '{rule}' at {physical}"),
        "hint": HINT,
    });
    // Full envelope: loc must remain absent and the existing hint unchanged.
    assert_eq!(
        actual,
        expected_envelope(command, expected),
        "{command} {fixture}: expected physical {physical}"
    );
}

fn expected_envelope(command: &str, mut expected: Value) -> Value {
    if command == "native-check" {
        expected["versions"] = json!({
            "verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
            "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
            "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}
        });
    }
    expected
}

macro_rules! ai_commands {
    ($module:ident, $assertion:ident $(, $argument:expr)* $(,)?) => {
        mod $module {
            use super::*;
            #[test]
            fn native_check() {
                $assertion("native-check" $(, $argument)*);
            }
            #[test]
            fn ai_check() {
                $assertion("check" $(, $argument)*);
            }
            #[test]
            fn ai_compat() {
                $assertion("compat" $(, $argument)*);
            }
            #[test]
            fn ai_replay() {
                $assertion("replay" $(, $argument)*);
            }
        }
    };
}

ai_commands!(
    leading_comments,
    assert_rule_error,
    "error_envelope_ai_project_invalid_rule.fsl",
    "unregistered_hard_rule",
    "9:3"
);
ai_commands!(
    inline_indentation,
    assert_rule_error,
    "ai_project_inline_invalid_rule.fsl",
    "unregistered_inline_rule",
    "4:33"
);
ai_commands!(
    valid_first_invalid_later,
    assert_rule_error,
    "ai_project_later_invalid_rule.fsl",
    "unregistered_later_rule",
    "12:7"
);
ai_commands!(
    standalone_control,
    assert_rule_error,
    "error_envelope_ai_invalid_rule.fsl",
    "unregistered_hard_rule",
    "17:3"
);

fn assert_parse_error_control(command: &str) {
    let (actual, exit) = run(
        command,
        &format!("{FIXTURES}/error_envelope_broken_ai_project.fsl"),
    );
    assert_eq!(exit, 2, "{command}: {actual}");
    // Compat's historical wrapper intentionally differs; do not normalize it.
    let expected = if command == "compat" {
        json!({"fsl": "1.0", "result": "error", "kind": "semantics",
               "message": "expected ai_component declaration"})
    } else {
        json!({"fsl": "1.0", "result": "error", "kind": "parse",
               "message": "expected ai_component declaration at 9:3",
               "loc": {"line": 9, "column": 3}, "diagnostic_code": "FSL-PARSE"})
    };
    assert_eq!(
        actual,
        expected_envelope(command, expected),
        "{command}: existing parser-owned position"
    );
}

fn assert_valid_control(command: &str) {
    let (actual, exit) = run(command, "examples/ai/support_answer_quality.fsl");
    assert_eq!(exit, 0, "{command}: {actual}");
    let expected = match command {
        "native-check" => "ok",
        "check" => "ai_project_analyzed",
        "compat" => "compat_profile_generated",
        "replay" => "replay_conformant",
        _ => unreachable!("three AI commands"),
    };
    assert_eq!(actual["result"], expected, "{actual}");
}

ai_commands!(parse_error_control, assert_parse_error_control);
ai_commands!(valid_project_control, assert_valid_control);
