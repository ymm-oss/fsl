// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1475: shared renderer validation and fallible legacy-fresh parsing.

use std::path::Path;

use fsl_tools::{AppliedEvidence, CheckError, Locale, MarkerIssue, RequirementClaimSet};
use serde_json::json;
use sha2::{Digest, Sha256};

fn fixture() -> (
    String,
    fsl_core::KernelSpec,
    fsl_core::KernelModel,
    RequirementClaimSet,
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let source = std::fs::read_to_string(root.join("approval.fsl")).expect("read accepted source");
    let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(&root))
        .expect("parse source");
    let model = fsl_core::build_model(kernel.clone()).expect("check source");
    let claims =
        fsl_tools::project_requirement_claims_from_source(&source, Some("spec.fsl"), &root)
            .expect("project checked source");
    (source, kernel, model, claims)
}

fn normal() -> (RequirementClaimSet, String) {
    let (source, kernel, model, claims) = fixture();
    let rendered = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        Locale::En,
        None,
        None,
        None,
    )
    .expect("normal render");
    (claims, rendered.markdown)
}

#[test]
fn renderer_rejects_marker_in_evidence_result() {
    let (source, kernel, model, claims) = fixture();
    let files = vec![(
        "evidence.json".to_owned(),
        json!({
            "requirement": {"id": "REQ-190"},
            "completeness": "unbounded", "result": "verified\n<!-- fsl:bogus -->\n"
        }),
    )];
    let evidence = AppliedEvidence {
        files: &files,
        digest: "sha256:test-owned-evidence",
    };
    let rendered = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        Locale::En,
        None,
        Some(&evidence),
        None,
    );
    let Err(error) = rendered else {
        panic!("renderer accepted malformed generated marker");
    };
    assert_eq!(
        error,
        "malformed generated-block marker: \"<!-- fsl:bogus -->\""
    );
}

#[test]
fn renderer_rejects_newline_source_label() {
    let (source, kernel, model, _) = fixture();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("spec.fsl\nnot-a-frontmatter-field"),
        &root,
    )
    .expect("project the same checked source with the hostile source label");
    let rendered = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        Locale::En,
        None,
        None,
        None,
    );
    let Err(error) = rendered else {
        panic!("renderer accepted malformed generated frontmatter");
    };
    assert_eq!(
        error,
        "malformed frontmatter line: \"not-a-frontmatter-field\""
    );
}

#[test]
fn checker_returns_marker_error_for_legacy_fresh_body() {
    let (claims, artifact) = normal();
    let fresh = format!("{artifact}\n<!-- fsl:bogus -->\n");
    match fsl_tools::check_requirements_document(&artifact, &claims, &fresh) {
        Err(CheckError::Marker(MarkerIssue::MalformedMarkerLine(line))) => {
            assert_eq!(line, "<!-- fsl:bogus -->");
        }
        _ => panic!("fresh marker failure must be CheckError::Marker, not drift/success"),
    }
}

#[test]
fn checker_returns_marker_error_for_legacy_fresh_frontmatter() {
    let (claims, artifact) = normal();
    assert!(artifact.contains("source: spec.fsl"));
    let fresh = artifact.replacen(
        "source: spec.fsl",
        "source: spec.fsl\nnot-a-frontmatter-field",
        1,
    );
    match fsl_tools::check_requirements_document(&artifact, &claims, &fresh) {
        Err(CheckError::Marker(MarkerIssue::MalformedFrontmatterLine(line))) => {
            assert_eq!(line, "not-a-frontmatter-field");
        }
        _ => panic!("fresh frontmatter failure must be CheckError::Marker"),
    }
}

#[test]
fn checker_rejects_schema_overridden_by_newline_source_label() {
    let (_, artifact) = normal();
    let (source, kernel, model, _) = fixture();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("spec.fsl\nfsl_document_schema: bad"),
        &root,
    )
    .expect("project the same checked source with a schema-shaped source label");
    let fresh = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        Locale::En,
        None,
        None,
        None,
    )
    .expect("duplicate frontmatter keys remain syntactically accepted");
    let frontmatter = fsl_tools::parse_frontmatter_only(&fresh.markdown)
        .expect("fresh frontmatter parses before schema validation");
    assert_eq!(frontmatter.schema, "bad");
    match fsl_tools::check_requirements_document(&artifact, &claims, &fresh.markdown) {
        Err(CheckError::UnsupportedSchema(schema)) => assert_eq!(schema, "bad"),
        _ => {
            panic!("fresh schema override must be CheckError::UnsupportedSchema, not drift/success")
        }
    }
}

#[test]
fn ordinary_bytes_digests_and_artifact_only_drift_are_preserved() {
    let (claims, artifact) = normal();
    // Byte baseline captured from the named, unmodified 8e54e862 renderer.
    // Hash the entire Markdown, including frontmatter and generated blocks.
    assert_eq!(
        format!("{:x}", Sha256::digest(artifact.as_bytes())),
        "a9bbae4871b98ac97e7a68215af8b442577da05ce2a405a50aee6de5683a0311"
    );
    assert_eq!(
        claims.spec.spec_digest,
        "sha256:1d8ad00cf360d39990a8cba12ef5bd10961524e774054fa6dbe3f548a2e44aca"
    );
    assert_eq!(
        claims.spec.claim_set_digest,
        "sha256:6b11a312eb01a7a8f2c5ac3bdd6c9f4f7d0a116441e963f709c6b90d10dd7afe"
    );
    let (again_claims, again) = normal();
    assert_eq!(artifact.as_bytes(), again.as_bytes());
    assert_eq!(claims.spec.spec_digest, again_claims.spec.spec_digest);
    assert_eq!(
        claims.spec.claim_set_digest,
        again_claims.spec.claim_set_digest
    );
    let report = fsl_tools::check_requirements_document(&artifact, &claims, &again)
        .unwrap_or_else(|error| panic!("normal check: {error}"));
    assert!(report.is_conformant());
    let malformed_artifact = format!("{artifact}\n<!-- fsl:bogus -->\n");
    let report = fsl_tools::check_requirements_document(&malformed_artifact, &claims, &again)
        .unwrap_or_else(|error| panic!("artifact body stays drift, not schema error: {error}"));
    assert_eq!(
        serde_json::to_value(report.reasons).expect("serialize reasons"),
        json!([{
            "kind": "marker_malformed", "code": "FSL-DOC-MARKER-MALFORMED",
            "detail": "malformed generated-block marker: \"<!-- fsl:bogus -->\""
        }])
    );
}
