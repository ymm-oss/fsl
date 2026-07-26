// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #470: native `ai_component` lowering
//! (`rust/fsl-core/src/dialect.rs::lower_ai_component`) used to be a
//! one-boolean catalog sentinel (`lower_catalog_sentinel`) that could never
//! be violated -- `fslc ai check` reported `verified_under_assumptions`
//! with `findings: []` and `formal_result: "verified"` on specs that
//! violate the documented hard contract, and `check hard { rule <bogus> }`
//! silently parsed as a no-op instead of a check-time error. This is
//! AGENTS.md's "confidently green false negative", on the highest-
//! consequence claim in the dialect (AI tool authority / human-approval
//! safety).
//!
//! Native now lowers the documented `Tool` enum / `human_approved` /
//! `tool_executed` / `tool_suggested` / `fallback_required` state and
//! generated actions/invariants (`docs/LANGUAGE.md` §13.6,
//! `docs/DESIGN-ai-hard.md`), validates `check hard { rule ... }` against
//! the five documented rule names, and `fsl_tools::check_ai`/`replay_ai`
//! implement all five hard rules instead of only a narrowed
//! `human_approval_required`.

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

fn run(args: &[String]) -> (Value, i32) {
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

fn run_str(args: &[&str]) -> (Value, i32) {
    run(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

// --- unknown rule name: check-time error from all three entrypoints -----

#[test]
fn ai_check_rejects_an_unknown_hard_rule_name() {
    let path = fixture("issue_470_unknown_rule.fsl").display().to_string();
    let (value, status) = run_str(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message
                .contains("unknown ai hard-contract rule 'totally_bogus_rule_name'")),
        "{value}"
    );
}

#[test]
fn check_rejects_an_unknown_hard_rule_name() {
    let path = fixture("issue_470_unknown_rule.fsl").display().to_string();
    let (value, status) = run_str(&["check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
}

#[test]
fn verify_rejects_an_unknown_hard_rule_name() {
    let path = fixture("issue_470_unknown_rule.fsl").display().to_string();
    let (value, status) = run_str(&["verify", &path, "--depth", "4"]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
}

// --- the four documented static violations -------------------------------

#[test]
fn ai_check_rejects_a_forbidden_tool_also_declared_executable() {
    let path = fixture("issue_470_tool_authority_violation.fsl")
        .display()
        .to_string();
    let (value, status) = run_str(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    assert_eq!(findings[0]["failed_rule"], "tool_authority");
    assert_eq!(
        findings[0]["violation"],
        "forbidden_tool_declared_executable"
    );
    assert_eq!(findings[0]["tool"], "SearchOrder");
    assert!(
        findings[0]["repair_candidates"]
            .as_array()
            .is_some_and(|candidates| !candidates.is_empty()),
        "{value}"
    );
}

#[test]
fn ai_check_rejects_an_executable_tool_without_a_schema() {
    let path = fixture("issue_470_tool_schema_violation.fsl")
        .display()
        .to_string();
    let (value, status) = run_str(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    assert_eq!(findings[0]["failed_rule"], "tool_schema_declared");
    assert_eq!(findings[0]["violation"], "executable_tool_without_schema");
}

#[test]
fn ai_check_rejects_an_irreversible_tool_without_explicit_human_approval() {
    // Negative control for the narrowed predicate: this tool is only
    // `may_suggest`, never `may_execute`, so the old
    // `irreversible && may_execute.contains && !approvals.contains`
    // predicate would have missed it entirely.
    let path = fixture("issue_470_human_approval_narrow.fsl")
        .display()
        .to_string();
    let (value, status) = run_str(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{value}");
    assert_eq!(findings[0]["failed_rule"], "human_approval_required");
    assert_eq!(
        findings[0]["violation"],
        "irreversible_tool_without_human_approval_guard"
    );
}

// --- positive control: the documented example stays clean ---------------

#[test]
fn ai_check_reports_no_findings_for_the_documented_positive_example() {
    let (value, status) = run_str(&["ai", "check", "examples/ai/refund_agent_tool_safety.fsl"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "verified_under_assumptions");
    assert_eq!(value["findings"].as_array().map(Vec::len), Some(0));
    assert_eq!(value["formal_result"], "verified");
}

// --- kernel expansion is real, not the one-boolean sentinel --------------

#[test]
fn verify_checks_the_documented_generated_invariants_not_a_sentinel() {
    let (value, status) = run_str(&[
        "verify",
        "examples/ai/refund_agent_tool_safety.fsl",
        "--depth",
        "8",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "verified");
    let mut invariants = value["invariants_checked"]
        .as_array()
        .expect("invariants_checked array")
        .iter()
        .map(|name| name.as_str().expect("invariant name").to_owned())
        .collect::<Vec<_>>();
    invariants.sort();
    assert_eq!(
        invariants,
        vec![
            "ai_approval_before_execute.RefundPayment".to_owned(),
            "ai_forbidden_tool_not_executed.DeleteCustomerData".to_owned(),
        ]
    );
    // The pre-fix sentinel lowering emitted exactly these two names instead.
    assert!(
        !invariants
            .iter()
            .any(|name| name.contains("_ai_ok") || name.contains("_ai_catalog_ok"))
    );
}

#[test]
fn kernel_emits_the_documented_tool_enum_and_actions_not_the_noop_sentinel() {
    let (value, status) = run_str(&["kernel", "examples/ai/refund_agent_tool_safety.fsl"]);
    assert_eq!(status, 0, "{value}");
    let actions = value["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .map(|action| action["name"].as_str().expect("action name").to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(actions.contains("execute_RefundPayment"), "{actions:?}");
    assert!(actions.contains("execute_SearchOrder"), "{actions:?}");
    assert!(actions.contains("approve_RefundPayment"), "{actions:?}");
    // No forbidden tool ever gets an execute action generated for it.
    assert!(
        !actions.contains("execute_DeleteCustomerData"),
        "{actions:?}"
    );
    // The pre-fix sentinel lowering emitted exactly one action: `_ai_noop`.
    assert!(!actions.contains("_ai_noop"), "{actions:?}");
    assert!(actions.len() > 1, "{actions:?}");
}

// --- replay: tool_authority and missing-precondition-evidence -----------

#[test]
fn replay_reports_tool_authority_and_missing_precondition_evidence() {
    let path = fixture("issue_470_replay_component.fsl")
        .display()
        .to_string();
    let logs = fixture("issue_470_replay_events.jsonl")
        .display()
        .to_string();
    let (value, status) = run_str(&["ai", "replay", &path, "--logs", &logs]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "replay_nonconformant");
    let findings = value["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 2, "{value}");

    let authority = findings
        .iter()
        .find(|finding| finding["tool"] == "UnauthorizedTool")
        .expect("tool_authority finding");
    assert_eq!(authority["failed_rule"], "tool_authority");
    assert_eq!(authority["violation"], "suggestion_without_authority");

    let precondition = findings
        .iter()
        .find(|finding| finding["tool"] == "AuditTool")
        .expect("precondition finding");
    assert_eq!(precondition["failed_rule"], "tool_precondition_declared");
    assert_eq!(precondition["violation"], "business_precondition_mismatch");
    assert_eq!(
        precondition["witness"]["missing_preconditions"],
        serde_json::json!(["case_open"])
    );
}

#[test]
fn replay_is_conformant_when_authority_and_preconditions_are_satisfied() {
    let path = fixture("issue_470_replay_component.fsl")
        .display()
        .to_string();
    let scratch = std::env::temp_dir().join("issue_470_replay_conformant.jsonl");
    std::fs::write(
        &scratch,
        concat!(
            r#"{"component":"ReplayComponent","event":"tool_call","tool":"AuditTool","mode":"execute","preconditions":{"case_open":true}}"#,
            "\n"
        ),
    )
    .expect("write scratch events");
    let (value, status) = run_str(&[
        "ai",
        "replay",
        &path,
        "--logs",
        scratch.to_str().expect("utf8 path"),
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "replay_conformant");
    assert_eq!(value["findings"].as_array().map(Vec::len), Some(0));
}
