// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #509: native `fslc ai eval`/`regress`/
//! `drift` aggregated eval/telemetry JSONL with hardcoded example
//! metrics/thresholds and never read the declared
//! `statistical_property`/`ai_migration`/`observed_property` selection or
//! even the spec path (`regress`/`drift` took `_path: &Path` and ignored
//! it). A spec author's declared slice gate, migration threshold, or
//! observed-property requirement had no effect on the result.
//!
//! Native now parses the selected declaration
//! (`fsl_syntax::parse_ai_project`) and executes it via
//! `fsl_tools::evaluate_statistical_property` /
//! `evaluate_migration` / `evaluate_observed_property`.

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

fn write_slice_fail_records() -> PathBuf {
    // Ten trusted `slice:"all"` accuracy records with `outcome:true` and
    // five trusted `slice:"JapaneseRefundTickets"` accuracy records with
    // `outcome:false` -- the exact reproduction from issue #509's evidence.
    // `examples/ai/support_answer_quality.fsl`'s `LooseQuality` declares an
    // "all" gate at 0.45 (which this data clears) *and* a separate
    // `JapaneseRefundTickets` slice gate at 0.35 (which it does not); only a
    // per-slice-declaration-aware evaluator can see the second gate at all.
    let mut lines = Vec::new();
    for index in 0..10 {
        lines.push(serde_json::json!({
            "schema_version": "fsl-ai-eval-record.v0",
            "case_id": format!("all-{index}"),
            "component": "SupportAnswerAgent",
            "dataset": "SupportEvalV3",
            "slice": "all",
            "metric": "accuracy",
            "outcome": true,
            "evaluator": {"id": "SupportAnswerJudge", "calibration_status": "trusted"},
        }));
    }
    for index in 0..5 {
        lines.push(serde_json::json!({
            "schema_version": "fsl-ai-eval-record.v0",
            "case_id": format!("jp-{index}"),
            "component": "SupportAnswerAgent",
            "dataset": "SupportEvalV3",
            "slice": "JapaneseRefundTickets",
            "metric": "accuracy",
            "outcome": false,
            "evaluator": {"id": "SupportAnswerJudge", "calibration_status": "trusted"},
        }));
    }
    let path = std::env::temp_dir().join("issue_509_slice_fail.jsonl");
    let body = lines
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body + "\n").expect("write scratch records");
    path
}

// --- eval: a declared per-slice gate must be evaluated, not skipped ------

#[test]
fn eval_flags_a_declared_slice_gate_the_combined_estimate_would_hide() {
    let records = write_slice_fail_records();
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--records",
        records.to_str().expect("utf8 path"),
        "--dataset",
        "SupportEvalV3",
        "--property",
        "LooseQuality",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "statistically_unsupported");
    let checks = value["checks"].as_array().expect("checks array");
    let slice_check = checks
        .iter()
        .find(|check| check["slice"] == "JapaneseRefundTickets" && check["metric"] == "accuracy")
        .expect("JapaneseRefundTickets accuracy check");
    assert_eq!(slice_check["status"], "statistically_unsupported");
    assert_eq!(slice_check["n"], 5);
}

// --- eval: the documented positive path is unaffected --------------------

