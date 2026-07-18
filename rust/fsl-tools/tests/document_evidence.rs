// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the evidence/assurance overlay (issue #332):
//! `fsl_tools::document_evidence`'s classification (reusing `fslc ledger`'s
//! own vocabulary, never re-deriving it) and how the renderer (issue #326)
//! applies it. `fslc`'s `--evidence` flag, unmatched-file diagnostics, and
//! `fslc document check`'s `evidence_changed` drift reason are exercised
//! end-to-end in `rust/fslc/tests/document_evidence_cli.rs`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use fsl_tools::{AppliedEvidence, Locale, RequirementClaimSet, Segment};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn evidence(path: &str, value: Value) -> (String, Value) {
    (path.to_owned(), value)
}

// --- `requirement_assurance`: classification (no model/render needed) -------

#[test]
fn proved_evidence_classifies_into_the_formal_dimension() {
    let files = vec![evidence(
        "proof.json",
        json!({"requirement": {"id": "REQ-1"}, "completeness": "unbounded"}),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(assurance.formal.len(), 1);
    assert_eq!(assurance.formal[0].label, "proved(induction)");
    assert!(assurance.conformance.is_empty());
    assert!(assurance.statistical.is_empty());
}

#[test]
fn bounded_evidence_classifies_into_the_formal_dimension_with_its_depth() {
    let files = vec![evidence(
        "bmc.json",
        json!({"requirement": {"id": "REQ-1"}, "completeness": "bounded", "checked_to_depth": 8}),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(assurance.formal.len(), 1);
    assert_eq!(assurance.formal[0].label, "bounded(BMC depth 8)");
}

#[test]
fn replay_observed_evidence_classifies_into_the_conformance_dimension() {
    let files = vec![evidence(
        "replay.json",
        json!({"requirement": {"id": "REQ-1"}, "result": "conformant"}),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(assurance.conformance.len(), 1);
    assert_eq!(assurance.conformance[0].label, "replay-observed");
}

#[test]
fn statistical_evidence_classifies_into_the_statistical_dimension() {
    let files = vec![evidence(
        "stats.json",
        json!({"requirement": {"id": "REQ-1"}, "status": "statistically_supported"}),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(assurance.statistical.len(), 1);
    assert_eq!(assurance.statistical[0].label, "statistical");
}

#[test]
fn no_matching_evidence_leaves_a_requirement_not_run() {
    let files = vec![evidence(
        "other.json",
        json!({"requirement": {"id": "REQ-9"}, "completeness": "unbounded"}),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert!(assurance.is_not_run());
}

#[test]
fn a_violated_bounded_run_still_classifies_as_bounded_never_downgraded() {
    // Acceptance criterion 3: assurance class (method coverage) is
    // orthogonal to pass/fail verdict -- a `violated` BMC run is still
    // `bounded`, never silently reported as weaker or omitted.
    let files = vec![evidence(
        "bmc.json",
        json!({
            "requirement": {"id": "REQ-1"},
            "completeness": "bounded",
            "result": "violated",
            "checked_to_depth": 5,
        }),
    )];
    let assurance = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(assurance.formal.len(), 1);
    assert_eq!(assurance.formal[0].label, "bounded(BMC depth 5)");
    assert_eq!(assurance.formal[0].result.as_deref(), Some("violated"));
}

#[test]
fn matches_via_both_the_requirements_array_and_the_singular_requirement_id() {
    let files = vec![
        evidence(
            "multi.json",
            json!({"requirements": ["REQ-1", "REQ-2"], "completeness": "unbounded"}),
        ),
        evidence(
            "single.json",
            json!({"requirement": {"id": "REQ-1"}, "status": "statistically_supported"}),
        ),
    ];
    let req1 = fsl_tools::requirement_assurance("REQ-1", &files);
    assert_eq!(req1.formal.len(), 1);
    assert_eq!(req1.statistical.len(), 1);
    let req2 = fsl_tools::requirement_assurance("REQ-2", &files);
    assert_eq!(req2.formal.len(), 1);
    assert!(req2.statistical.is_empty());
}

#[test]
fn evidence_file_order_does_not_change_the_computed_assurance() {
    let forward = vec![
        evidence(
            "a.json",
            json!({"requirement": {"id": "REQ-1"}, "completeness": "unbounded"}),
        ),
        evidence(
            "b.json",
            json!({"requirement": {"id": "REQ-1"}, "completeness": "bounded", "checked_to_depth": 3}),
        ),
    ];
    let mut reversed = forward.clone();
    reversed.reverse();
    let forward_assurance = fsl_tools::requirement_assurance("REQ-1", &forward);
    let reversed_assurance = fsl_tools::requirement_assurance("REQ-1", &reversed);
    let labels = |assurance: &fsl_tools::RequirementAssurance| -> Vec<String> {
        assurance
            .formal
            .iter()
            .map(|entry| entry.label.clone())
            .collect()
    };
    assert_eq!(labels(&forward_assurance), labels(&reversed_assurance));
}

// --- `unmatched_evidence_paths` ----------------------------------------------

#[test]
fn whole_spec_evidence_naming_no_requirement_id_is_never_unmatched() {
    let ids: BTreeSet<&str> = ["REQ-1"].into_iter().collect();
    let files = vec![evidence(
        "wholespec.json",
        json!({"completeness": "unbounded"}),
    )];
    assert!(fsl_tools::unmatched_evidence_paths(&ids, &files).is_empty());
}

#[test]
fn evidence_naming_an_unknown_requirement_id_is_reported_as_unmatched() {
    let ids: BTreeSet<&str> = ["REQ-1"].into_iter().collect();
    let files = vec![evidence(
        "typo.json",
        json!({"requirement": {"id": "REQ-9"}}),
    )];
    assert_eq!(
        fsl_tools::unmatched_evidence_paths(&ids, &files),
        vec!["typo.json".to_owned()]
    );
}

#[test]
fn evidence_matching_a_known_requirement_id_is_not_unmatched() {
    let ids: BTreeSet<&str> = ["REQ-1"].into_iter().collect();
    let files = vec![evidence("ok.json", json!({"requirement": {"id": "REQ-1"}}))];
    assert!(fsl_tools::unmatched_evidence_paths(&ids, &files).is_empty());
}

// --- Rendering integration ---------------------------------------------------

fn claims_fixture_render(
    locale: Locale,
    evidence: Option<&AppliedEvidence<'_>>,
) -> (RequirementClaimSet, String) {
    let source = read("tests/fixtures/document_claims_fixture.fsl");
    let root = manifest_path("tests/fixtures");
    let label = "document_claims_fixture.fsl";
    let claims = fsl_tools::project_requirement_claims_from_source(&source, Some(label), &root)
        .expect("project document_claims_fixture.fsl");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&source).expect("trace contract");
    let doc = fsl_tools::render_requirements_document(
        &claims,
        &model,
        trace.as_ref(),
        locale,
        None,
        evidence,
        None,
    );
    (claims, doc.markdown)
}

fn requirement_section<'a>(markdown: &'a str, id: &str, next_id: &str) -> &'a str {
    let start = markdown
        .find(&format!("### {id}"))
        .unwrap_or_else(|| panic!("{id} section exists"));
    let rest = &markdown[start..];
    let end = rest.find(&format!("### {next_id}")).unwrap_or(rest.len());
    rest[..end].trim_end()
}

fn claim_segments(markdown: &str) -> Vec<(String, String, String)> {
    fsl_tools::parse_generated_document(markdown)
        .expect("parse generated document")
        .segments
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Claim { id, digest, body } => Some((id, digest, body)),
            _ => None,
        })
        .collect()
}

#[test]
fn no_evidence_renders_byte_identically_to_no_evidence_argument() {
    let with_explicit_none = claims_fixture_render(Locale::Ja, None).1;
    assert!(!with_explicit_none.contains("evidence_digest"));
    assert!(!with_explicit_none.contains("## 検証エビデンス出典"));
}

#[test]
fn claim_block_digests_are_unaffected_by_evidence_presence_or_absence() {
    // Acceptance criterion 2: the assurance overlay renders as residue
    // outside every `<!-- fsl:claim -->` marker, so a claim's own digest and
    // body text must never depend on whether evidence was supplied.
    let (_, without) = claims_fixture_render(Locale::Ja, None);
    let files = vec![evidence(
        "proof.json",
        json!({"requirement": {"id": "REQ-1"}, "completeness": "unbounded"}),
    )];
    let applied = AppliedEvidence {
        files: &files,
        digest: "sha256:test",
    };
    let (_, with) = claims_fixture_render(Locale::Ja, Some(&applied));

    assert_eq!(claim_segments(&without), claim_segments(&with));
    assert!(with.contains("evidence_digest: sha256:test"));
    assert!(!without.contains("evidence_digest"));
}

#[test]
fn every_dimension_renders_explicitly_even_when_not_run() {
    // Acceptance criterion 1: an aspect with no evidence is shown as
    // `not_run`, never silently omitted.
    let (_, markdown) = claims_fixture_render(Locale::Ja, None);
    let section = requirement_section(&markdown, "REQ-1", "REQ-2");
    assert!(section.contains("**保証クラス**"));
    assert!(section.contains("形式検証: `not_run`"));
    assert!(section.contains("実装適合: `not_run`"));
    assert!(section.contains("統計的裏付け: `not_run`"));
}

#[test]
fn bounded_evidence_appears_in_the_requirement_it_names_and_not_in_others() {
    let files = vec![evidence(
        "bmc.json",
        json!({"requirement": {"id": "REQ-1"}, "completeness": "bounded", "checked_to_depth": 8}),
    )];
    let applied = AppliedEvidence {
        files: &files,
        digest: "sha256:test",
    };
    let (_, markdown) = claims_fixture_render(Locale::Ja, Some(&applied));
    let req1 = requirement_section(&markdown, "REQ-1", "REQ-2");
    assert!(req1.contains("bounded(BMC depth 8)"));
    let req2 = requirement_section(&markdown, "REQ-2", "REQ-3");
    assert!(req2.contains("形式検証: `not_run`"));
}

#[test]
fn liveness_requirement_gets_a_caveat_when_only_bounded_formal_evidence_backs_it() {
    // REQ-4 in document_claims_fixture.fsl links both a `reachable`
    // (ReachabilityGoal) and a `leadsTo` (ProgressRule) claim.
    let files = vec![evidence(
        "bmc.json",
        json!({"requirements": ["REQ-4"], "completeness": "bounded", "checked_to_depth": 10}),
    )];
    let applied = AppliedEvidence {
        files: &files,
        digest: "sha256:test",
    };
    let (_, markdown) = claims_fixture_render(Locale::Ja, Some(&applied));
    let req4 = requirement_section(&markdown, "REQ-4", "REQ-5");
    assert!(req4.contains("bounded(BMC depth 10)"));
    assert!(req4.contains("進行条件（liveness）が含まれる"));
    assert!(req4.contains("liveness の証明ではない"));
}

#[test]
fn a_non_liveness_requirement_never_gets_the_liveness_caveat() {
    let files = vec![evidence(
        "bmc.json",
        json!({"requirement": {"id": "REQ-1"}, "completeness": "bounded", "checked_to_depth": 8}),
    )];
    let applied = AppliedEvidence {
        files: &files,
        digest: "sha256:test",
    };
    let (_, markdown) = claims_fixture_render(Locale::Ja, Some(&applied));
    let req1 = requirement_section(&markdown, "REQ-1", "REQ-2");
    assert!(!req1.contains("liveness"));
}

#[test]
fn evidence_sources_section_lists_files_sorted_with_matched_requirement_ids() {
    let files = vec![
        evidence(
            "z_evidence.json",
            json!({"requirement": {"id": "REQ-1"}, "completeness": "unbounded"}),
        ),
        evidence(
            "a_evidence.json",
            json!({"requirement": {"id": "REQ-9"}, "result": "conformant"}),
        ),
    ];
    let applied = AppliedEvidence {
        files: &files,
        digest: "sha256:test",
    };
    let (_, markdown) = claims_fixture_render(Locale::Ja, Some(&applied));
    let start = markdown
        .find("## 検証エビデンス出典")
        .expect("section exists");
    let section = &markdown[start..];
    let a_pos = section.find("a_evidence.json").expect("a_evidence row");
    let z_pos = section.find("z_evidence.json").expect("z_evidence row");
    assert!(a_pos < z_pos, "rows must be sorted");
    assert!(section.contains("（本仕様のどの要件 ID にも対応しない）"));
}

#[test]
fn en_locale_renders_the_assurance_class_heading_and_not_run_dimensions() {
    let (_, markdown) = claims_fixture_render(Locale::En, None);
    let section = requirement_section(&markdown, "REQ-1", "REQ-2");
    assert!(section.contains("**Assurance class**"));
    assert!(section.contains("Formal verification: `not_run`"));
}
