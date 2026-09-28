// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The business dialect must reject the transition clauses it cannot lower
//! (#1109).
//!
//! `rust/fsl-core/src/dialect.rs` parses `with`, `when` and `set` on a
//! business transition through the same `process_item` the requirements
//! dialect uses, but `lower_business` never reads `inputs`, `guard` or
//! `assignments`. Before this test the three fixtures below were accepted:
//! `check` answered `ok` for a guard that was never applied, an input that
//! was never bound, and an assignment to a field that was never declared, and
//! `verify` then answered for that other model. The contract is fail closed
//! --- a positioned error naming the clause --- not a silent drop.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/issue_1109_business_transition_clauses";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run(command: &str, fixture: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .arg(format!("{FIXTURE_DIR}/{fixture}"))
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; command={command}; fixture={fixture}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// The three clauses, each alone, on an otherwise accepted process.
///
/// Each fixture differs from `control.fsl` only by its clause, so a failure
/// here cannot be explained by anything else in the spec. The asserted line
/// and column are the transition's own, which is what the author has to edit;
/// asserting them is what separates "a positioned error" from "an error".
#[test]
fn business_transition_clauses_are_positioned_errors() {
    for (fixture, clause) in [
        ("inputs.fsl", "with"),
        ("guard.fsl", "when"),
        ("assignments.fsl", "set"),
    ] {
        let (envelope, code) = run("check", fixture);
        assert_eq!(code, 2, "{fixture}: exit status; envelope={envelope}");
        assert_eq!(envelope["result"], "error", "{fixture}: {envelope}");
        assert_eq!(envelope["kind"], "semantics", "{fixture}: {envelope}");
        let message = envelope["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{fixture}: no message; envelope={envelope}"));
        assert!(
            message.contains(&format!("declares '{clause}'")),
            "{fixture}: message must name the clause; got {message}"
        );
        assert!(
            message.contains("transition 'approve'"),
            "{fixture}: message must name the transition; got {message}"
        );
        assert!(
            message.contains("'requirements'"),
            "{fixture}: message must point at the dialect that accepts the clause; got {message}"
        );
        assert!(
            message.ends_with(" at 9:5"),
            "{fixture}: message must carry the transition position; got {message}"
        );
    }
}

/// The control: the same process without the clauses stays accepted, by both
/// `check` and `verify`, so the error above is attributable to the clause and
/// not to a business process having become unlowerable.
#[test]
fn business_process_without_the_clauses_is_unchanged() {
    let (envelope, code) = run("check", "control.fsl");
    assert_eq!(code, 0, "control check: envelope={envelope}");
    assert_eq!(envelope["result"], "ok", "control check: {envelope}");

    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify", &format!("{FIXTURE_DIR}/control.fsl")])
        .args(["--depth", "3", "--deadlock", "ignore"])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("verify envelope");
    assert_eq!(
        output.status.code(),
        Some(0),
        "control verify: envelope={envelope}"
    );
    assert_eq!(envelope["result"], "verified", "control verify: {envelope}");
}
