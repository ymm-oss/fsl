// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #511: `fslc ai compat` line-scanned any
//! readable file with the legacy `ai_project_summary` text scanner and
//! always reported `compat_profile_generated` with exit 0, even when the
//! input was not an AI project/component at all -- the generated
//! `dbsystem` fragment was syntactically empty (`artifact  { requires ;
//! provides ; }`), a false-success profile indistinguishable from a
//! genuine clean result.
//!
//! Native now parses either a single `ai_component` document or a full
//! fsl-ai project (`fsl_syntax::parse_ai_project`) and rejects wrong-dialect
//! input and an AI project with no `ai_component` at all, both with exit 2.

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

// --- negative: non-AI input must be rejected, not emit an empty profile --

#[test]
fn rejects_a_non_ai_spec_instead_of_an_empty_profile() {
    let (value, status) = run(&["ai", "compat", "specs/cart_v1.fsl"]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
    // The pre-fix bug reported `compat_profile_generated` with exit 0 and an
    // empty `artifact  { requires ; provides ; }` fragment for this exact
    // input; assert that shape is entirely gone, not merely that some error
    // happened to be returned.
    assert!(value.get("profiles").is_none(), "{value}");
    assert!(value.get("dbsystem_fragment").is_none(), "{value}");
}

#[test]
fn rejects_an_ai_project_that_declares_no_ai_component() {
    let path = fixture("issue_511_ai_project_without_component.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "compat", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "semantics");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("no ai_component")),
        "{value}"
    );
}

// --- positive: a genuine AI input still produces a real, non-empty profile

#[test]
fn a_single_ai_component_produces_a_real_capability_profile() {
    let (value, status) = run(&["ai", "compat", "examples/ai/refund_agent_tool_safety.fsl"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "compat_profile_generated");
    let profiles = value["profiles"].as_array().expect("profiles array");
    assert_eq!(profiles.len(), 1, "{value}");
    assert_eq!(profiles[0]["artifact"], "refund_agent_tool_safety");
    assert_eq!(profiles[0]["component"], "RefundAgentToolSafety");
    let requires = profiles[0]["requires"]
        .as_array()
        .expect("requires array")
        .iter()
        .map(|value| value.as_str().expect("string").to_owned())
        .collect::<Vec<_>>();
    // Every declared tool has a schema in this fixture -- the profile must
    // use `tool.<schema>`, not the pre-fix line-scanner's bare `tool.<name>`.
    assert!(
        requires.contains(&"model.refund_model_v1".to_owned()),
        "{requires:?}"
    );
    assert!(
        requires.contains(&"tool.SearchOrderV1".to_owned()),
        "{requires:?}"
    );
    assert!(
        !requires.contains(&"tool.SearchOrder".to_owned()),
        "{requires:?}"
    );
    assert_eq!(
        profiles[0]["provides"],
        serde_json::json!(["output.RefundDecisionV1"])
    );
    let fragment = value["dbsystem_fragment"].as_str().expect("fragment");
    assert!(
        fragment.starts_with("artifact refund_agent_tool_safety {"),
        "{fragment}"
    );
    assert!(!fragment.contains("requires ;"), "{fragment}");
    assert!(!fragment.contains("provides ;"), "{fragment}");
}

#[test]
fn an_fsl_ai_project_produces_one_profile_per_declared_component() {
    let (value, status) = run(&[
        "ai",
        "compat",
        "examples/ai/support_answer_quality.fsl",
        "--environment",
        "prod",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "compat_profile_generated");
    assert_eq!(value["environment"], "prod");
    let profiles = value["profiles"].as_array().expect("profiles array");
    assert_eq!(profiles.len(), 1, "{value}");
    assert_eq!(profiles[0]["component"], "SupportAnswerAgent");
    assert!(
        profiles[0]["requires"]
            .as_array()
            .is_some_and(|values| !values.is_empty()),
        "{value}"
    );
    assert!(
        profiles[0]["provides"]
            .as_array()
            .is_some_and(|values| !values.is_empty()),
        "{value}"
    );
}
