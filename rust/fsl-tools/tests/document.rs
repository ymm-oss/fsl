// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the Requirement Claim IR (RCIR) v1 projector,
//! issue #325: `schemas/fslc/document/requirement-claims.v1.schema.json`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fsl_tools::{Completeness, ProvenanceAssurance, RequirementClaimSet};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn project(source: &str, source_path: &str, resolver_root: &Path) -> RequirementClaimSet {
    fsl_tools::project_requirement_claims_from_source(source, Some(source_path), resolver_root)
        .unwrap_or_else(|error| panic!("project {source_path}: {error}"))
}

fn cancel_system() -> (String, PathBuf) {
    (
        read("../../examples/pm/cancel_system.fsl"),
        manifest_path("../../examples/pm"),
    )
}

fn claims_fixture() -> (String, PathBuf) {
    (
        read("tests/fixtures/document_claims_fixture.fsl"),
        manifest_path("tests/fixtures"),
    )
}

fn kpi_fixture() -> (String, PathBuf) {
    (
        read("tests/fixtures/document_kpi_fixture.fsl"),
        manifest_path("tests/fixtures"),
    )
}

fn claim_digest<'a>(set: &'a RequirementClaimSet, id: &str) -> &'a str {
    &set.claims
        .iter()
        .find(|claim| claim.id == id)
        .unwrap_or_else(|| panic!("claim '{id}' not found; have {:?}", claim_ids(set)))
        .claim_digest
}

fn claim_ids(set: &RequirementClaimSet) -> Vec<&str> {
    set.claims.iter().map(|claim| claim.id.as_str()).collect()
}

// --- Projection completeness / coverage classification ---------------------

#[test]
fn classifies_every_authored_target_in_cancel_system() {
    let (source, root) = cancel_system();
    let claims = project(&source, "cancel_system.fsl", &root);

    let expected_universe: BTreeSet<&str> = [
        "init",
        "action:tap_cancel",
        "action:submit_cancel",
        "action:accept",
        "action:decline",
        "property:invariant:CountsMatchScreens",
        "property:invariant:OfferedFlagConsistent",
        "acceptance:AC-1",
        "acceptance:AC-2",
        "refinement:CancelFlow",
    ]
    .into_iter()
    .collect();
    let authored: BTreeSet<&str> = claims
        .coverage
        .authored
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(authored, expected_universe);

    assert_eq!(claims.coverage.counts.authored, 10);
    assert_eq!(claims.coverage.counts.unsupported, 2);
    assert_eq!(claims.coverage.counts.rendered, 8);
    assert_eq!(claims.coverage.counts.unattributed, 0);

    let unsupported: BTreeSet<&str> = claims
        .coverage
        .unsupported
        .iter()
        .map(|entry| entry.target.as_str())
        .collect();
    assert_eq!(
        unsupported,
        BTreeSet::from(["init", "refinement:CancelFlow"])
    );
    for entry in &claims.coverage.unsupported {
        assert!(!entry.reason.is_empty());
    }

    // Fail-closed partition invariant: no target is missing or double-counted.
    let rendered: BTreeSet<&str> = claims
        .coverage
        .rendered
        .iter()
        .map(String::as_str)
        .collect();
    let reconstructed: BTreeSet<&str> = rendered.union(&unsupported).copied().collect();
    assert_eq!(reconstructed, expected_universe);
}

#[test]
fn covers_all_nine_claim_kinds_across_fixtures() {
    let (cancel_source, cancel_root) = cancel_system();
    let (claims_source, claims_root) = claims_fixture();
    let cancel = project(&cancel_source, "cancel_system.fsl", &cancel_root);
    let fixture = project(&claims_source, "document_claims_fixture.fsl", &claims_root);

    let kinds: BTreeSet<&str> = cancel
        .claims
        .iter()
        .chain(&fixture.claims)
        .map(|claim| claim.kind.as_str())
        .collect();
    let expected: BTreeSet<&str> = [
        "operation",
        "state_rule",
        "transition_rule",
        "progress_rule",
        "reachability_goal",
        "acceptance_trace",
        "forbidden_trace",
        "deadline_rule",
        "terminal_rule",
    ]
    .into_iter()
    .collect();
    assert_eq!(kinds, expected);

    for claim in cancel.claims.iter().chain(&fixture.claims) {
        let suffix = format!("#{}", claim.kind.as_str());
        assert!(
            claim.id.ends_with(&suffix),
            "claim id '{}' does not end with '{suffix}'",
            claim.id
        );
    }
}

