// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for generated block markers and frontmatter (issue
//! #329): `fsl_tools::document_markers`'s grammar/parser, and how
//! `render_requirements_document` (issue #326) now emits it. `fslc document
//! check`'s comparison algorithm is exercised end-to-end in
//! `rust/fslc/tests/document_check_cli.rs`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fsl_tools::{DOCUMENT_RENDERER, DOCUMENT_RENDERER_VERSION, DOCUMENT_SCHEMA, Locale, Segment};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn cancel_system_markdown(locale: Locale) -> String {
    let source = read("../../examples/pm/cancel_system.fsl");
    let root = manifest_path("../../examples/pm");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("examples/pm/cancel_system.fsl"),
        &root,
    )
    .expect("project cancel_system.fsl");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&source).expect("trace contract");
    fsl_tools::render_requirements_document(&claims, &kernel, &model, trace.as_ref(), locale, None)
        .expect("render paired RCIR")
        .markdown
}

#[test]
fn generated_document_starts_with_a_well_formed_frontmatter_block() {
    let markdown = cancel_system_markdown(Locale::Ja);
    let parsed = fsl_tools::parse_generated_document(&markdown).expect("parse generated document");
    assert_eq!(parsed.frontmatter.schema, DOCUMENT_SCHEMA);
    assert_eq!(parsed.frontmatter.view, "requirements");
    assert_eq!(parsed.frontmatter.lang, "ja");
    assert_eq!(
        parsed.frontmatter.source.as_deref(),
        Some("examples/pm/cancel_system.fsl")
    );
    assert_eq!(parsed.frontmatter.renderer, DOCUMENT_RENDERER);
    assert_eq!(
        parsed.frontmatter.renderer_version,
        DOCUMENT_RENDERER_VERSION
    );
    assert_eq!(
        parsed.frontmatter.normative_scope,
        "generated-claim-blocks-only"
    );
    assert!(parsed.frontmatter.spec_digest.starts_with("sha256:"));
    assert!(parsed.frontmatter.claim_set_digest.starts_with("sha256:"));
}

#[test]
fn every_claim_renders_inside_exactly_one_marker_pair_with_a_unique_id() {
    let markdown = cancel_system_markdown(Locale::Ja);
    let claims = {
        let source = read("../../examples/pm/cancel_system.fsl");
        let root = manifest_path("../../examples/pm");
        fsl_tools::project_requirement_claims_from_source(
            &source,
            Some("examples/pm/cancel_system.fsl"),
            &root,
        )
        .expect("project")
    };
    let parsed = fsl_tools::parse_generated_document(&markdown).expect("parse");
    let marker_ids: Vec<&str> = parsed
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Claim { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let unique: BTreeSet<&str> = marker_ids.iter().copied().collect();
    assert_eq!(
        marker_ids.len(),
        unique.len(),
        "every claim id must appear in exactly one marker pair"
    );
    let expected_ids: BTreeSet<&str> = claims
        .claims
        .iter()
        .map(|claim| claim.id.as_str())
        .collect();
    assert_eq!(unique, expected_ids);
}

#[test]
fn back_references_carry_no_markers() {
    // REQ-1 and REQ-2 in the shared-claim fixture both reference the same
    // claim; only the first occurrence gets a marker (rust/fsl-tools/tests/
    // document_render.rs's `shared_claim_across_two_requirements_...` test
    // pins the back-reference wording itself).
    let source = read("tests/fixtures/document_claims_fixture.fsl");
    let root = manifest_path("tests/fixtures");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("document_claims_fixture.fsl"),
        &root,
    )
    .expect("project");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&source).expect("trace contract");
    let markdown = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        trace.as_ref(),
        Locale::Ja,
        None,
    )
    .expect("render paired RCIR")
    .markdown;
    assert!(
        claims
            .claims
            .iter()
            .any(|claim| claim.requirements.len() > 1),
        "fixture must actually exercise a claim shared by 2+ requirements"
    );
    let parsed = fsl_tools::parse_generated_document(&markdown).expect("parse");
    let marker_count = parsed
        .segments
        .iter()
        .filter(|segment| matches!(segment, Segment::Claim { .. }))
        .count();
    // One marker per distinct claim id, however many requirements link it —
    // a shared claim renders in full once and every later reference is an
    // unmarked back-reference (`document_render.rs`'s `back_reference`).
    assert_eq!(marker_count, claims.claims.len());
}

