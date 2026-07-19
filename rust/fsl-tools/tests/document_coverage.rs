// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Coverage registry and no-silent-omission gate for the RCIR v1 projector
//! (issue #328). See `docs/DESIGN-document-coverage-registry.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fsl_tools::{
    RCIR_SUPPORTED_DIALECTS, RCIR_TARGET_KIND_REGISTRY, RequirementClaimSet, target_kind,
};
use serde_json::Value;

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn workspace_root() -> PathBuf {
    manifest_path("../..")
}

fn project(source: &str, source_path: &str, root: &Path) -> RequirementClaimSet {
    fsl_tools::project_requirement_claims_from_source(source, Some(source_path), root)
        .unwrap_or_else(|error| panic!("project {source_path}: {error}"))
}

/// The exact fixture corpus issues #325-#327 already established: together
/// they exercise every one of `RCIR_TARGET_KIND_REGISTRY`'s 11 kinds at
/// least once (`cancel_system.fsl` alone covers action / all four property
/// kinds / terminal / acceptance / forbidden / refinement via its
/// `implements`; `document_kpi_fixture.fsl` adds `projection`; `init` is
/// exercised by every fixture's own `init { ... }` block).
fn corpus() -> Vec<RequirementClaimSet> {
    vec![
        project(
            &read("../../examples/pm/cancel_system.fsl"),
            "examples/pm/cancel_system.fsl",
            &manifest_path("../../examples/pm"),
        ),
        project(
            &read("tests/fixtures/document_claims_fixture.fsl"),
            "document_claims_fixture.fsl",
            &manifest_path("tests/fixtures"),
        ),
        project(
            &read("tests/fixtures/document_kpi_fixture.fsl"),
            "document_kpi_fixture.fsl",
            &manifest_path("tests/fixtures"),
        ),
    ]
}

// --- Deliverable 1: projection completeness -------------------------------

#[test]
fn every_fixture_authored_target_is_exactly_partitioned() {
    for claims in corpus() {
        let coverage = &claims.coverage;
        assert_eq!(
            coverage.counts.authored,
            coverage.counts.rendered + coverage.counts.unattributed + coverage.counts.unsupported,
            "{}: rendered + unattributed + unsupported must equal authored targets",
            claims.spec.name
        );

        let rendered: BTreeSet<&String> = coverage.rendered.iter().collect();
        let unattributed: BTreeSet<&String> = coverage.unattributed.iter().collect();
        let unsupported: BTreeSet<&String> = coverage
            .unsupported
            .iter()
            .map(|entry| &entry.target)
            .collect();
        let authored: BTreeSet<&String> = coverage.authored.iter().collect();

        assert!(
            rendered.is_disjoint(&unattributed)
                && rendered.is_disjoint(&unsupported)
                && unattributed.is_disjoint(&unsupported),
            "{}: rendered/unattributed/unsupported must not overlap",
            claims.spec.name
        );
        let union: BTreeSet<&String> = rendered
            .into_iter()
            .chain(unattributed)
            .chain(unsupported)
            .collect();
        assert_eq!(
            union, authored,
            "{}: every authored target must land in exactly one of \
             rendered/unattributed/unsupported — none dropped, none duplicated",
            claims.spec.name
        );
    }
}

// --- Deliverable 2: no-silent-omission gate --------------------------------

#[test]
fn every_authored_target_kind_across_the_corpus_is_registered_and_every_registered_kind_is_exercised()
 {
    let mut observed = BTreeSet::new();
    for claims in corpus() {
        for target in &claims.coverage.authored {
            observed.insert(target_kind(target));
        }
    }
    let registered: BTreeSet<String> = RCIR_TARGET_KIND_REGISTRY
        .iter()
        .map(|row| row.kind.to_owned())
        .collect();
    assert_eq!(
        observed, registered,
        "a target kind is observed in the fixture corpus with no coverage-registry row, or a \
         registered row's kind is never exercised by any fixture — add a row to \
         RCIR_TARGET_KIND_REGISTRY in rust/fsl-tools/src/document_coverage.rs, or add/extend a \
         fixture, so this set stays exact"
    );
}

/// The Kernel-native subset of the registry (every kind derived directly
/// from a `KernelModel` collection, as opposed to a requirements-dialect
/// surface-only concept RCIR also projects: `acceptance`/`forbidden`
/// trace cases, `projection` (KPI), and `refinement` (`implements`), none
/// of which appear in the dialect-neutral Public Kernel contract) must match
/// `schemas/fslc/kernel/kernel.v1.schema.json`'s own required-key lists
/// exactly. This is the coupled-change gate: a new Kernel-level semantic
/// element added to the language (and, per that schema's own discipline, to
/// this required-key list) fails this test until the RCIR projector is
/// updated to classify it — it cannot ship with the projector silently
/// unaware the new construct exists at all, which the corpus-observed test
/// above cannot catch on its own (nothing pushes an unrecognized construct
/// into any fixture's `coverage.authored` in the first place).
const NON_TARGET_TOP_LEVEL_KERNEL_KEYS: &[&str] = &[
    "$schema",
    "schema_version",
    "language_version",
    "spec",
    "semantics",
    "constants",
    "types",
    "state",
    "properties",
];