#[test]
fn emits_unattributed_claims_without_dropping_them() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);

    let target = "property:invariant:RetentionPlaceholder";
    assert!(claims.coverage.unattributed.contains(&target.to_owned()));
    let claim = claims
        .claims
        .iter()
        .find(|claim| claim.semantic_targets.iter().any(|t| t == target))
        .expect("RetentionPlaceholder claim is emitted");
    assert!(claim.requirements.is_empty());
}

#[test]
fn deadline_invariant_projects_as_deadline_rule_only_in_requirements_dialect() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);
    let deadline_claim = claims
        .claims
        .iter()
        .find(|claim| claim.id.starts_with("property:invariant:_deadline_"))
        .expect("a deadline-generated invariant claim exists");
    assert_eq!(deadline_claim.kind.as_str(), "deadline_rule");
    assert!(!deadline_claim.requirements.is_empty());

    // Negative control: a direct-spec invariant that merely happens to start
    // with `_deadline_` is not reclassified; the heuristic is dialect-scoped.
    let spec_source = r"
spec DeadlineNamingIsNotSpecial {
  state { ready: Bool }
  init { ready = false }
  invariant _deadline_lookalike { ready or not ready }
}
";
    let spec_root = manifest_path("tests/fixtures");
    let spec_claims = project(spec_source, "inline.fsl", &spec_root);
    let claim = spec_claims
        .claims
        .iter()
        .find(|claim| {
            claim
                .semantic_targets
                .iter()
                .any(|t| t == "property:invariant:_deadline_lookalike")
        })
        .expect("claim exists");
    assert_eq!(claim.kind.as_str(), "state_rule");
}

#[test]
fn rejects_unsupported_dialect_fail_closed() {
    let source = r"
business NotSupportedYet {
  entity Case
  process Case {
    stages Open, Closed
    initial Open
    transition close Open -> Closed by Staff
  }
}
verify {
  instances Case = 2
}
";
    let root = manifest_path("tests/fixtures");
    let error = fsl_tools::project_requirement_claims_from_source(source, None, &root)
        .expect_err("business dialect is rejected");
    assert!(
        error.contains("business"),
        "unexpected error message: {error}"
    );
}

#[test]
fn kpi_projection_is_an_unsupported_target() {
    let (source, root) = kpi_fixture();
    let claims = project(&source, "document_kpi_fixture.fsl", &root);
    let entry = claims
        .coverage
        .unsupported
        .iter()
        .find(|entry| entry.target == "projection:paid_claims")
        .expect("kpi projection is reported as unsupported");
    assert!(!entry.reason.is_empty());
}

// --- Requirement relations (many-to-many) -----------------------------------

#[test]
fn keeps_multiple_statements_for_one_requirement_id() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);
    let req1 = claims
        .requirements
        .iter()
        .find(|requirement| requirement.id == "REQ-1")
        .expect("REQ-1 exists");
    let texts: BTreeSet<Option<String>> = req1
        .statements
        .iter()
        .map(|statement| statement.text.clone())
        .collect();
    assert_eq!(texts.len(), 2, "REQ-1 keeps both distinct statements");
    assert_eq!(req1.claim_ids.len(), 2);
}

#[test]
fn keeps_identical_requirement_text_from_distinct_source_declarations() {
    let (source, root) = claims_fixture();
    let source = source.replace(
        "An escalated case is closed once handled",
        "An open case can be escalated by staff",
    );
    let claims = project(&source, "document_claims_fixture.fsl", &root);
    let req1 = claims
        .requirements
        .iter()
        .find(|requirement| requirement.id == "REQ-1")
        .expect("REQ-1 exists");
    assert_eq!(req1.statements.len(), 2);
    let source_lines: BTreeSet<u32> = req1
        .statements
        .iter()
        .filter_map(|statement| statement.source.as_ref().map(|source| source.line))
        .collect();
    assert_eq!(source_lines.len(), 2);
}

#[test]
fn keeps_full_many_to_many_requirement_relation() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);
    let close_claim = claims
        .claims
        .iter()
        .find(|claim| claim.id == "action:close#operation")
        .expect("close claim exists");
    let mut requirements = close_claim.requirements.clone();
    requirements.sort();
    assert_eq!(requirements, vec!["REQ-1".to_owned(), "REQ-2".to_owned()]);

    for id in ["REQ-1", "REQ-2"] {
        let requirement = claims
            .requirements
            .iter()
            .find(|requirement| requirement.id == id)
            .unwrap_or_else(|| panic!("{id} exists"));
        assert!(
            requirement
                .claim_ids
                .contains(&"action:close#operation".to_owned())
        );
    }

    // No singular compatibility projection anywhere in the artifact.
    let value = serde_json::to_value(&claims).expect("serialize");
    assert!(value.get("requirement").is_none());
    assert!(value["claims"][0].get("requirement").is_none());
}

