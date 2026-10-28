// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Drift control for #858: every public type that carries a verification
//! outcome must be `#[must_use]`.
//!
//! `Result` is already `#[must_use]`, so `monitor.step(x).map_err(..)?;`
//! satisfies the compiler while the inner `StepResult` -- which owns
//! `violation` -- is dropped as a statement value. Annotating the *function*
//! cannot close that: the attribute is redundant against `Result`. Only the
//! attribute on the TYPE makes the discard a `-D warnings` build failure, and
//! #843 is the defect that gap actually shipped.
//!
//! This test is the drift detector, not the mechanism: the mechanism is the
//! attribute itself plus CI's
//! `cargo clippy --workspace --all-targets -- -D warnings`. What this test
//! catches is a NEW result-carrying public type added without the attribute,
//! which would silently reopen the gap with no failing build anywhere --
//! `unused_must_use` cannot fire for an attribute nobody wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[path = "support/outcome_census_rule.rs"]
mod outcome_census_rule;

/// The 31 public type names this change annotates: #858's enumeration minus
/// the exclusions below, plus `BoundedLivenessViolation` and
/// `LeadsToViolation`, which the extended `*Violation` rule found and #858's
/// curated table missed, `RefinementVerdict` (#1245), `Verdict` (#901),
/// `TraceGate` (#1285), and `ProcessFormOutcome` (the migration's process-form
/// rewrite or refusal). Each must carry `#[must_use]` on the TYPE, because on
/// the function it is redundant against `Result` and closes nothing.
const ANNOTATED: [&str; 31] = [
    // fsl-runtime
    "Violation",
    "StepResult",
    "ReachableWitness",
    "BfsResult",
    "RefinementCheck",
    "RefinementVerdict",
    "BoundaryProbe",
    "BoundedLivenessViolation",
    "ExplicitViolation",
    "ExplicitReachableWitness",
    "ExplicitResult",
    // fsl-verifier
    "BmcViolation",
    "LeadsToViolation",
    "ReachableBlocker",
    "ReachableDiagnosis",
    "BmcResult",
    "InductionResult",
    "RankedLeadstoResult",
    "VacuityFinding",
    "ProgressCheck",
    "ImplicationResult",
    // fsl-solver
    "SatResult",
    // fsl-tools
    "DocumentCheckReport",
    "CausalError",
    "CausalWarning",
    "Applicability",
    "SupportOverlay",
    // fslc
    "OutcomeClass",
    "Verdict",
    "TraceGate",
    "ProcessFormOutcome",
];

/// `ANNOTATED` holds 31 distinct names but 32 declarations: `ReachableWitness`
/// is declared once in `fsl-runtime` and once in `fsl-verifier`, and both must
/// carry the attribute.
const ANNOTATED_DECLARATIONS: usize = 32;

/// Types deliberately left un-annotated, each with the reason. A type moved
/// off this list must gain the attribute; a type added to it must state why.
const EXCLUDED: [(&str, &str); 7] = [
    (
        "Monitor",
        "a stateful interpreter, not an outcome: callers hold it across steps",
    ),
    (
        "BoundedLivenessMonitor",
        "a stateful tracker, not an outcome",
    ),
    (
        "EnabledAction",
        "an action descriptor produced by enumeration, not a verdict",
    ),
    (
        "VariableRole",
        "a classification of a causal variable, not an outcome",
    ),
    ("DomainAggregate", "a surface AST node, not an outcome"),
    ("DomainEffect", "a surface AST node, not an outcome"),
    ("DomainRetry", "a surface AST node, not an outcome"),
];

/// Read scanned Rust source with canonical line endings so adjacent-line
/// assertions retain their meaning even when a checkout uses CRLF.
fn read_source(path: &Path) -> String {
    std::fs::read_to_string(path)
        .expect("read source")
        .replace("\r\n", "\n")
}

/// What the rule below actually matches today. #858 reports 33 types from a
/// hand-curated table; this rule -- #858's, extended with the `*Violation`
/// suffix -- selects 19, all of which are in `ANNOTATED`. Both numbers are pinned so
/// neither the curated decision nor the detector can drift unnoticed.
const DISCOVERED: [&str; 19] = [
    "BfsResult",
    "BmcResult",
    "BmcViolation",
    "BoundedLivenessViolation",
    "DocumentCheckReport",
    "ExplicitResult",
    "ExplicitViolation",
    "ImplicationResult",
    "InductionResult",
    "LeadsToViolation",
    "ProcessFormOutcome",
    "ProgressCheck",
    "RankedLeadstoResult",
    "RefinementVerdict",
    "SatResult",
    "StepResult",
    "VacuityFinding",
    "Verdict",
    "Violation",
];

