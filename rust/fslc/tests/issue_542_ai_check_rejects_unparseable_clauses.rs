// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #542: `fslc ai check` (and the generic
//! `fslc check` dispatch that shares its output) reported
//! `ai_project_analyzed` with exit 0 for an fsl-ai project whose declaration
//! bodies were never read. The check stage ran a line scanner that collected
//! declaration *names* by string prefix, so a `require` clause matching no
//! known evidence-clause grammar was accepted -- a confidently green verdict
//! over a project `fslc ai eval`/`drift` cannot execute.
//!
//! Native now reports the check stage from `fsl_syntax::parse_ai_project`,
//! the same parser the evidence commands run, and rejects an unexecutable
//! clause as a spec error (exit 2, `kind: "parse"`) per `docs/LANGUAGE.md`'s
//! exit-code table.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

// --- negative: an unexecutable clause must not be reported as analyzed -----

#[test]
fn ai_check_rejects_an_unparseable_statistical_require_clause() {
    let path = fixture("issue_542_unparseable_statistical_clause.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "parse");
    let message = value["message"].as_str().expect("message");
    assert!(
        message.contains("statistical_property 'Whatever'"),
        "{message}"
    );
    assert!(
        message.contains("this is not a valid clause at all"),
        "{message}"
    );
    // The pre-fix bug returned exactly this success envelope for this input;
    // assert the whole shape is gone, not merely that some error appeared.
    assert!(value.get("statistical_properties").is_none(), "{value}");
    assert!(value.get("formal_result").is_none(), "{value}");
}

#[test]
fn ai_check_rejects_an_unparseable_observed_require_clause() {
    let path = fixture("issue_542_unparseable_observed_clause.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "parse");
    let message = value["message"].as_str().expect("message");
    assert!(
        message.contains("observed_property 'Operational'"),
        "{message}"
    );
    // The sibling `statistical_property` in the same file parses cleanly, so
    // the rejection must name the observed clause and only it.
    assert!(!message.contains("'Parseable'"), "{message}");
}

#[test]
fn generic_check_rejects_the_same_project_as_ai_check() {
    // `fslc check` dispatches fsl-ai project sources through the same check
    // output; leaving it green would keep the false green one command away.
    let path = fixture("issue_542_unparseable_statistical_clause.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "parse");
    assert!(value.get("ai_analysis_result").is_none(), "{value}");
}

// --- positive: legitimate project input is still accepted -----------------

#[test]
fn ai_check_still_analyzes_a_legitimate_project() {
    let (value, status) = run(&["ai", "check", "examples/ai/support_answer_quality.fsl"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ai_project_analyzed");
    assert_eq!(value["formal_result"], "not_run");
    assert_eq!(
        value["components"],
        serde_json::json!(["SupportAnswerAgent"])
    );
    assert_eq!(
        value["statistical_properties"],
        serde_json::json!(["LooseQuality", "StrictQuality"])
    );
    assert_eq!(
        value["observed_properties"],
        serde_json::json!(["SupportAgentOperationalQuality"])
    );
    assert_eq!(value["migrations"], serde_json::json!(["PromptV7ToV8"]));
    assert_eq!(value["findings"], serde_json::json!([]));
    // `raw_blocks` carries the un-descended declarations as `{kind, name}`
    // (`skills/fsl/reference.md`), one entry per block.
    let raw_blocks = value["raw_blocks"].as_array().expect("raw_blocks array");
    assert!(
        raw_blocks.iter().any(|block| {
            block["kind"] == "authority" && block["name"] == "SupportAgentAuthority"
        }),
        "{value}"
    );
}

#[test]
fn ai_check_still_accepts_garbage_inside_an_unvalidated_raw_block() {
    // Raw blocks are recognized only as block boundaries and are
    // contractually not validated; tightening clause parsing must not start
    // rejecting them.
    let path = fixture("issue_542_unvalidated_raw_block.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ai_project_analyzed");
    assert_eq!(
        value["statistical_properties"],
        serde_json::json!(["Quality"])
    );
    assert_eq!(
        value["raw_blocks"],
        serde_json::json!([
            {"kind": "ai_action", "name": "Draft"},
            {"kind": "trust_boundary", "name": "Edge"},
        ])
    );

    let (checked, check_status) = run(&["check", &path]);
    assert_eq!(check_status, 0, "{checked}");
    assert_eq!(checked["result"], "ok");
    assert_eq!(checked["ai_analysis_result"], "ai_project_analyzed");
}
