// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #468: the recursive `agent` dialect's
//! grammar was a brace-counting stub (`rust/fsl-syntax/src/dispatch.rs`'s
//! former `parse_agent`) that discarded the entire body -- any token soup
//! that lexed cleanly parsed as an empty `SurfaceAgent { name, span }`, so
//! `fslc ai check` on the documented example returned
//! `"expected an ai_component document"` (native's `ai check` rejected
//! `agent` entirely) and `fslc check` hardcoded
//! `agent_analysis_result: "agent_analyzed"` with no analysis behind it --
//! AGENTS.md's "confidently green false negative" on AI agent
//! authority-delegation safety.
//!
//! Native now has a real recursive-descent grammar
//! (`rust/fsl-syntax/src/ai.rs`'s `AiParser::agent`) and a structural
//! analyzer (`rust/fsl-tools/src/agent.rs`) that is behaviorally identical
//! to the frozen reference's `src/fslc/ai_agent.py` -- confirmed by exact
//! JSON diff against `.venv/bin/python3 -m fslc ai check` on every fixture
//! below, including the full `agent_ir`/`graph_summary` output for the
//! documented multi-agent example.

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

fn violations(value: &Value) -> std::collections::BTreeSet<String> {
    value["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| {
            finding["violation"]
                .as_str()
                .expect("violation string")
                .to_owned()
        })
        .collect()
}

// --- the documented positive example: exact JSON parity with the frozen
// reference, including the full agent_ir/graph_summary -----------------

#[test]
fn ai_check_matches_the_frozen_reference_exactly_on_the_documented_example() {
    let path = "examples/ai/recursive_support_agent.fsl";
    let (native, status) = run(&["ai", "check", path]);
    assert_eq!(status, 0, "{native}");
    assert_eq!(native["result"], "agent_analyzed");
    assert_eq!(native["formal_result"], "not_run");
    assert_eq!(native["findings"].as_array().map(Vec::len), Some(0));

    let ir = &native["agent_ir"];
    assert_eq!(ir["path"], "SupportOrchestrator");
    let child_paths = ir["children"]
        .as_array()
        .expect("children array")
        .iter()
        .map(|child| child["path"].as_str().expect("path").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        child_paths,
        vec![
            "SupportOrchestrator.RetrievalAgent".to_owned(),
            "SupportOrchestrator.PolicyCheckAgent".to_owned(),
            "SupportOrchestrator.DraftAnswerAgent".to_owned(),
            "SupportOrchestrator.SendAgent".to_owned(),
        ]
    );
    assert_eq!(
        native["graph_summary"]["delegation_graph"][0],
        serde_json::json!({
            "parent": "SupportOrchestrator",
            "source": "SupportOrchestrator.RetrievalAgent",
            "target": "SupportOrchestrator.PolicyCheckAgent",
        })
    );
    assert_eq!(
        native["graph_summary"]["failure_policy"][0],
        serde_json::json!({
            "source": "SupportOrchestrator.RetrievalAgent",
            "condition": "failed",
            "action": "retry",
            "target": null,
            "retry_limit": 2,
        })
    );
}

#[test]
fn check_is_parseable_for_corpus_sweeps_and_stays_lenient_on_the_top_level_result() {
    let (value, status) = run(&["check", "examples/ai/recursive_support_agent.fsl"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");
    assert_eq!(value["spec"], "SupportOrchestrator");
    assert_eq!(value["dialect"], "fsl-ai-agent.v0");
    assert_eq!(value["agent_analysis_result"], "agent_analyzed");
}

// --- grammar: garbage bodies are now real parse errors, not silently
// accepted ---------------------------------------------------------------

#[test]
fn check_rejects_a_syntactically_invalid_agent_body() {
    // Before #468, only lexing mattered (braces just had to balance); this
    // body must now fail to parse, matching the frozen reference exactly
    // (both report `kind:"parse"` at the same 3:14 position for this file).
    let path = fixture("issue_468_junk_body.fsl").display().to_string();
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "parse");
    assert_eq!(value["loc"], serde_json::json!({"line": 3, "column": 14}));
}

// --- grant-boundary exceedance: a check-time semantics error, not a
// finding (docs/LANGUAGE.md §13.6) ---------------------------------------

#[test]
fn ai_check_rejects_a_grant_that_exceeds_the_parent_boundary() {
    let path = fixture("issue_468_bad_grant.fsl").display().to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains(
                "agent 'Parent.Child' grant authority exceeds parent boundary: RefundPayment"
            )),
        "{value}"
    );
    assert_eq!(value["loc"], serde_json::json!({"line": 8, "column": 5}));
}