#[test]
fn acceptance_case_own_id_is_a_requirement_relation() {
    // `skills/fsl/reference.md` sec. 10: native lowering folds requirement
    // blocks, process `covers`, and acceptance/forbidden IDs into the same
    // typed annotation carrier, so an acceptance/forbidden case's own ID is a
    // requirement relation even without an explicit `@requirement(...)`.
    let (source, root) = cancel_system();
    let claims = project(&source, "cancel_system.fsl", &root);
    let ac1 = claims
        .claims
        .iter()
        .find(|claim| claim.id == "acceptance:AC-1#acceptance_trace")
        .expect("AC-1 claim exists");
    assert_eq!(ac1.requirements, vec!["AC-1".to_owned()]);
    assert!(
        claims
            .coverage
            .rendered
            .contains(&"acceptance:AC-1".to_owned())
    );
    let requirement = claims
        .requirements
        .iter()
        .find(|requirement| requirement.id == "AC-1")
        .expect("AC-1 has its own requirement record");
    assert!(
        requirement
            .claim_ids
            .contains(&"acceptance:AC-1#acceptance_trace".to_owned())
    );
}

// --- Mutation sensitivity ----------------------------------------------------

fn mutated_cancel_system(from: &str, to: &str) -> RequirementClaimSet {
    let (source, root) = cancel_system();
    assert!(source.contains(from), "fixture no longer contains {from:?}");
    let mutated = source.replacen(from, to, 1);
    assert_ne!(source, mutated, "mutation must change the source");
    project(&mutated, "cancel_system.fsl", &root)
}

fn assert_only_target_changed(
    before: &RequirementClaimSet,
    after: &RequirementClaimSet,
    changed_id: &str,
    unrelated_id: &str,
) {
    assert_ne!(
        claim_digest(before, changed_id),
        claim_digest(after, changed_id),
        "expected {changed_id} digest to change"
    );
    assert_eq!(
        claim_digest(before, unrelated_id),
        claim_digest(after, unrelated_id),
        "expected {unrelated_id} digest to stay the same"
    );
    assert_ne!(
        before.spec.claim_set_digest, after.spec.claim_set_digest,
        "claim_set_digest must move with any claim digest"
    );
}

#[test]
fn claim_digest_changes_when_not_is_removed_from_a_guard() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system("requires not scr[c].offered", "requires scr[c].offered");
    assert_only_target_changed(
        &before,
        &after,
        "action:submit_cancel#operation",
        "action:tap_cancel#operation",
    );
}

#[test]
fn claim_digest_changes_when_a_guard_is_removed() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "requires scr[c].st == CancelForm\n      requires not scr[c].offered",
        "requires not scr[c].offered",
    );
    assert_only_target_changed(
        &before,
        &after,
        "action:submit_cancel#operation",
        "action:tap_cancel#operation",
    );
}

#[test]
fn claim_digest_changes_when_an_assignment_is_removed() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system("      retain_count = retain_count + 1\n", "");
    assert_only_target_changed(
        &before,
        &after,
        "action:accept#operation",
        "action:decline#operation",
    );
}

#[test]
fn claim_digest_changes_when_an_assignment_target_changes() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "retain_count = retain_count + 1",
        "churn_count = retain_count + 1",
    );
    assert_only_target_changed(
        &before,
        &after,
        "action:accept#operation",
        "action:decline#operation",
    );
}

#[test]
fn claim_digest_changes_when_a_referenced_enum_member_changes() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system("scr[c].st = ThanksStay", "scr[c].st = GoodbyePage");
    assert_only_target_changed(
        &before,
        &after,
        "action:accept#operation",
        "action:decline#operation",
    );
}

#[test]
fn claim_digest_changes_when_fairness_is_removed() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "fair action accept(c: Sub) maps accept_offer(c)",
        "action accept(c: Sub) maps accept_offer(c)",
    );
    assert_only_target_changed(
        &before,
        &after,
        "action:accept#operation",
        "action:decline#operation",
    );
}