#[test]
fn eval_supports_the_documented_property_without_records_via_dataset_source() {
    // `docs/LANGUAGE.md` documents this exact invocation with no
    // `--records`, relying on the declared `dataset SupportEvalV3 { source
    // "..."; }` fallback -- the pre-fix implementation required `--records`
    // unconditionally and rejected this with exit 2.
    let (value, status) = run(&[
        "ai",
        "eval",
        "examples/ai/support_answer_quality.fsl",
        "--property",
        "LooseQuality",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "statistically_supported");
}

// --- regress: unknown migration/property selection must error, not run --

#[test]
fn regress_rejects_an_unknown_migration_name() {
    let (value, status) = run(&[
        "ai",
        "regress",
        "examples/ai/support_answer_quality.fsl",
        "--before-records",
        "examples/ai/support_eval_v7.jsonl",
        "--after-records",
        "examples/ai/support_eval_v8_regressed.jsonl",
        "--migration",
        "DoesNotExist",
    ]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown ai_migration 'DoesNotExist'")),
        "{value}"
    );
}

#[test]
fn drift_rejects_an_unknown_observed_property_name() {
    let (value, status) = run(&[
        "ai",
        "drift",
        "examples/ai/support_answer_quality.fsl",
        "--logs",
        "examples/ai/runtime_drift_current.jsonl",
        "--property",
        "DoesNotExist",
    ]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown observed_property 'DoesNotExist'")),
        "{value}"
    );
}

#[test]
fn regress_rejects_a_spec_path_that_does_not_exist() {
    // The pre-fix implementation took `_path: &Path` and never read it --
    // even a missing spec produced a statistical verdict.
    let (value, status) = run(&[
        "ai",
        "regress",
        "tests/fixtures/issue_509_does_not_exist.fsl",
        "--before-records",
        "examples/ai/support_eval_v7.jsonl",
        "--after-records",
        "examples/ai/support_eval_v8_regressed.jsonl",
        "--migration",
        "DoesNotExist",
    ]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "io");
}

// --- regress: the declared threshold and declared metric set must govern -

#[test]
fn regress_uses_the_declared_threshold_and_only_the_declared_metric() {
    // This fixture declares only `metric accuracy drop <= 0.50` -- looser
    // than the pre-fix hardcoded 0.05, and it never declares
    // `hallucination_rate` at all. Real fixtures
    // (support_eval_v7/v8_regressed) move accuracy 1.0 -> 0.8 (a 0.2 drop,
    // under 0.50) and hallucination_rate 0.0 -> 0.2. The pre-fix hardcoded
    // checker would still flag both metrics regardless of this file's
    // content; a declaration-driven checker passes cleanly because only the
    // declared, looser accuracy gate applies.
    let path = fixture("issue_509_migration_custom_threshold.fsl")
        .display()
        .to_string();
    let (value, status) = run(&[
        "ai",
        "regress",
        &path,
        "--before-records",
        "examples/ai/support_eval_v7.jsonl",
        "--after-records",
        "examples/ai/support_eval_v8_regressed.jsonl",
        "--migration",
        "LooseAccuracyMigration",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "statistically_supported");
    let checks = value["checks"].as_array().expect("checks array");
    assert_eq!(checks.len(), 1, "{value}");
    assert_eq!(checks[0]["metric"], "accuracy");
    assert_eq!(checks[0]["allowed_delta"], 0.5);
    assert_eq!(value["findings"].as_array().map(Vec::len), Some(0));
}

// --- drift: only the declared observed requirement must govern -----------

#[test]
fn drift_checks_only_the_declared_requirement_not_a_hardcoded_metric() {
    // This fixture declares only `observed(metric.hallucination_rate) <=
    // 0.90` -- it never mentions `refusal_rate` at all.
    // `examples/ai/runtime_drift_current.jsonl` has a real refusal_rate
    // shift the pre-fix hardcoded checker always flags
    // (`|current - baseline| > 0.10`) regardless of what the spec declares.
    // A declaration-driven checker reports `observed_supported` because the
    // only declared requirement (hallucination_rate <= 0.90) is met.
    let path = fixture("issue_509_drift_custom_threshold.fsl")
        .display()
        .to_string();
    let (value, status) = run(&[
        "ai",
        "drift",
        &path,
        "--logs",
        "examples/ai/runtime_drift_current.jsonl",
        "--property",
        "LenientDriftProperty",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "observed_supported");
    let checks = value["checks"].as_array().expect("checks array");
    assert_eq!(checks.len(), 1, "{value}");
    assert_eq!(checks[0]["metric"], "hallucination_rate");
    assert_eq!(value["findings"].as_array().map(Vec::len), Some(0));
}

// --- drift/regress: a real declared violation is still caught ------------

#[test]
fn regress_still_catches_a_real_declared_regression() {
    let (value, status) = run(&[
        "ai",
        "regress",
        "examples/ai/support_answer_quality.fsl",
        "--migration",
        "PromptV7ToV8",
        "--before-records",
        "examples/ai/support_eval_v7.jsonl",
        "--after-records",
        "examples/ai/support_eval_v8_regressed.jsonl",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "statistically_unsupported");
    assert_eq!(value["findings"].as_array().map(Vec::len), Some(2));
}

#[test]
fn drift_still_catches_a_real_declared_drift() {
    let (value, status) = run(&[
        "ai",
        "drift",
        "examples/ai/support_answer_quality.fsl",
        "--logs",
        "examples/ai/runtime_drift_current.jsonl",
        "--baseline-logs",
        "examples/ai/runtime_drift_baseline.jsonl",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "observed_mismatch");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    assert_eq!(findings[0]["kind"], "ai_observed_drift");
}
