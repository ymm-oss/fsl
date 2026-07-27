// SPDX-License-Identifier: Apache-2.0

//! `--strict-tags`' `untagged` hint must name the canonical requirement link.
//!
//! The hint fires at the exact moment an author is choosing a tag, so it is
//! the most load-bearing sentence FSL emits about ID form. It used to propose
//! `add a declaration tag such as "REQ-1: original requirement"; use
//! "MODEL: ..." or "ASSUME-1: ..."` — the `"ID: text"` string slot that
//! `docs/DESIGN-id-policy.md` classifies as non-canonical migration input and
//! that `fslc lint` rejects as `legacy_string_metadata`. Following the
//! diagnostic produced a spec the next gate refuses.
//!
//! Nothing tested the hint's text, so the two gates could disagree
//! indefinitely. These tests pin the contract from both sides: the hint names
//! the typed annotation, that annotation clears `--strict-tags` *and* `lint`,
//! and the legacy form it used to propose is the negative control — accepted
//! by `--strict-tags`, rejected by `lint`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The retired proposal. Named here so a revert cannot pass silently.
const LEGACY_PROPOSAL: &str = "REQ-1: original requirement";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/untagged_hint_{name}.fsl")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {args:?}`: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

fn untagged_warnings(envelope: &Value) -> Vec<Value> {
    envelope["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter(|warning| warning["kind"] == "untagged")
        .cloned()
        .collect()
}

#[test]
fn untagged_hint_proposes_the_typed_annotation_and_not_the_string_slot() {
    let (envelope, status) = run(&["check", &fixture("untagged"), "--strict-tags"]);
    assert_eq!(envelope["result"], "ok");
    assert_eq!(status, 0);

    let warnings = untagged_warnings(&envelope);
    assert_eq!(warnings.len(), 2, "one per declaration: {warnings:?}");
    for warning in &warnings {
        let hint = warning["hint"].as_str().expect("hint string");
        assert!(
            hint.contains(r#"@requirement("REQ-SCOPE-001""#),
            "hint must name the canonical annotation: {hint}"
        );
        assert!(
            hint.contains("MODEL-/ASSUME-prefixed id"),
            "hint must keep the modeling-intent roles in canonical form: {hint}"
        );
        assert!(
            !hint.contains(LEGACY_PROPOSAL),
            "hint must not propose the non-canonical string slot: {hint}"
        );
    }
}

#[test]
fn the_hinted_repair_clears_both_tagging_gates() {
    let (envelope, status) = run(&["check", &fixture("canonical"), "--strict-tags"]);
    assert_eq!(status, 0);
    assert!(
        untagged_warnings(&envelope).is_empty(),
        "typed annotations must count as tagged: {envelope:?}"
    );

    let (lint, lint_status) = run(&["lint", &fixture("canonical")]);
    assert_eq!(lint["finding_count"], 0, "{lint:?}");
    assert_eq!(lint_status, 0);
}

#[test]
fn the_previously_hinted_form_is_accepted_by_strict_tags_and_rejected_by_lint() {
    // The asymmetry that let the old hint survive: `--strict-tags` only asks
    // whether a tag exists, so it cannot be the gate that catches ID form.
    let (envelope, status) = run(&["check", &fixture("legacy"), "--strict-tags"]);
    assert_eq!(status, 0);
    assert!(
        untagged_warnings(&envelope).is_empty(),
        "strict-tags counts the legacy string as tagged: {envelope:?}"
    );

    let (lint, lint_status) = run(&["lint", &fixture("legacy")]);
    assert_eq!(lint_status, 1);
    let findings = lint["files"][0]["findings"]
        .as_array()
        .expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "legacy_string_metadata"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding["canonical_replacement"]
                .as_str()
                .is_some_and(|replacement| replacement.starts_with("@requirement("))),
        "lint's replacement is the form the hint now names: {findings:?}"
    );
}
