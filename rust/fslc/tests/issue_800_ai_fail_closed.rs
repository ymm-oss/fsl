// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #800: `fslc ai check` / `ai compat` / `ai replay`
//! must fail closed on invalid `ai_component` declarations instead of emitting
//! success verdicts (`ai_project_analyzed`, `compat_profile_generated`,
//! `replay_conformant`) with exit 0.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const LOGS: &str = "rust/fslc/tests/fixtures/error_envelope_empty_records.json";
const VALID_COMPONENT: &str = "examples/ai/refund_agent_tool_safety.fsl";
const VALID_PROJECT: &str = "examples/ai/support_answer_quality.fsl";

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

fn assert_rejects_ai_semantics(args: &[&str]) {
    let (value, status) = run(args);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "semantics", "{value}");
}

fn assert_rejects_ai_parse(args: &[&str]) {
    let (value, status) = run(args);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "parse", "{value}");
}

// --- measured false greens: component + invalid hard rule -------------------

#[test]
fn ai_compat_rejects_a_component_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "compat", &path]);
}

#[test]
fn ai_replay_rejects_a_component_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "replay", &path, "--logs", LOGS]);
}

#[test]
fn ai_check_still_rejects_a_component_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "check", &path]);
}

// --- measured false greens: component + unknown authority tool --------------

#[test]
fn ai_compat_rejects_a_component_with_an_unknown_authority_tool() {
    let path = fixture("error_envelope_ai_unknown_tool.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "compat", &path]);
}

#[test]
fn ai_replay_rejects_a_component_with_an_unknown_authority_tool() {
    let path = fixture("error_envelope_ai_unknown_tool.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "replay", &path, "--logs", LOGS]);
}

#[test]
fn ai_check_still_rejects_a_component_with_an_unknown_authority_tool() {
    let path = fixture("error_envelope_ai_unknown_tool.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "check", &path]);
}

// --- measured false greens: project + invalid hard rule ---------------------

#[test]
fn ai_check_rejects_a_project_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_project_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "check", &path]);
}

#[test]
fn ai_compat_rejects_a_project_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_project_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "compat", &path]);
}

#[test]
fn ai_replay_rejects_a_project_with_an_unregistered_hard_rule() {
    let path = fixture("error_envelope_ai_project_invalid_rule.fsl")
        .display()
        .to_string();
    assert_rejects_ai_semantics(&["ai", "replay", &path, "--logs", LOGS]);
}

// --- measured false green: parse-broken project on replay only --------------

#[test]
fn ai_replay_rejects_a_parse_broken_ai_project() {
    let path = fixture("error_envelope_broken_ai_project.fsl")
        .display()
        .to_string();
    assert_rejects_ai_parse(&["ai", "replay", &path, "--logs", LOGS]);
}

#[test]
fn ai_check_and_compat_still_reject_a_parse_broken_ai_project() {
    let path = fixture("error_envelope_broken_ai_project.fsl")
        .display()
        .to_string();
    assert_rejects_ai_parse(&["ai", "check", &path]);
    let (value, status) = run(&["ai", "compat", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_ne!(
        value["kind"], "parse",
        "compat parse-kind asymmetry remains tracked separately"
    );
}

// --- positive controls: valid inputs still succeed --------------------------

#[test]
fn valid_component_inputs_still_succeed_on_all_three_commands() {
    let (check, check_status) = run(&["ai", "check", VALID_COMPONENT]);
    assert_eq!(check_status, 0, "{check}");
    assert_ne!(check["result"], "error", "{check}");

    let (compat, compat_status) = run(&["ai", "compat", VALID_COMPONENT]);
    assert_eq!(compat_status, 0, "{compat}");
    assert_eq!(compat["result"], "compat_profile_generated", "{compat}");

    let (replay, replay_status) = run(&["ai", "replay", VALID_COMPONENT, "--logs", LOGS]);
    assert_eq!(replay_status, 0, "{replay}");
    assert_eq!(replay["result"], "replay_conformant", "{replay}");
}

#[test]
fn valid_project_inputs_still_succeed_on_all_three_commands() {
    let (check, check_status) = run(&["ai", "check", VALID_PROJECT]);
    assert_eq!(check_status, 0, "{check}");
    assert_eq!(check["result"], "ai_project_analyzed", "{check}");

    let (compat, compat_status) = run(&["ai", "compat", VALID_PROJECT]);
    assert_eq!(compat_status, 0, "{compat}");
    assert_eq!(compat["result"], "compat_profile_generated", "{compat}");

    let (replay, replay_status) = run(&["ai", "replay", VALID_PROJECT, "--logs", LOGS]);
    assert_eq!(replay_status, 0, "{replay}");
    assert_eq!(replay["result"], "replay_conformant", "{replay}");
}
