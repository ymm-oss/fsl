// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Golden and synchronization tests for the conformance corpus feature
//! coverage matrix (issue #223). See `docs/DESIGN-kernel-contract.md`'s
//! "Conformance coverage matrix" section for the coupled-change discipline
//! these tests enforce.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fslc_rust::coverage::{
    COVERAGE_SCHEMA_ID, OUTCOME_FEATURE_KEYS, SEMANTICS_FEATURE_KEYS, coverage_matrix,
    coverage_matrix_markdown,
};
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Every `kernel.v1.schema.json` `semantics` key must have a coverage-matrix
/// row. A newly added semantics key that isn't wired up here would
/// otherwise ship with silent, unmeasured coverage.
#[test]
fn semantics_schema_keys_are_all_registered_as_feature_rows() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("schemas/fslc/kernel/kernel.v1.schema.json"),
        )
        .expect("read Kernel schema"),
    )
    .expect("Kernel schema JSON");
    let required = schema["properties"]["semantics"]["required"]
        .as_array()
        .expect("semantics.required")
        .iter()
        .map(|value| value.as_str().expect("string key").to_owned())
        .collect::<BTreeSet<_>>();
    let registered = SEMANTICS_FEATURE_KEYS
        .iter()
        .map(|&(schema_key, _)| schema_key.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        required, registered,
        "every kernel.v1.schema.json semantics key must have a coverage-matrix row \
         (add one to SEMANTICS_FEATURE_KEYS/build_rows in rust/fslc/src/coverage.rs)"
    );
}

/// Every `outcome.kind` the fixture corpus actually emits must be
/// registered in `OUTCOME_FEATURE_KEYS` and exercised. Regenerates vectors
/// directly (rather than trusting the matrix's own bookkeeping) so this is
/// an independent cross-check, and would fail loudly if `fsl-runtime` ever
/// grows a new violation kind that the fixtures happen to trigger without a
/// matching coverage row.
#[test]
fn every_outcome_kind_the_corpus_emits_is_registered_and_exercised() {
    let mut observed = BTreeSet::new();
    for (file, depth) in [("kernel_contract.fsl", 2), ("conformance_failures.fsl", 1)] {
        let path = fixture(file);
        let source = std::fs::read_to_string(&path).expect("read fixture");
        let kernel = parse_kernel_source(
            &source,
            &FsResolver::new(path.parent().expect("fixture dir")),
        )
        .expect("parse fixture");
        let model = build_model(kernel).expect("build fixture");
        let vectors = fslc_rust::conformance_vectors(&model, depth).expect("generate vectors");
        for vector in vectors["vectors"].as_array().expect("vectors") {
            observed.insert(
                vector["outcome"]["kind"]
                    .as_str()
                    .expect("outcome kind")
                    .to_owned(),
            );
        }
    }

    let registered = OUTCOME_FEATURE_KEYS
        .iter()
        .map(|&(kind, _)| kind.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed, registered,
        "every outcome.kind the fixture corpus emits must have a matching outcome_<kind> row \
         (add one to OUTCOME_FEATURE_KEYS/build_rows in rust/fslc/src/coverage.rs)"
    );

    let matrix = coverage_matrix().expect("coverage matrix");
    let features = matrix["features"].as_array().expect("features");
    for &(_, key) in OUTCOME_FEATURE_KEYS {
        let level = features
            .iter()
            .find(|feature| feature["key"] == key)
            .and_then(|feature| feature["level"].as_str());
        assert_eq!(
            level,
            Some("exercised"),
            "outcome row `{key}` must be exercised"
        );
    }
}

/// `coverage_matrix()` itself is the uncovered-feature enforcement gate: it
/// returns `Err` naming every feature row that falls short of its required
/// evidence level instead of silently under-reporting. This test is the
/// loud CI failure the issue asks for.
#[test]
fn every_feature_row_meets_its_required_coverage_level() {
    coverage_matrix().expect("conformance coverage matrix must have no uncovered features");
}

/// Golden JSON/Markdown equality. Regenerate with
/// `FSLC_COVERAGE_UPDATE=1 cargo test -p fslc-rust --test conformance_coverage`
/// after an intended coverage-matrix change.
#[test]
fn coverage_matrix_matches_the_v1_golden_json_and_markdown() {
    let matrix = coverage_matrix().expect("coverage matrix");
    let markdown = coverage_matrix_markdown(&matrix);

    let json_path = fixture("conformance_coverage.v1.json");
    let markdown_path = fixture("conformance_coverage.v1.md");

    if std::env::var("FSLC_COVERAGE_UPDATE").is_ok() {
        let pretty = serde_json::to_string_pretty(&matrix).expect("serialize coverage matrix");
        std::fs::write(&json_path, format!("{pretty}\n")).expect("write golden JSON");
        std::fs::write(&markdown_path, &markdown).expect("write golden Markdown");
        return;
    }

    let expected_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&json_path)
            .expect("read golden JSON (run with FSLC_COVERAGE_UPDATE=1 to generate)"),
    )
    .expect("golden JSON");
    assert_eq!(matrix, expected_json);

    let expected_markdown = std::fs::read_to_string(&markdown_path)
        .expect("read golden Markdown (run with FSLC_COVERAGE_UPDATE=1 to generate)");
    assert_eq!(markdown, expected_markdown);
}

#[test]
fn published_coverage_schema_id_matches_the_rust_api_constant() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("schemas/fslc/kernel/conformance-coverage.v1.schema.json"),
        )
        .expect("read coverage schema"),
    )
    .expect("coverage schema JSON");
    assert_eq!(schema["$id"], COVERAGE_SCHEMA_ID);
}