#[test]
fn claim_digest_changes_when_an_invariant_is_weakened() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "(scr[c].st == OfferDialog or scr[c].st == ThanksStay or scr[c].st == GoodbyePage)",
        "(scr[c].st == OfferDialog or scr[c].st == ThanksStay)",
    );
    assert_only_target_changed(
        &before,
        &after,
        "property:invariant:OfferedFlagConsistent#state_rule",
        "property:invariant:CountsMatchScreens#state_rule",
    );
}

#[test]
fn claim_digest_changes_when_an_acceptance_step_changes() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "tap_cancel(0)\n    submit_cancel(0)\n    accept(0)",
        "tap_cancel(0)\n    submit_cancel(0)\n    decline(0)",
    );
    assert_only_target_changed(
        &before,
        &after,
        "acceptance:AC-1#acceptance_trace",
        "acceptance:AC-2#acceptance_trace",
    );
}

#[test]
fn claim_digest_changes_when_a_forbidden_step_changes() {
    let (source, root) = claims_fixture();
    let before = project(&source, "document_claims_fixture.fsl", &root);
    assert!(source.contains("forbidden FB-1"));
    let mutated = source.replacen(
        "forbidden FB-1 \"A case cannot be closed while still open\" {\n    close(0)\n    expect rejected\n  }",
        "forbidden FB-1 \"A case cannot be closed while still open\" {\n    close(1)\n    expect rejected\n  }",
        1,
    );
    assert_ne!(source, mutated);
    let after = project(&mutated, "document_claims_fixture.fsl", &root);
    assert_only_target_changed(
        &before,
        &after,
        "forbidden:FB-1#forbidden_trace",
        "acceptance:AC-1#acceptance_trace",
    );
}

// --- Digest stability --------------------------------------------------------

#[test]
fn digests_are_stable_under_comment_and_formatting_changes() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let mutated = format!("// an added leading comment\n{source}\n\n// trailing comment\n");
    let after = project(&mutated, "cancel_system.fsl", &root);

    assert_eq!(before.spec.spec_digest, after.spec.spec_digest);
    assert_eq!(before.spec.claim_set_digest, after.spec.claim_set_digest);
    for claim in &before.claims {
        assert_eq!(
            claim.claim_digest,
            claim_digest(&after, &claim.id),
            "claim {} digest moved under a comment-only change",
            claim.id
        );
    }
}

#[test]
fn claim_set_digest_changes_when_requirement_text_changes_but_claim_digests_do_not() {
    let (source, root) = cancel_system();
    let before = project(&source, "cancel_system.fsl", &root);
    let after = mutated_cancel_system(
        "\"On cancellation-form submission, show the retention offer exactly once per subscription\"",
        "\"On cancellation-form submission, show the retention offer exactly once\"",
    );
    assert_ne!(before.spec.claim_set_digest, after.spec.claim_set_digest);
    for claim in &before.claims {
        assert_eq!(claim.claim_digest, claim_digest(&after, &claim.id));
    }
}

#[test]
fn projection_is_deterministic() {
    let (source, root) = cancel_system();
    let first = project(&source, "cancel_system.fsl", &root);
    let second = project(&source, "cancel_system.fsl", &root);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap()
    );
}

// --- Provenance ---------------------------------------------------------------

#[test]
fn requirements_dialect_claims_are_source_backed() {
    let (source, root) = cancel_system();
    let claims = project(&source, "cancel_system.fsl", &root);
    assert!(
        claims
            .claims
            .iter()
            .all(|claim| claim.provenance.assurance == ProvenanceAssurance::SourceBacked)
    );
    assert_eq!(claims.provenance.completeness, Completeness::Complete);
}

#[test]
fn unknown_provenance_is_reported_not_guessed() {
    // The internal origin registry is currently sparse outside the domain
    // dialect (docs/DESIGN-kernel-origin-v2.md), so most claims fall back to
    // their checked declaration's own span (a real, non-guessed signal — see
    // `provenance_for`). `terminal` is the one claim kind with no span of its
    // own (`KernelModel::terminal` is a bare `Expr`), so stripping the origin
    // registry is the only way to observe genuinely unknown provenance
    // without fabricating a location for it.
    let (source, root) = claims_fixture();
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let stripped_model =
        fsl_core::build_surface_model(kernel.syntax().clone()).expect("build surface model");

    let input = fsl_tools::DocumentInput {
        kernel: &kernel,
        model: &stripped_model,
        source: &source,
        source_path: Some("document_claims_fixture.fsl"),
        dialect: fsl_tools::DocumentDialect::Requirements,
        implements_names: Vec::new(),
        analysis_scope: fsl_tools::AnalysisScope::default(),
    };
    let claims = fsl_tools::project_requirement_claims(&input).expect("project");
    assert!(!claims.claims.is_empty());

    let terminal = claims
        .claims
        .iter()
        .find(|claim| claim.kind.as_str() == "terminal_rule")
        .expect("a terminal claim exists");
    assert_eq!(terminal.provenance.assurance, ProvenanceAssurance::Unknown);
    assert!(terminal.provenance.sources.is_empty());
    // Not dropped just because provenance is missing.
    assert!(!terminal.semantic_targets.is_empty());

    assert!(
        claims
            .claims
            .iter()
            .filter(|claim| claim.kind.as_str() != "terminal_rule")
            .all(|claim| claim.provenance.assurance == ProvenanceAssurance::SourceBacked)
    );
    assert_eq!(claims.provenance.completeness, Completeness::Partial);
}

