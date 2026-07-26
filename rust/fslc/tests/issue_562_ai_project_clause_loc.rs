// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #562, first half: the fsl-ai project parser tracked no positions, so
//! the spec error #542 introduced for an unexecutable `require` clause carried
//! no `loc` at all. `docs/DESIGN-v1.md` §7.2 guarantees every `parse` error
//! carries one.
//!
//! These assert the **exact** line and column of the offending clause, never
//! merely that `loc` is present: a position derived from the enclosing block
//! instead of the clause's own line would satisfy "not null" while pointing a
//! consumer at the wrong line, which is worse than reporting nothing.
//!
//! The second half of #562 — rejecting an unknown *non-*`require` line inside
//! a declaration body — is deliberately not addressed here; it requires
//! deciding a closed body grammar that no current source specifies. See the
//! `CHANGELOG.md` entry and the report on the issue.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

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
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// `require` on line 5, indented two spaces, so column 3.
#[test]
fn a_statistical_clause_reports_its_own_line_and_column() {
    let path = fixture("issue_542_unparseable_statistical_clause.fsl")
        .display()
        .to_string();
    for command in [
        vec!["ai", "check", &path],
        // The generic dispatch shares the envelope and must carry it too.
        vec!["check", &path],
    ] {
        let (value, status) = run(&command);
        assert_eq!(status, 2, "{command:?}: {value}");
        assert_eq!(value["kind"], "parse", "{command:?}: {value}");
        assert_eq!(
            value["loc"],
            json!({"line": 5, "column": 3}),
            "{command:?}: {value}"
        );
    }
}

/// The clause sits inside a nested `slice`, on line 18 at column 5. A `loc`
/// taken from the `statistical_property` block (line 12) or from the `slice`
/// block (line 16) would be non-null and wrong.
#[test]
fn a_clause_nested_in_a_slice_reports_the_clause_line_not_the_block() {
    let path = fixture("issue_562_nested_slice_clause.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "parse", "{value}");
    assert_eq!(value["loc"], json!({"line": 18, "column": 5}), "{value}");
}

/// `observed_property` clauses run through a separate parser; its position
/// must be resolved the same way. The offending clause is on line 13.
#[test]
fn an_observed_clause_reports_its_own_line_and_column() {
    let path = fixture("issue_542_unparseable_observed_clause.fsl")
        .display()
        .to_string();
    let (value, status) = run(&["ai", "check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "parse", "{value}");
    assert_eq!(value["loc"], json!({"line": 13, "column": 3}), "{value}");
}

/// Positive control. Position tracking must not change which projects are
/// accepted: the corpus's only fsl-ai project file still analyzes at exit 0,
/// with its declarations intact. Increasing rejection power at the cost of
/// valid input would be a different regression.
#[test]
fn the_corpus_project_still_analyzes_unchanged() {
    for command in [
        vec!["ai", "check", "examples/ai/support_answer_quality.fsl"],
        vec!["check", "examples/ai/support_answer_quality.fsl"],
    ] {
        let (value, status) = run(&command);
        assert_eq!(status, 0, "{command:?}: {value}");
        assert!(value.get("loc").is_none(), "{command:?}: {value}");
    }
    let (value, _) = run(&["ai", "check", "examples/ai/support_answer_quality.fsl"]);
    assert_eq!(value["result"], "ai_project_analyzed", "{value}");
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
}
