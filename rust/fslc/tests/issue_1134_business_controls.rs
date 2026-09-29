// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The business dialect must not discard its control catalog (#1134).
//!
//! `BusinessItem::Control` fell into the catch-all arm of `lower_business`'s
//! item loop (`rust/fsl-core/src/dialect.rs`), so a `control` declaration ---
//! and every `satisfies` naming one --- was parsed, accepted and dropped.
//! `check` therefore answered `ok` for a policy pointing at a control the
//! document never declared, the same defect class #1109 closed for `with` /
//! `when` / `set`. `docs/DESIGN-dialects.md` §3.2 already promised both
//! checks, and `src/fslc/dialects.py` already implemented them, so this is a
//! port gap rather than a narrowing of the language.
//!
//! The contract: an unknown control reference is a positioned error, a
//! declared control nothing satisfies is an `unused_control` warning located
//! at the control, and a declared-and-satisfied catalog stays accepted and
//! warning-free.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/issue_1134_business_controls";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn check(fixture: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("check")
        .arg(format!("{FIXTURE_DIR}/{fixture}"))
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; fixture={fixture}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn warnings_of_kind<'a>(envelope: &'a Value, kind: &str) -> Vec<&'a Value> {
    envelope["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| warning["kind"] == kind)
                .collect()
        })
        .unwrap_or_default()
}

/// A `satisfies` naming an undeclared control is rejected, from a policy and
/// from a goal alike.
///
/// Each fixture differs from `control.fsl` only by the missing `control`
/// declaration, so a failure here cannot be explained by anything else in the
/// spec. The asserted line and column are the policy's or goal's own, which is
/// what the author has to edit; asserting them is what separates "a positioned
/// error" from "an error".
#[test]
fn unknown_control_references_are_positioned_errors() {
    for (fixture, element, id) in [
        ("unknown_control.fsl", "policy", "REQ-CLOSE"),
        ("unknown_control_goal.fsl", "goal", "GOAL-CLOSE"),
    ] {
        let (envelope, code) = check(fixture);
        assert_eq!(code, 2, "{fixture}: exit status; envelope={envelope}");
        assert_eq!(envelope["result"], "error", "{fixture}: {envelope}");
        assert_eq!(envelope["kind"], "semantics", "{fixture}: {envelope}");
        let message = envelope["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{fixture}: no message; envelope={envelope}"));
        assert_eq!(
            message,
            format!("{element} '{id}' satisfies unknown control 'CTRL-ABSENT' at 12:3"),
            "{fixture}: message must name the element, its id, the missing control and the \
             position; got {message}"
        );
    }
}

/// A declared control nothing satisfies is an `unused_control` warning, the
/// one `docs/DESIGN-dialects.md` §3.2 promises and `src/fslc/dialects.py`
/// already emits. The spec still checks, so the warning is the whole signal:
/// asserting its `name` and `loc` is what keeps it actionable.
#[test]
fn an_unsatisfied_control_is_an_unused_control_warning() {
    let (envelope, code) = check("unused_control.fsl");
    assert_eq!(code, 0, "unused_control check: envelope={envelope}");
    assert_eq!(envelope["result"], "ok", "unused_control: {envelope}");
    let warnings = warnings_of_kind(&envelope, "unused_control");
    assert_eq!(warnings.len(), 1, "unused_control: {envelope}");
    let warning = warnings[0];
    assert_eq!(warning["element"], "control", "{warning}");
    assert_eq!(warning["name"], "CTRL-CLOSURE", "{warning}");
    assert_eq!(warning["loc"]["line"], 12, "{warning}");
    assert_eq!(warning["loc"]["column"], 3, "{warning}");
    assert_eq!(
        warning["hint"], "no policy or goal declares `satisfies` for this control",
        "{warning}"
    );
}

/// The control: the same catalog, declared and satisfied, stays accepted and
/// raises no `unused_control`, so both diagnostics above are attributable to
/// the defect in their fixture and not to `control` having become unusable.
#[test]
fn a_satisfied_control_is_accepted_without_a_warning() {
    let (envelope, code) = check("control.fsl");
    assert_eq!(code, 0, "control check: envelope={envelope}");
    assert_eq!(envelope["result"], "ok", "control check: {envelope}");
    assert!(
        warnings_of_kind(&envelope, "unused_control").is_empty(),
        "control check: {envelope}"
    );
}

/// The existing business spec that uses the catalog keeps checking clean, so
/// the new gate did not become an over-detection on the corpus.
#[test]
fn the_repository_business_catalog_still_checks_clean() {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "check",
            "rust/fslc/tests/fixtures/manifest_vocabulary/business.fsl",
        ])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("check envelope");
    assert_eq!(output.status.code(), Some(0), "envelope={envelope}");
    assert_eq!(envelope["result"], "ok", "{envelope}");
    assert!(
        warnings_of_kind(&envelope, "unused_control").is_empty(),
        "{envelope}"
    );
}
