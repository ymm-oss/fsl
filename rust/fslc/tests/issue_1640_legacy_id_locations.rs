// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1640: a normalized legacy ID is located at its first written token,
//! never at matching description text or at the declaration keyword.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn source(role: &str, id: &str, description: &str) -> String {
    // Keep the tested header on line 4, with two leading ASCII spaces.
    // Acceptance and forbidden need legal scenario bodies, not requirement bodies.
    let body = match role {
        "requirement" => "{\n    action go() { requires x == 0 x = 1 }\n  }",
        "acceptance" => "{ go() expect x == 1 }\n  action go() { requires x == 0 x = 1 }",
        "forbidden" => "{ go() go() expect rejected }\n  action go() { requires x == 0 x = 1 }",
        _ => panic!("unknown legacy role {role}"),
    };
    format!(
        "requirements R {{\n  state {{ x: 0..1 }}\n  init {{ x = 0 }}\n  {role} {id} \"{description}\" {body}\n}}\n"
    )
}

fn check(label: &str, source: &str, expected_exit: i32) -> (Value, PathBuf) {
    // Retain the source and unmodified process evidence even when an assertion
    // fails. PID + distinct case labels isolate parallel tests and repeated runs.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".codex/tmp")
        .join(format!("issue-1640-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create evidence directory");
    let path = dir.join("source.fsl");
    std::fs::write(&path, source).expect("write source fixture");
    let binary = env!("CARGO_BIN_EXE_fslc");
    let output = Command::new(binary)
        .arg("check")
        .arg(&path)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native fslc");
    std::fs::write(dir.join("stdout.json"), &output.stdout).expect("retain raw stdout");
    std::fs::write(dir.join("stderr.txt"), &output.stderr).expect("retain raw stderr");
    std::fs::write(
        dir.join("invocation.json"),
        serde_json::to_vec_pretty(&json!({
            "argv": [binary, "check", path],
            "env": {"FSLC_CACHE": "off"},
            "exit": output.status.code(),
        }))
        .expect("serialize invocation"),
    )
    .expect("retain invocation");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: invalid JSON ({error}); evidence: {}",
            dir.display()
        )
    });
    assert!(output.stderr.is_empty(), "{label}: {output:?}");
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{label}: {value:#}"
    );
    (value, path)
}

fn rejected(label: &str, role: &str, id: &str, description: &str, column: u32) {
    let (value, path) = check(label, &source(role, id, description), 2);
    // Existing acceptance diagnostics deliberately use the role name scenario.
    let position = if role == "acceptance" {
        "scenario"
    } else {
        role
    };
    assert_eq!(value["result"], "error", "{label}: {value:#}");
    assert_eq!(value["kind"], "name", "{label}: {value:#}");
    let message = format!(
        "{position} name 'REQ-__1' contains '__', which is reserved for the names FSL generates \
         (a compose component 'alias__name', a branch 'action__b1') and published with '.' in \
         its place; rename the declaration without '__' at {}:4:{column}",
        path.display()
    );
    // Compare both location surfaces together so a baseline failure shows the
    // actual structured position AND the complete source-stamped #1256 message.
    assert_eq!(
        json!({"loc": value["loc"], "message": value["message"]}),
        json!({"loc": {"line": 4, "column": column}, "message": message}),
        "{label}: {value:#}"
    );
}

// Independent literal oracles: 2 spaces + keyword lengths 11/10/9 + 1 space
// put the first R of REQ at columns 15/14/13. No source search or normalizer
// computes these expectations. Each detector is its own test so none is hidden
// behind another role's first assertion failure.
#[test]
fn requirement_repeated_description() {
    rejected(
        "requirement_repeated",
        "requirement",
        "REQ - __1",
        "REQ-__1",
        15,
    );
}

#[test]
fn requirement_ordinary_description() {
    rejected(
        "requirement_ordinary",
        "requirement",
        "REQ - __1",
        "split id",
        15,
    );
}

#[test]
fn acceptance_repeated_description() {
    rejected(
        "acceptance_repeated",
        "acceptance",
        "REQ - __1",
        "REQ-__1",
        14,
    );
}

#[test]
fn acceptance_ordinary_description() {
    rejected(
        "acceptance_ordinary",
        "acceptance",
        "REQ - __1",
        "split id",
        14,
    );
}

#[test]
fn forbidden_repeated_description() {
    rejected(
        "forbidden_repeated",
        "forbidden",
        "REQ - __1",
        "REQ-__1",
        13,
    );
}

#[test]
fn forbidden_ordinary_description() {
    rejected(
        "forbidden_ordinary",
        "forbidden",
        "REQ - __1",
        "split id",
        13,
    );
}

fn valid_controls(role: &str) {
    // Actually check each legal body with split, contiguous, and zero-padded
    // spellings. These are baseline preservation controls, not location detectors.
    for (style, id) in [
        ("split", "REQ - 1"),
        ("contiguous", "REQ-1"),
        ("split_zero_padded", "REQ - 001"),
        ("contiguous_zero_padded", "REQ-001"),
    ] {
        let label = format!("valid_{role}_{style}");
        let (value, _) = check(&label, &source(role, id, "valid id"), 0);
        assert_eq!(value["result"], "ok", "{label}: {value:#}");
    }
}

#[test]
fn valid_requirement_controls() {
    valid_controls("requirement");
}

#[test]
fn valid_acceptance_controls() {
    valid_controls("acceptance");
}

#[test]
fn valid_forbidden_controls() {
    valid_controls("forbidden");
}

#[test]
fn contiguous_reserved_ids_keep_their_locations() {
    for (role, column) in [("requirement", 15), ("acceptance", 14), ("forbidden", 13)] {
        rejected(
            &format!("contiguous_{role}"),
            role,
            "REQ-__1",
            "REQ-__1",
            column,
        );
    }
}