#[test]
fn marker_digest_equals_framed_text_digest_of_the_body() {
    let markdown = cancel_system_markdown(Locale::En);
    let parsed = fsl_tools::parse_generated_document(&markdown).expect("parse");
    let mut checked_any = false;
    for segment in &parsed.segments {
        if let Segment::Claim { digest, body, .. } = segment {
            let expected =
                fsl_tools::framed_text_digest(fsl_tools::CLAIM_BLOCK_DIGEST_ALGORITHM, body);
            assert_eq!(*digest, expected);
            checked_any = true;
        }
    }
    assert!(checked_any, "fixture must contain at least one claim");
}

#[test]
fn background_slot_is_present_exactly_once_with_the_fixed_name() {
    let markdown = cancel_system_markdown(Locale::Ja);
    let parsed = fsl_tools::parse_generated_document(&markdown).expect("parse");
    let slot_names: Vec<&str> = parsed
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Slot { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(slot_names, vec!["background"]);
    assert_eq!(fsl_tools::SLOT_NAMES, &["background"]);
}

#[test]
fn parser_roundtrips_a_freshly_rendered_document() {
    // The renderer's own output must always parse cleanly under its own
    // grammar — this is what `fslc document check` relies on when it
    // re-renders and re-parses the fresh side.
    for locale in [Locale::Ja, Locale::En] {
        let markdown = cancel_system_markdown(locale);
        fsl_tools::parse_generated_document(&markdown)
            .unwrap_or_else(|error| panic!("{locale:?} render must parse: {error}"));
    }
}

#[test]
fn a_document_with_no_frontmatter_is_rejected() {
    let error = fsl_tools::parse_generated_document("# just a heading, no frontmatter")
        .expect_err("missing frontmatter must be rejected");
    assert!(error.to_string().contains("frontmatter"));
}

#[test]
fn an_unknown_frontmatter_key_is_rejected() {
    let text = format!(
        "---\nfsl_document_schema: {DOCUMENT_SCHEMA}\nview: requirements\nlang: ja\nrenderer: {DOCUMENT_RENDERER}\nrenderer_version: {DOCUMENT_RENDERER_VERSION}\nnormative_scope: generated-claim-blocks-only\nspec_digest: sha256:0\nclaim_set_digest: sha256:0\nunknown_key: surprise\n---\n\nbody"
    );
    let error =
        fsl_tools::parse_generated_document(&text).expect_err("unknown key must be rejected");
    assert!(error.to_string().contains("unknown_key"));
}

#[test]
fn a_marker_like_line_inside_a_slot_is_rejected() {
    let text = format!(
        "---\nfsl_document_schema: {DOCUMENT_SCHEMA}\nview: requirements\nlang: ja\nrenderer: {DOCUMENT_RENDERER}\nrenderer_version: {DOCUMENT_RENDERER_VERSION}\nnormative_scope: generated-claim-blocks-only\nspec_digest: sha256:0\nclaim_set_digest: sha256:0\n---\n\n<!-- fsl:slot begin name=\"background\" normative=\"false\" -->\nsome text\n<!-- fsl:claim begin id=\"x\" digest=\"sha256:0\" -->\n<!-- fsl:slot end -->"
    );
    let error =
        fsl_tools::parse_generated_document(&text).expect_err("marker-like line must be rejected");
    assert!(error.to_string().contains("marker-like"));
}