// --- Undecided / analysis scope separation -------------------------------------

#[test]
fn undecided_projects_as_metadata_not_a_claim() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);

    let undecided = claims
        .undecided
        .iter()
        .find(|item| item.target == "invariant:RetentionPlaceholder")
        .expect("undecided item exists");
    assert!(!undecided.reason.is_empty());
    assert!(undecided.source.is_some());

    // The same declaration still projects as an ordinary (unattributed) claim.
    assert!(claims.claims.iter().any(|claim| {
        claim
            .semantic_targets
            .iter()
            .any(|t| t == "property:invariant:RetentionPlaceholder")
    }));
}

#[test]
fn verify_bounds_project_into_analysis_scope_only() {
    let (source, root) = claims_fixture();
    let claims = project(&source, "document_claims_fixture.fsl", &root);

    assert!(
        claims
            .analysis_scope
            .instances
            .iter()
            .any(|instance| instance["entity"] == "Case" && instance["count"] == 2)
    );
    assert!(
        claims
            .analysis_scope
            .values
            .iter()
            .any(|value| value["number"] == "Budget")
    );

    // analysis_scope never leaks into a claim's own payload.
    let claim_set_value = serde_json::to_value(&claims.claims).expect("serialize");
    let claim_text = claim_set_value.to_string();
    assert!(!claim_text.contains("\"instances\""));
    assert!(!claim_text.contains("\"Budget\""));
}

// --- Schema conformance ---------------------------------------------------------

fn compiled_schema() -> jsonschema::Validator {
    let schema_text = read("../../schemas/fslc/document/requirement-claims.v1.schema.json");
    let schema_value: serde_json::Value =
        serde_json::from_str(&schema_text).expect("schema is valid JSON");
    let kernel_text = read("../../schemas/fslc/kernel/kernel.v2.schema.json");
    let kernel_value: serde_json::Value =
        serde_json::from_str(&kernel_text).expect("kernel schema is valid JSON");
    let registry = jsonschema::Registry::new()
        .add(
            "https://fsl.dev/schemas/fslc/kernel/kernel.v2.schema.json",
            &kernel_value,
        )
        .expect("kernel schema resource")
        .prepare()
        .expect("schema registry");
    jsonschema::options()
        .with_registry(&registry)
        .build(&schema_value)
        .expect("schema compiles")
}

#[test]
fn rcir_output_validates_against_the_v1_schema() {
    let validator = compiled_schema();
    let fixtures: [(String, PathBuf, &str); 3] = [
        (cancel_system().0, cancel_system().1, "cancel_system.fsl"),
        (
            claims_fixture().0,
            claims_fixture().1,
            "document_claims_fixture.fsl",
        ),
        (kpi_fixture().0, kpi_fixture().1, "document_kpi_fixture.fsl"),
    ];
    for (source, root, path) in fixtures {
        let claims = project(&source, path, &root);
        let value = serde_json::to_value(&claims).expect("serialize");
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "{path} failed schema validation: {errors:?}"
        );
    }
}

#[test]
fn schema_rejects_a_malformed_document_negative_control() {
    let validator = compiled_schema();
    let broken = serde_json::json!({"schema_version": "1.0.0", "result": "requirement_claims"});
    assert!(!validator.is_valid(&broken));
}

#[test]
fn schema_rejects_a_malformed_embedded_public_kernel() {
    let (source, root) = cancel_system();
    let mut value = serde_json::to_value(project(&source, "cancel_system.fsl", &root))
        .expect("serialize claims");
    value["public_kernel"]["actions"][0]["name"] = serde_json::Value::Null;
    assert!(!compiled_schema().is_valid(&value));
}
