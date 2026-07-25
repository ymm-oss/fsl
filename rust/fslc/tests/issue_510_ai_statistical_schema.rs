// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #510: native `fslc ai eval` results did not
//! conform to `schemas/fslc/ai/statistical-result.v0.schema.json` (a
//! successful result carried only `fsl,result,formal_result,property,
//! dataset,interval,checks,findings`, omitting the required
//! `schema_version,status,slice,metric,n,estimate,threshold,evaluator,
//! assumptions`) and every non-`statistically_supported` terminal status
//! (`dataset_invalid`, `evaluator_untrusted`, `insufficient_samples`,
//! `inconclusive`) exited 0 instead of the documented non-success routing
//! (`docs/LANGUAGE.md:898-906`).
//!
//! Native now builds every eval result through
//! `fsl_tools::evaluate_statistical_property`, which always emits the full
//! schema-required field set, and `wrap_specialized` routes every gate
//! status (not just `statistically_unsupported`) to exit 1.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
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

/// The schema's required top-level fields
/// (`schemas/fslc/ai/statistical-result.v0.schema.json`).
const REQUIRED_FIELDS: &[&str] = &[
    "schema_version",
    "result",
    "status",
    "formal_result",
    "dataset",
    "slice",
    "metric",
    "n",
    "estimate",
    "interval",
    "threshold",
    "evaluator",
    "assumptions",
    "findings",
];

fn assert_schema_conformant(value: &Value) {
    for field in REQUIRED_FIELDS {
        assert!(
            value.get(field).is_some(),
            "missing required field '{field}': {value}"
        );
    }
    assert_eq!(value["schema_version"], "fsl-ai-statistical-result.v0");
    assert_eq!(value["formal_result"], "not_run");
    let interval = value["interval"].as_object().expect("interval object");
    for field in ["method", "confidence", "lower", "upper"] {
        assert!(
            interval.contains_key(field),
            "interval missing '{field}': {value}"
        );
    }
    assert_eq!(interval["method"], "wilson");
    let threshold = value["threshold"].as_object().expect("threshold object");
    for field in ["operator", "value"] {
        assert!(
            threshold.contains_key(field),
            "threshold missing '{field}': {value}"
        );
    }
    let evaluator = value["evaluator"].as_object().expect("evaluator object");
    assert!(
        evaluator.contains_key("id"),
        "evaluator missing 'id': {value}"
    );
    assert!(
        evaluator.contains_key("trust_status"),
        "evaluator missing 'trust_status': {value}"
    );
    assert!(value["n"].is_i64() || value["n"].is_u64(), "{value}");
    assert!(value["estimate"].is_number(), "{value}");
    assert!(value["assumptions"].is_array(), "{value}");
    assert!(value["findings"].is_array(), "{value}");
}

// --- a successful result now carries every required schema field ---------

#[test]
fn a_supported_result_is_schema_conformant_and_exits_zero() {
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--property",
        "LooseQuality",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "statistically_supported");
    assert_eq!(value["status"], "statistically_supported");
    assert_schema_conformant(&value);
    // The pre-fix output shape omitted these entirely.
    assert!(value.get("slice").is_some_and(Value::is_string), "{value}");
    assert!(value.get("metric").is_some_and(Value::is_string), "{value}");
    assert!(value.get("n").is_some(), "{value}");
}

// --- every gate status is schema-conformant AND exits 1, not 0 -----------

#[test]
fn duplicate_records_are_dataset_invalid_and_exit_one() {
    let path = std::env::temp_dir().join("issue_510_duplicate.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"schema_version":"fsl-ai-eval-record.v0","case_id":"dup","component":"SupportAnswerAgent","dataset":"SupportEvalV3","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}"#,
            "\n",
            r#"{"schema_version":"fsl-ai-eval-record.v0","case_id":"dup","component":"SupportAnswerAgent","dataset":"SupportEvalV3","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}"#,
            "\n",
        ),
    )
    .expect("write scratch records");
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--records",
        path.to_str().expect("utf8 path"),
        "--dataset",
        "SupportEvalV3",
        "--property",
        "LooseQuality",
    ]);
    // The pre-fix implementation reported this exact result with exit 0.
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "dataset_invalid");
    assert_schema_conformant(&value);
}

#[test]
fn an_untrusted_evaluator_is_evaluator_untrusted_and_exits_one() {
    let path = std::env::temp_dir().join("issue_510_untrusted.jsonl");
    let mut body = String::new();
    for index in 0..10 {
        body.push_str(
            &serde_json::json!({
                "schema_version": "fsl-ai-eval-record.v0",
                "case_id": format!("case-{index}"),
                "component": "SupportAnswerAgent",
                "dataset": "SupportEvalV3",
                "slice": "all",
                "metric": "accuracy",
                "outcome": true,
                "evaluator": {"id": "SupportAnswerJudge", "calibration_status": "untrusted"},
            })
            .to_string(),
        );
        body.push('\n');
    }
    std::fs::write(&path, body).expect("write scratch records");
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--records",
        path.to_str().expect("utf8 path"),
        "--dataset",
        "SupportEvalV3",
        "--property",
        "LooseQuality",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "evaluator_untrusted");
    assert_schema_conformant(&value);
}

#[test]
fn too_few_samples_is_insufficient_samples_and_exits_one() {
    // `LooseQuality` declares `slice JapaneseRefundTickets { require
    // min_samples >= 5; ... }`; supply plenty of "all"-slice records (so
    // that gate passes cleanly) but only 2 for `JapaneseRefundTickets`.
    // `insufficient_samples` outranks `statistically_supported`/
    // `statistically_unsupported` in the status priority regardless of how
    // the slice's own ci_lower bound happens to land, so the overall result
    // is deterministically `insufficient_samples` -- a non-success gate
    // status that must not exit 0.
    let path = std::env::temp_dir().join("issue_510_insufficient.jsonl");
    let mut body = String::new();
    for index in 0..10 {
        body.push_str(
            &serde_json::json!({
                "schema_version": "fsl-ai-eval-record.v0",
                "case_id": format!("all-{index}"),
                "component": "SupportAnswerAgent",
                "dataset": "SupportEvalV3",
                "slice": "all",
                "metric": "accuracy",
                "outcome": true,
                "evaluator": {"id": "SupportAnswerJudge", "calibration_status": "trusted"},
            })
            .to_string(),
        );
        body.push('\n');
    }
    for index in 0..2 {
        body.push_str(
            &serde_json::json!({
                "schema_version": "fsl-ai-eval-record.v0",
                "case_id": format!("jp-{index}"),
                "component": "SupportAnswerAgent",
                "dataset": "SupportEvalV3",
                "slice": "JapaneseRefundTickets",
                "metric": "accuracy",
                "outcome": true,
                "evaluator": {"id": "SupportAnswerJudge", "calibration_status": "trusted"},
            })
            .to_string(),
        );
        body.push('\n');
    }
    std::fs::write(&path, body).expect("write scratch records");
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--records",
        path.to_str().expect("utf8 path"),
        "--dataset",
        "SupportEvalV3",
        "--property",
        "LooseQuality",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "insufficient_samples");
    assert_schema_conformant(&value);
    let checks = value["checks"].as_array().expect("checks array");
    assert!(
        checks
            .iter()
            .any(|check| check["status"] == "insufficient_samples" && check["n"] == 2),
        "{value}"
    );
}