fn crate_sources() -> Vec<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust workspace directory");
    let mut files = Vec::new();
    for crate_name in [
        "fsl-runtime",
        "fsl-verifier",
        "fsl-solver",
        "fsl-tools",
        "fsl-syntax",
        "fslc",
    ] {
        let source = workspace.join(crate_name).join("src");
        let mut stack = vec![source];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory)
                .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
            {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// `(type name, annotated)` for every public struct/enum in the product body
/// of the audited crates. Direct test-only items are excluded: test helper
/// types are not a public outcome surface. The shared rule handles blanked
/// comments in the annotation look-back.
fn census() -> BTreeMap<String, bool> {
    let mut found = BTreeMap::new();
    for file in crate_sources() {
        let text = read_source(&file);
        found.extend(outcome_census_rule::outcome_types(&text));
    }
    found
}

#[test]
fn every_discovered_outcome_type_is_must_use() {
    let census = census();
    let discovered: Vec<&str> = census.keys().map(String::as_str).collect();
    assert_eq!(
        discovered, DISCOVERED,
        "the discovery rule's match set changed. A NEW result-carrying public \
         type is the case this test exists for: annotate it and add it to \
         DISCOVERED and ANNOTATED. A type that disappeared means the rule has \
         stopped matching the source."
    );
    let unannotated: Vec<&String> = census
        .iter()
        .filter(|(_, annotated)| !**annotated)
        .map(|(name, _)| name)
        .collect();
    assert!(
        unannotated.is_empty(),
        "these public types carry a verification outcome with no #[must_use], \
         so discarding one is not a build failure: {unannotated:?}"
    );
}

/// The curated decision from #858: all 26 annotated, all 7 exclusions still
/// un-annotated and still carrying their stated reason.
#[test]
fn the_reviewed_annotation_decision_is_intact() {
    let sources: Vec<String> = crate_sources()
        .into_iter()
        .map(|path| read_source(&path))
        .collect();

    let mut declarations = 0;
    for name in ANNOTATED {
        let mut annotated_somewhere = false;
        let mut declared_somewhere = false;
        for text in &sources {
            // `<` also matches a generic declaration (`RefinementVerdict<'a>`).
            for (keyword, after) in [("struct", ' '), ("enum", ' '), ("enum", '<')] {
                let needle = format!("\npub {keyword} {name}{after}");
                let mut cursor = 0;
                while let Some(offset) = text[cursor..].find(&needle) {
                    let at = cursor + offset;
                    declared_somewhere = true;
                    declarations += 1;
                    let preceding = &text[..at];
                    let attribute = preceding.rsplit('\n').next().unwrap_or_default();
                    if attribute.trim_start().starts_with("#[must_use") {
                        annotated_somewhere = true;
                    } else {
                        panic!("'{name}' is declared without #[must_use] directly above it");
                    }
                    cursor = at + needle.len();
                }
            }
        }
        assert!(
            declared_somewhere,
            "'{name}' is no longer declared anywhere"
        );
        assert!(annotated_somewhere, "'{name}' lost its #[must_use]");
    }
    assert_eq!(
        declarations, ANNOTATED_DECLARATIONS,
        "the number of annotated declarations changed"
    );

    for (name, reason) in EXCLUDED {
        assert!(
            !reason.is_empty(),
            "'{name}' must record why it is not an outcome"
        );
        for text in &sources {
            for keyword in ["struct", "enum"] {
                let needle = format!("\npub {keyword} {name} ");
                if let Some(at) = text.find(&needle) {
                    let attribute = text[..at].rsplit('\n').next().unwrap_or_default();
                    assert!(
                        !attribute.trim_start().starts_with("#[must_use"),
                        "'{name}' is annotated but is still on EXCLUDED ({reason})"
                    );
                }
            }
        }
    }
}

/// The #843 type specifically, named so a reader of that issue finds it.
#[test]
fn step_result_is_must_use_with_a_reason() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust workspace directory")
        .join("fsl-runtime/src/lib.rs");
    let text = read_source(&source);
    let declaration = text
        .find("\npub struct StepResult {")
        .expect("StepResult declaration");
    let preceding = &text[..declaration];
    assert!(
        preceding.ends_with(']'),
        "an attribute must sit directly above StepResult"
    );
    let attribute_line = preceding.rsplit('\n').next().expect("attribute line");
    assert!(
        attribute_line.starts_with("#[must_use = "),
        "StepResult must carry a reasoned #[must_use]; found: {attribute_line}"
    );
    assert!(
        attribute_line.contains("violation"),
        "the reason must name what is lost: {attribute_line}"
    );
}

#[test]
fn source_reader_normalizes_crlf_for_adjacent_line_controls() {
    let path = std::env::temp_dir().join(format!("fslc-issue-858-crlf-{}.rs", std::process::id()));
    std::fs::write(
        &path,
        "#[must_use = \"a StepResult carries a violation\"]\r\npub struct StepResult {}\r\n",
    )
    .expect("write CRLF source fixture");
    assert_eq!(
        read_source(&path),
        "#[must_use = \"a StepResult carries a violation\"]\npub struct StepResult {}\n"
    );
    std::fs::remove_file(path).expect("remove CRLF source fixture");
}
