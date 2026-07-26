// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

#[test]
fn process_stage_refinement_accepts_explicit_conversion_and_rejects_non_total_mapping() {
    let design = fixture("issue_450_design.fsl").display().to_string();
    let requirements = fixture("issue_450_requirements.fsl").display().to_string();
    let mapping = fixture("issue_450_mapping.fsl").display().to_string();
    let (success, status) = run(&[
        "refine".to_owned(),
        design.clone(),
        requirements.clone(),
        mapping.clone(),
        "--depth".to_owned(),
        "2".to_owned(),
    ]);
    assert_eq!(status, 0, "{success}");
    assert_eq!(success["result"], "refines");

    let incomplete = fixture("issue_450_mapping_incomplete.fsl")
        .display()
        .to_string();
    let (failure, status) = run(&[
        "refine".to_owned(),
        design,
        requirements.clone(),
        incomplete,
        "--depth".to_owned(),
        "2".to_owned(),
    ]);
    assert_eq!(status, 2, "{failure}");
    assert_eq!(failure["kind"], "type");
    assert!(failure["loc"].is_object(), "{failure}");
    assert!(failure["span"].is_object(), "{failure}");
    assert!(failure["message"].as_str().is_some_and(|message| {
        message.contains("missing source: [Conflict]; missing target: [Conflict]")
    }));

    let (replay, status) = run(&[
        "replay".to_owned(),
        requirements,
        "--from-log".to_owned(),
        "not-read.jsonl".to_owned(),
        "--mapping".to_owned(),
        mapping,
    ]);
    assert_eq!(status, 2, "{replay}");
    assert_eq!(replay["kind"], "type");
    assert!(replay["loc"].is_object(), "{replay}");
    assert!(
        replay["message"]
            .as_str()
            .is_some_and(|message| { message.contains("typed impl model") })
    );
}

#[test]
fn inline_implements_enum_conversion_errors_keep_their_location() {
    let scratch =
        std::env::temp_dir().join(format!("fslc-issue-450-inline-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    std::fs::write(
        scratch.join("abs.fsl"),
        "spec Abs { enum AbsStage { A, B } state { status: AbsStage } init { status = A } action step() { status = B } }",
    )
    .expect("write abstraction");
    let implementation = scratch.join("impl.fsl");
    std::fs::write(
        &implementation,
        r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum conversion stage ImplStage -> AbsStage {
      A -> A
    }
    map status = convert(stage, stage)
    action step() -> step()
  }
  enum ImplStage { A, B }
  state { stage: ImplStage }
  init { stage = A }
  action step() { stage = B }
}
"#,
    )
    .expect("write implementation");

    let (failure, status) = run(&["check".to_owned(), implementation.display().to_string()]);
    assert_eq!(status, 2, "{failure}");
    assert_eq!(failure["kind"], "type");
    assert_eq!(failure["loc"], serde_json::json!({"line": 3, "column": 5}));
    assert!(failure["span"].is_object(), "{failure}");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing source: [B]"))
    );
}

#[test]
fn empty_enum_conversion_is_a_located_type_error_not_a_panic() {
    let scratch = std::env::temp_dir().join(format!("fslc-issue-450-empty-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let implementation = scratch.join("impl.fsl");
    let abstraction = scratch.join("abs.fsl");
    let mapping = scratch.join("mapping.fsl");
    std::fs::write(
        &implementation,
        "spec Impl { enum EmptyImpl {} state { ready: Bool } init { ready = false } action send(s: EmptyImpl) { requires false } }",
    )
    .expect("write implementation");
    std::fs::write(
        &abstraction,
        "spec Abs { enum EmptyAbs {} state { ready: Bool } init { ready = false } action send(s: EmptyAbs) { requires false } }",
    )
    .expect("write abstraction");
    std::fs::write(
        &mapping,
        "refinement R { impl Impl abs Abs enum conversion empty EmptyImpl -> EmptyAbs {} map ready = ready action send(s) -> send(convert(empty, s)) }",
    )
    .expect("write mapping");

    let (failure, status) = run(&[
        "refine".to_owned(),
        implementation.display().to_string(),
        abstraction.display().to_string(),
        mapping.display().to_string(),
    ]);
    assert_eq!(status, 2, "{failure}");
    assert_eq!(failure["kind"], "type");
    assert!(failure["loc"].is_object(), "{failure}");
    assert!(failure["span"].is_object(), "{failure}");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("EmptyImpl' has no members"))
    );
}
