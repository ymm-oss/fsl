// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
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

fn old_requirements() -> &'static str {
    r#"requirements Old {
  state { ready: Bool }
  init { ready = false }
  requirement REQ-1 "prepare then reject" {
    action prepare() { ready = true }
    action reject() { requires ready == false ready = ready }
  }
  forbidden FB-1 "reject after prepare" {
    prepare() reject()
    expect rejected
  }
}"#
}

fn related_new(final_requires: &str) -> String {
    format!(
        r"spec New {{
  state {{ ready: Bool }}
  init {{ ready = false }}
  action prepare() {{ ready = true }}
  action reject() {{ requires {final_requires} ready = ready }}
}}"
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn unrelatable_forbidden_is_unknown_and_can_fail_the_gate() {
    let scratch = std::env::temp_dir().join(format!("fslc-issue-460-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let old = scratch.join("old.fsl");
    let old_domain = scratch.join("old-domain.fsl");
    let missing = scratch.join("missing.fsl");
    let wrong_arity = scratch.join("wrong-arity.fsl");
    let wrong_domain = scratch.join("wrong-domain.fsl");
    let replay_failure = scratch.join("replay-failure.fsl");
    write(&old, old_requirements());
    write(
        &old_domain,
        r#"requirements Old {
  enum OldKind { A }
  state { ready: Bool }
  init { ready = false }
  requirement REQ-1 "prepare then reject" {
    action prepare() { ready = true }
    action reject(value: OldKind) { requires ready == false ready = ready }
  }
  forbidden FB-1 "reject after prepare" {
    prepare() reject(A)
    expect rejected
  }
}"#,
    );
    write(
        &missing,
        r"spec New {
  state { ready: Bool }
  init { ready = false }
  action prepare() { ready = true }
  action other() { ready = ready }
}",
    );
    write(
        &wrong_arity,
        r"spec New {
  state { ready: Bool }
  init { ready = false }
  action prepare() { ready = true }
  action reject(value: Bool) { requires value ready = ready }
}",
    );
    write(
        &wrong_domain,
        r"spec New {
  enum NewKind { A }
  state { ready: Bool }
  init { ready = false }
  action prepare() { ready = true }
  action reject(value: NewKind) { requires value == A ready = ready }
}",
    );
    write(
        &replay_failure,
        r"spec New {
  state { ready: Bool, zero: Int }
  init { ready = false zero = 0 }
  action prepare() { ready = true }
  action reject() { requires 1 / zero == 0 ready = ready }
}",
    );

    for (old, new, reason, step, action, detail) in [
        (
            &old,
            &missing,
            "forbidden_step_unrelatable",
            1,
            "reject",
            "unknown requirement action 'reject'",
        ),
        (
            &old,
            &wrong_arity,
            "forbidden_step_unrelatable",
            1,
            "reject",
            "no variant with 0 argument(s)",
        ),
        (
            &old_domain,
            &wrong_domain,
            "forbidden_step_unrelatable",
            1,
            "reject",
            "do not belong to the NEW action domain",
        ),
        (
            &old,
            &replay_failure,
            "forbidden_replay_failed",
            0,
            "prepare",
            "division by zero",
        ),
    ] {
        let (result, status) = run(&[
            "diff".to_owned(),
            old.display().to_string(),
            new.display().to_string(),
            "--depth".to_owned(),
            "2".to_owned(),
            "--forbid".to_owned(),
            "unknown".to_owned(),
        ]);
        assert_eq!(status, 1, "{result}");
        assert!(
            result["summary"]
                .as_array()
                .is_some_and(|summary| summary.iter().any(|kind| kind == "unknown"))
        );
        let finding = result["findings"]
            .as_array()
            .and_then(|findings| {
                findings.iter().find(|finding| {
                    finding["kind"] == "unknown"
                        && finding["subject"] == "forbidden"
                        && finding["id"] == "FB-1"
                })
            })
            .unwrap_or_else(|| panic!("missing forbidden unknown: {result}"));
        assert_eq!(finding["reason"], reason);
        assert_eq!(finding["step"], step);
        assert_eq!(finding["action"], action);
        assert!(
            finding["detail"]
                .as_str()
                .is_some_and(|message| message.contains(detail))
        );
        assert_eq!(result["gate"]["passed"], false);
        assert_eq!(result["gate"]["violations"], serde_json::json!(["unknown"]));
    }

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

#[test]
fn strict_diff_relation_does_not_replace_structured_check_failures() {
    let scratch = std::env::temp_dir().join(format!(
        "fslc-issue-460-check-sibling-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let source = scratch.join("broken-setup.fsl");
    write(
        &source,
        r#"requirements BrokenSetup {
  state { ready: Bool }
  init { ready = false }
  action reject() { requires ready ready = ready }
  forbidden FB-1 "missing setup stays structured" {
    missing() reject()
    expect rejected
  }
}"#,
    );

    let (result, status) = run(&["check".to_owned(), source.display().to_string()]);
    assert_eq!(status, 2, "{result}");
    assert_eq!(result["result"], "error");
    assert_eq!(result["kind"], "forbidden_setup");
    assert_eq!(result["id"], "FB-1");
    assert_eq!(result["failed_step"], 0);
    assert_eq!(result["step"]["action"], "missing");
    assert!(result["loc"].is_object());
    assert_eq!(result["trace_type"], "forbidden");

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

#[test]
fn related_guard_rejection_is_preserved_but_acceptance_is_relaxed() {
    let scratch = std::env::temp_dir().join(format!(
        "fslc-issue-460-negative-control-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let old = scratch.join("old.fsl");
    let preserved = scratch.join("preserved.fsl");
    let relaxed = scratch.join("relaxed.fsl");
    write(&old, old_requirements());
    write(&preserved, &related_new("ready == false"));
    write(&relaxed, &related_new("ready == true"));

    let (preserved_result, status) = run(&[
        "diff".to_owned(),
        old.display().to_string(),
        preserved.display().to_string(),
        "--depth".to_owned(),
        "2".to_owned(),
    ]);
    assert_eq!(status, 0, "{preserved_result}");
    assert_eq!(
        preserved_result["summary"],
        serde_json::json!(["no_semantic_change"])
    );
    assert!(
        preserved_result["findings"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let (relaxed_result, status) = run(&[
        "diff".to_owned(),
        old.display().to_string(),
        relaxed.display().to_string(),
        "--depth".to_owned(),
        "2".to_owned(),
    ]);
    assert_eq!(status, 0, "{relaxed_result}");
    assert!(
        relaxed_result["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| {
                finding["kind"] == "forbidden_relaxed" && finding["id"] == "FB-1"
            }))
    );

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}