fn public_kernel_schema() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("schemas/fslc/kernel/kernel.v1.schema.json"),
        )
        .expect("read Public Kernel v1 schema"),
    )
    .expect("Kernel schema is valid JSON")
}

fn kernel_semantic_schema_keys(schema: &Value) -> BTreeSet<String> {
    let top_required: BTreeSet<&str> = schema["required"]
        .as_array()
        .expect("top-level required")
        .iter()
        .map(|value| value.as_str().expect("string key"))
        .collect();
    let properties_required: BTreeSet<&str> = schema["properties"]["properties"]["required"]
        .as_array()
        .expect("properties.required")
        .iter()
        .map(|value| value.as_str().expect("string key"))
        .collect();

    let non_targets: BTreeSet<&str> = NON_TARGET_TOP_LEVEL_KERNEL_KEYS.iter().copied().collect();
    top_required
        .difference(&non_targets)
        .map(|key| (*key).to_owned())
        .chain(
            properties_required
                .into_iter()
                .map(|key| format!("properties.{key}")),
        )
        .collect()
}

fn registered_kernel_schema_keys() -> BTreeSet<String> {
    RCIR_TARGET_KIND_REGISTRY
        .iter()
        .filter_map(|row| row.kernel_schema_key.map(str::to_owned))
        .collect()
}

#[test]
fn kernel_native_target_kinds_match_the_public_kernel_v1_schema_required_keys() {
    let expected = kernel_semantic_schema_keys(&public_kernel_schema());
    let registered = registered_kernel_schema_keys();
    assert_eq!(
        registered, expected,
        "a Kernel-native semantic element kind (from kernel.v1.schema.json's own required-key \
         lists) has no matching row in RCIR_TARGET_KIND_REGISTRY, or a registered \
         Kernel-native row doesn't correspond to any required Kernel schema key — add a row to \
         rust/fsl-tools/src/document_coverage.rs (and, if the schema itself just gained a new \
         required key, project it in rust/fsl-tools/src/document_project.rs first)"
    );
}

// --- Deliverable 3 (issue #334): the dialect-activation tripwire -----------

/// `RCIR_SUPPORTED_DIALECTS` (`requirements`/`spec` only, issue #334's
/// deliberately narrow v1) must stay exactly the dialect-keyword registry
/// minus the eight dialects `fsl_tools::tests::document::
/// rejects_every_unsupported_dialect_fail_closed` proves are rejected. This
/// is the activation-contract tripwire: a dialect can only move into
/// `RCIR_SUPPORTED_DIALECTS` together with a dedicated document adapter and
/// its own coverage-registry row (issue #328's no-silent-omission gate,
/// above) — moving it without both fails this test. It also catches a
/// wholly new dialect keyword added to the language: `DIALECT_KEYWORDS`
/// growing without a corresponding decision here fails too, forcing an
/// explicit RCIR posture for the newcomer rather than an implicit one. See
/// `docs/DESIGN-document-dialect-adapters.md`.
#[test]
fn rcir_supported_dialects_are_exactly_spec_and_requirements() {
    let supported: BTreeSet<&str> = RCIR_SUPPORTED_DIALECTS.iter().copied().collect();
    assert_eq!(
        supported,
        BTreeSet::from(["requirements", "spec"]),
        "RCIR_SUPPORTED_DIALECTS drifted from issue #334's accepted v1 scope"
    );

    let all_keywords: BTreeSet<&str> = fsl_syntax::DIALECT_KEYWORDS.iter().copied().collect();
    let rejected: BTreeSet<&str> = all_keywords.difference(&supported).copied().collect();
    assert_eq!(
        rejected,
        BTreeSet::from([
            "business",
            "governance",
            "compose",
            "refinement",
            "domain",
            "dbsystem",
            "ai_component",
            "agent",
        ]),
        "a dialect keyword was added to or removed from fsl_syntax::DIALECT_KEYWORDS without an \
         explicit RCIR-support decision here and in rust/fsl-tools/tests/document.rs's \
         rejects_every_unsupported_dialect_fail_closed table"
    );
}

#[test]
fn a_new_required_kernel_collection_is_not_silently_ignored() {
    let mut schema = public_kernel_schema();
    schema["required"]
        .as_array_mut()
        .expect("top-level required")
        .push(Value::String("new_semantic_collection".to_owned()));
    let expected = kernel_semantic_schema_keys(&schema);
    let registered = registered_kernel_schema_keys();
    assert_ne!(registered, expected);
    assert!(expected.contains("new_semantic_collection"));
}
