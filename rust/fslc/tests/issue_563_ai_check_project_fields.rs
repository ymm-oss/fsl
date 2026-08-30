// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #563: native `fslc ai check` on an fsl-ai
//! project omitted six fields the frozen reference's `analyze_ai_project`
//! (`src/fslc/ai_project.py`) emits — `ai_project`, `assumptions`, `datasets`,
//! `dialect`, `evaluators`, `failure_modes`. `evaluators` and `failure_modes`
//! were not merely unprojected: the Rust project parser did not descend into
//! `evaluator` / `failure_mode` blocks at all, so the data did not exist.
//! `skills/fsl/references/syntax.md` states `failure_mode` is listed under
//! `failure_modes`, so the skill promised agents output native never produced.
//!
//! Key presence alone cannot distinguish an empty array from a missing
//! projection, so the controls assert the actual declared names from
//! `examples/ai/support_answer_quality.fsl`, which declares one `dataset`, one
//! `evaluator`, and one `failure_mode`.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const PROJECT: &str = "examples/ai/support_answer_quality.fsl";

/// Every key `analyze_ai_project` returns, measured against the frozen
/// reference rather than restated from the issue text.
const FROZEN_KEYS: &[&str] = &[
    "ai_project",
    "assumptions",
    "components",
    "datasets",
    "dialect",
    "evaluators",
    "failure_modes",
    "formal_result",
    "migrations",
    "observed_properties",
    "raw_blocks",
    "result",
    "statistical_properties",
];

/// Keys native adds on top of the frozen set. `fsl`/`versions` are the native
/// envelope; `findings` is native's fsl-ai finding contract, carried by every
/// `ai` result and not part of this change.
const NATIVE_ONLY_KEYS: &[&str] = &["findings", "fsl", "versions"];

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn ai_check(spec: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["ai", "check", spec])
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

#[test]
fn every_frozen_reference_field_is_present() {
    let (value, status) = ai_check(PROJECT);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ai_project_analyzed", "{value}");
    let object = value.as_object().expect("object envelope");
    let missing = FROZEN_KEYS
        .iter()
        .filter(|key| !object.contains_key(**key))
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "missing {missing:?} from {value}");
}

#[test]
fn native_adds_no_field_beyond_its_envelope_and_findings() {
    // Guards the other direction: a stray key is as much a contract drift as a
    // missing one.
    let (value, _) = ai_check(PROJECT);
    let object = value.as_object().expect("object envelope");
    let extra = object
        .keys()
        .filter(|key| {
            !FROZEN_KEYS.contains(&key.as_str()) && !NATIVE_ONLY_KEYS.contains(&key.as_str())
        })
        .collect::<Vec<_>>();
    assert!(extra.is_empty(), "unexpected {extra:?} in {value}");
}

#[test]
fn the_previously_missing_fields_carry_their_declared_names() {
    // The load-bearing assertion: an empty array would satisfy key presence
    // while proving nothing, so pin the names the fixture actually declares.
    let (value, _) = ai_check(PROJECT);
    assert_eq!(value["ai_project"], "support_answer_quality", "{value}");
    assert_eq!(value["dialect"], "fsl-ai-project.v0", "{value}");
    assert_eq!(value["datasets"], json!(["SupportEvalV3"]), "{value}");
    assert_eq!(
        value["evaluators"],
        json!(["SupportAnswerJudge"]),
        "{value}"
    );
    assert_eq!(value["failure_modes"], json!(["Hallucination"]), "{value}");
    assert_eq!(
        value["assumptions"],
        json!([{
            "id": "AI-ASSUME-EXTERNAL-EVIDENCE-JOBS",
            "text": "statistical, migration, and observed AI declarations are external evidence jobs and do not add probability semantics to fslc verify",
        }]),
        "{value}"
    );
}

#[test]
fn the_fields_that_already_worked_are_unchanged() {
    let (value, _) = ai_check(PROJECT);
    assert_eq!(value["formal_result"], "not_run", "{value}");
    assert_eq!(
        value["components"],
        json!(["SupportAnswerAgent"]),
        "{value}"
    );
    assert_eq!(
        value["statistical_properties"],
        json!(["LooseQuality", "StrictQuality"]),
        "{value}"
    );
    assert_eq!(
        value["observed_properties"],
        json!(["SupportAgentOperationalQuality"]),
        "{value}"
    );
    assert_eq!(value["migrations"], json!(["PromptV7ToV8"]), "{value}");
    assert_eq!(value["findings"], json!([]), "{value}");
    // `evaluator` and `failure_mode` are parsed declarations, so they must not
    // also appear as un-descended raw blocks.
    let raw_kinds = value["raw_blocks"]
        .as_array()
        .expect("raw_blocks")
        .iter()
        .map(|block| block["kind"].as_str().unwrap_or_default().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!raw_kinds.contains("evaluator"), "{value}");
    assert!(!raw_kinds.contains("failure_mode"), "{value}");
}