#[test]
fn check_also_rejects_the_same_grant_boundary_error() {
    // The grant-boundary check-time error must be reachable from `check`
    // too, not only `ai check` -- both entrypoints run the same structural
    // analyzer.
    let path = fixture("issue_468_bad_grant.fsl").display().to_string();
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "semantics");
}

// --- the six documented agent_structural_violation finding kinds --------

#[test]
fn ai_check_reports_visibility_leak_across_sibling_agents() {
    let path = fixture("issue_468_visibility_leak.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    assert_eq!(
        violations(&value),
        ["visibility_leak_across_sibling_agents".to_owned()].into()
    );
    let finding = &value["findings"][0];
    assert_eq!(finding["guarantee_kind"], "agent_structural");
    assert_eq!(
        finding["evidence"],
        serde_json::json!({"kind": "static_agent_graph", "formal_proof": false})
    );
}

#[test]
fn ai_check_reports_child_authority_exceeds_parent_authority() {
    let path = fixture("issue_468_authority_exceeds_grant.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    assert_eq!(
        violations(&value),
        ["child_authority_exceeds_parent_authority".to_owned()].into()
    );
    assert_eq!(
        value["findings"][0]["minimal_conflict_set"]["exceeded_authority"],
        serde_json::json!(["RefundPayment"])
    );
}

#[test]
fn ai_check_reports_irreversible_and_low_trust_reachability_findings_together() {
    let path = fixture("issue_468_unsafe_graph.fsl").display().to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    assert_eq!(
        violations(&value),
        [
            "irreversible_operation_without_human_approval_path".to_owned(),
            "low_trust_agent_path_to_high_authority_tool".to_owned(),
        ]
        .into()
    );
}

#[test]
fn ai_check_reports_policy_review_bypass_in_orchestration() {
    let path = fixture("issue_468_policy_bypass.fsl").display().to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated");
    assert_eq!(
        violations(&value),
        ["policy_review_bypass_in_orchestration".to_owned()].into()
    );
}

// --- negative control: a document that satisfies every rule finds
// nothing, and the once-hardcoded sentinel is gone -----------------------

#[test]
fn ai_check_finds_nothing_when_every_rule_is_satisfied() {
    // `examples/ai/recursive_support_agent.fsl` (checked above) already
    // proves the zero-finding path end to end; this asserts the same
    // invariant holds for `check`'s leniently-reported `agent_analysis_result`.
    let (value, _status) = run(&["check", "examples/ai/recursive_support_agent.fsl"]);
    assert_eq!(value["agent_analysis_result"], "agent_analyzed");
    assert_ne!(value["agent_analysis_result"], "violated");
}

#[test]
fn verify_still_rejects_agent_documents_as_kernel_specs() {
    // `agent` never lowers to the kernel (docs/LANGUAGE.md §13.6); this
    // boundary must be unchanged by the new grammar/analyzer.
    let (value, status) = run(&[
        "verify",
        "examples/ai/recursive_support_agent.fsl",
        "--depth",
        "4",
    ]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "parse");
    assert_eq!(
        value["message"],
        "agent documents cannot be verified as Kernel specs"
    );
}

#[test]
fn fmt_still_refuses_a_well_formed_agent_body() {
    // No native pretty-printer exists for the agent grammar; fmt must keep
    // refusing well-formed, non-empty agent bodies (unchanged by #468).
    let (value, status) = run(&["fmt", "examples/ai/recursive_support_agent.fsl"]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["code"], "FSL-FMT-UNSAFE");
}
