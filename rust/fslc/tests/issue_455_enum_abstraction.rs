// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::Value;

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

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
}

#[test]
#[allow(clippy::too_many_lines)]
fn cli_accepts_source_total_abstraction_and_rejects_wrong_or_incomplete_mappings() {
    let scratch = std::env::temp_dir().join(format!("fslc-issue-455-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let implementation = scratch.join("impl.fsl");
    let abstraction = scratch.join("abs.fsl");
    let correct = scratch.join("correct.fsl");
    let wrong = scratch.join("wrong.fsl");
    let incomplete = scratch.join("incomplete.fsl");
    write(
        &implementation,
        "spec Impl { enum ImplStage { C, B, A } state { stage: ImplStage } init { stage = A } action hold() { requires stage == A stage = B } action advance() { requires stage == B stage = C } }",
    );
    write(
        &abstraction,
        "spec Abs { enum AbsStage { Y, X, Unused } state { status: AbsStage } init { status = X } action hold() { requires status == X status = X } action advance() { requires status == X status = Y } }",
    );
    let mapping = "refinement R { impl Impl abs Abs enum abstraction stage ImplStage -> AbsStage { A -> X B -> X C -> Y } map status = abstract(stage, stage) action hold() -> hold() action advance() -> advance() }";
    write(&correct, mapping);
    write(&wrong, &mapping.replace("C -> Y", "C -> X"));
    write(&incomplete, &mapping.replace("C -> Y", ""));

    let base = [
        "refine".to_owned(),
        implementation.display().to_string(),
        abstraction.display().to_string(),
    ];
    let (success, status) = run(&[
        base[0].clone(),
        base[1].clone(),
        base[2].clone(),
        correct.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 0, "{success}");
    assert_eq!(success["result"], "refines");

    let (failure, status) = run(&[
        base[0].clone(),
        base[1].clone(),
        base[2].clone(),
        wrong.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 1, "{failure}");
    assert_ne!(failure["result"], "refines");
    assert!(failure["impl_trace"].is_array(), "{failure}");

    let (type_error, status) = run(&[
        base[0].clone(),
        base[1].clone(),
        base[2].clone(),
        incomplete.display().to_string(),
    ]);
    assert_eq!(status, 2, "{type_error}");
    assert_eq!(type_error["kind"], "type");
    assert!(type_error["loc"].is_object(), "{type_error}");
    assert!(type_error["span"].is_object(), "{type_error}");
    assert!(
        type_error["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing source: [C]"))
    );

    let (replay, status) = run(&[
        "replay".to_owned(),
        abstraction.display().to_string(),
        "--from-log".to_owned(),
        scratch.join("not-read.jsonl").display().to_string(),
        "--mapping".to_owned(),
        correct.display().to_string(),
    ]);
    assert_eq!(status, 2, "{replay}");
    assert_eq!(replay["kind"], "type");
    assert!(replay["loc"].is_object(), "{replay}");
    assert!(
        replay["message"]
            .as_str()
            .is_some_and(|message| message.contains("typed impl model"))
    );

    let binder_call = scratch.join("binder-call.fsl");
    write(
        &binder_call,
        "refinement R { impl External abs Abs map status[k: AbsStage where abstract(missing, k) == k] = X }",
    );
    let (binder_replay, status) = run(&[
        "replay".to_owned(),
        abstraction.display().to_string(),
        "--from-log".to_owned(),
        scratch.join("not-read.jsonl").display().to_string(),
        "--mapping".to_owned(),
        binder_call.display().to_string(),
    ]);
    assert_eq!(status, 2, "{binder_replay}");
    assert_eq!(binder_replay["kind"], "type");
    assert!(binder_replay["loc"].is_object(), "{binder_replay}");
    assert!(
        binder_replay["message"]
            .as_str()
            .is_some_and(|message| message.contains("typed impl model"))
    );

    let (diff, status) = run(&[
        "diff".to_owned(),
        abstraction.display().to_string(),
        implementation.display().to_string(),
        "--mapping".to_owned(),
        correct.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 0, "{diff}");
    assert_eq!(diff["directions"]["new_to_old"]["result"], "refines");
    assert_eq!(diff["directions"]["old_to_new"]["result"], "unknown");

    let (wrong_diff, status) = run(&[
        "diff".to_owned(),
        abstraction.display().to_string(),
        implementation.display().to_string(),
        "--mapping".to_owned(),
        wrong.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 0, "{wrong_diff}");
    assert_eq!(
        wrong_diff["directions"]["new_to_old"]["result"],
        "refinement_failed"
    );
    assert!(
        wrong_diff["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|item| {
                item["kind"] == "behavior_added" && item["direction"] == "new_to_old"
            })),
        "{wrong_diff}"
    );
}

#[test]
fn inline_implements_supports_abstraction_and_keeps_errors_located() {
    let scratch =
        std::env::temp_dir().join(format!("fslc-issue-455-inline-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    write(
        &scratch.join("abs.fsl"),
        "spec Abs { enum AbsStage { X, Y, Unused } state { status: AbsStage } init { status = X } action step() { status = Y } }",
    );
    let implementation = scratch.join("impl.fsl");
    let source = r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum abstraction stage ImplStage -> AbsStage { A -> X B -> X C -> Y }
    map status = abstract(stage, stage)
    action step() -> step()
  }
  enum ImplStage { C, B, A }
  state { stage: ImplStage }
  init { stage = A }
  action step() { stage = C }
}"#;
    write(&implementation, source);
    let (success, status) = run(&["check".to_owned(), implementation.display().to_string()]);
    assert_eq!(status, 0, "{success}");

    write(&implementation, &source.replace(" C -> Y", ""));
    let (failure, status) = run(&["check".to_owned(), implementation.display().to_string()]);
    assert_eq!(status, 2, "{failure}");
    assert_eq!(failure["kind"], "type");
    assert_eq!(failure["loc"], serde_json::json!({"line": 3, "column": 5}));
    assert!(failure["span"].is_object(), "{failure}");
}
