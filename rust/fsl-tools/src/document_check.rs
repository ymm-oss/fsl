// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `fslc document check` (issue #329): a purely structural drift check
//! between a (possibly hand-edited) generated document and a fresh
//! re-projection + re-render of the same spec. No natural language is
//! interpreted — only frontmatter values, claim/slot marker structure, and
//! claim-block digests/text are compared.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::document::RequirementClaimSet;
use crate::document_digest::{CLAIM_BLOCK_DIGEST_ALGORITHM, framed_text_digest};
use crate::document_markers::{
    DOCUMENT_RENDERER, DOCUMENT_RENDERER_VERSION, DOCUMENT_SCHEMA, MarkerIssue, NORMATIVE_SCOPE,
    SLOT_NAMES, Segment, parse_body, parse_frontmatter,
};
use crate::document_render::Locale;

#[derive(Clone, Debug, Serialize)]
pub struct DriftReason {
    pub kind: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn reason(kind: &str, code: &str) -> DriftReason {
    DriftReason {
        kind: kind.to_owned(),
        code: code.to_owned(),
        claim: None,
        slot: None,
        detail: None,
    }
}

pub struct DocumentCheckReport {
    pub reasons: Vec<DriftReason>,
}

impl DocumentCheckReport {
    #[must_use]
    pub fn is_conformant(&self) -> bool {
        self.reasons.is_empty()
    }
}

/// A failure the artifact's own frontmatter caused, severe enough that no
/// structural comparison is meaningful — the equivalent of a parse/schema
/// error, not a drift finding (`fslc document check` exits 2, not 1).
pub enum CheckError {
    Marker(MarkerIssue),
    UnsupportedSchema(String),
    UnsupportedView(String),
    UnsupportedLang(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Marker(issue) => write!(f, "{issue}"),
            Self::UnsupportedSchema(schema) => {
                write!(
                    f,
                    "unsupported fsl_document_schema '{schema}' (expected '{DOCUMENT_SCHEMA}')"
                )
            }
            Self::UnsupportedView(view) => write!(f, "unsupported document view '{view}'"),
            Self::UnsupportedLang(lang) => write!(f, "unsupported document lang '{lang}'"),
        }
    }
}

/// Compare `artifact_text` (an existing `fslc document generate` artifact,
/// possibly hand-edited) against `fresh_claims`/`fresh_markdown` — a fresh
/// re-projection and re-render of the same spec, produced by the caller
/// under the locale the artifact's own frontmatter recorded.
///
/// # Errors
///
/// Returns [`CheckError`] when the artifact's frontmatter is missing,
/// malformed, or names a schema/view/lang this build does not support. A
/// body marker-grammar violation (an unclosed block, a malformed marker
/// line, a marker-like line inside a slot) is not an error here — it comes
/// back as a `marker_malformed` [`DriftReason`], since the artifact is still
/// a recognizable generated document, just internally inconsistent.
///
/// # Panics
///
/// Panics if `fresh_markdown` (the caller's own fresh
/// `render_requirements_document` output for the same locale) does not
/// parse as a well-formed generated document — that would be this crate's
/// own renderer producing an artifact its own parser rejects.
pub fn check_requirements_document(
    artifact_text: &str,
    fresh_claims: &RequirementClaimSet,
    fresh_markdown: &str,
) -> Result<DocumentCheckReport, CheckError> {
    let artifact_text = artifact_text.replace("\r\n", "\n");
    let (frontmatter, consumed) = parse_frontmatter(&artifact_text).map_err(CheckError::Marker)?;
    if frontmatter.schema != DOCUMENT_SCHEMA {
        return Err(CheckError::UnsupportedSchema(frontmatter.schema));
    }
    if frontmatter.view != "requirements" {
        return Err(CheckError::UnsupportedView(frontmatter.view));
    }
    if Locale::parse(&frontmatter.lang).is_none() {
        return Err(CheckError::UnsupportedLang(frontmatter.lang));
    }

    let mut reasons = Vec::new();

    let renderer_changed = frontmatter.renderer != DOCUMENT_RENDERER
        || frontmatter.renderer_version != DOCUMENT_RENDERER_VERSION;
    if renderer_changed {
        reasons.push(reason("renderer_changed", "FSL-DOC-RENDERER-CHANGED"));
    }
    if frontmatter.normative_scope != NORMATIVE_SCOPE {
        reasons.push(reason(
            "normative_scope_changed",
            "FSL-DOC-EDIT-OUTSIDE-SLOT",
        ));
    }
    if frontmatter.spec_digest != fresh_claims.spec.spec_digest {
        reasons.push(reason("spec_digest_mismatch", "FSL-DOC-SPEC-DRIFT"));
    }
    if frontmatter.claim_set_digest != fresh_claims.spec.claim_set_digest {
        reasons.push(reason("claim_set_digest_mismatch", "FSL-DOC-SPEC-DRIFT"));
    }

    let artifact_body = artifact_text.get(consumed..).unwrap_or("");
    let artifact_segments = match parse_body(artifact_body) {
        Ok(segments) => segments,
        Err(issue) => {
            reasons.push(marker_malformed_reason(&issue));
            return Ok(DocumentCheckReport { reasons });
        }
    };
    let (fresh_frontmatter, fresh_consumed) =
        parse_frontmatter(fresh_markdown).expect("this build's own render always has frontmatter");
    debug_assert_eq!(fresh_frontmatter.schema, DOCUMENT_SCHEMA);
    let fresh_segments = parse_body(fresh_markdown.get(fresh_consumed..).unwrap_or(""))
        .expect("this build's own render always has well-formed markers");

    check_claims(
        &artifact_segments,
        &fresh_segments,
        renderer_changed,
        &mut reasons,
    );
    check_slots(&artifact_segments, &mut reasons);
    check_residue(
        &artifact_segments,
        &fresh_segments,
        renderer_changed,
        &mut reasons,
    );

    Ok(DocumentCheckReport { reasons })
}

fn marker_malformed_reason(issue: &MarkerIssue) -> DriftReason {
    let mut drift = reason("marker_malformed", "FSL-DOC-MARKER-MALFORMED");
    drift.detail = Some(issue.to_string());
    drift
}

fn claim_segments(segments: &[Segment]) -> Vec<(&str, &str, &str)> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Claim { id, digest, body } => {
                Some((id.as_str(), digest.as_str(), body.as_str()))
            }
            _ => None,
        })
        .collect()
}

fn check_claims(
    artifact_segments: &[Segment],
    fresh_segments: &[Segment],
    renderer_changed: bool,
    reasons: &mut Vec<DriftReason>,
) {
    let artifact_claims = claim_segments(artifact_segments);
    let fresh_claims = claim_segments(fresh_segments);

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (id, ..) in &artifact_claims {
        *counts.entry(*id).or_insert(0) += 1;
    }
    for (id, count) in &counts {
        if *count > 1 {
            let mut drift = reason("claim_duplicate", "FSL-DOC-BLOCK-DUPLICATE");
            drift.claim = Some((*id).to_owned());
            reasons.push(drift);
        }
    }

    let artifact_ids: BTreeSet<&str> = counts.keys().copied().collect();
    let fresh_ids: BTreeSet<&str> = fresh_claims.iter().map(|(id, ..)| *id).collect();
    for id in fresh_ids.difference(&artifact_ids) {
        let mut drift = reason("claim_missing", "FSL-DOC-BLOCK-MISSING");
        drift.claim = Some((*id).to_owned());
        reasons.push(drift);
    }
    for id in artifact_ids.difference(&fresh_ids) {
        let mut drift = reason("claim_unknown", "FSL-DOC-BLOCK-UNKNOWN");
        drift.claim = Some((*id).to_owned());
        reasons.push(drift);
    }

    let sets_match = artifact_ids == fresh_ids && counts.values().all(|&count| count == 1);
    if sets_match {
        let artifact_order: Vec<&str> = artifact_claims.iter().map(|(id, ..)| *id).collect();
        let fresh_order: Vec<&str> = fresh_claims.iter().map(|(id, ..)| *id).collect();
        if artifact_order != fresh_order {
            reasons.push(reason("claim_reordered", "FSL-DOC-BLOCK-REORDERED"));
        }
    }

    if renderer_changed {
        return;
    }
    let fresh_bodies: BTreeMap<&str, &str> = fresh_claims
        .iter()
        .map(|(id, _, body)| (*id, *body))
        .collect();
    for (id, digest, body) in &artifact_claims {
        let Some(fresh_body) = fresh_bodies.get(id) else {
            continue; // already reported as claim_unknown
        };
        let recomputed = framed_text_digest(CLAIM_BLOCK_DIGEST_ALGORITHM, body);
        let mut drift = reason("claim_changed", "FSL-DOC-BLOCK-DRIFT");
        drift.claim = Some((*id).to_owned());
        if recomputed != *digest {
            drift.detail = Some("artifact_edited".to_owned());
            reasons.push(drift);
        } else if body != fresh_body {
            drift.detail = Some("render_diverged".to_owned());
            reasons.push(drift);
        }
    }
}

fn check_slots(artifact_segments: &[Segment], reasons: &mut Vec<DriftReason>) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for segment in artifact_segments {
        if let Segment::Slot { name, .. } = segment {
            *counts.entry(name.as_str()).or_insert(0) += 1;
        }
    }
    for (name, count) in &counts {
        if *count > 1 {
            let mut drift = reason("slot_duplicate", "FSL-DOC-MARKER-MALFORMED");
            drift.slot = Some((*name).to_owned());
            reasons.push(drift);
        }
    }
    let artifact_names: BTreeSet<&str> = counts.keys().copied().collect();
    let expected_names: BTreeSet<&str> = SLOT_NAMES.iter().copied().collect();
    for name in expected_names.difference(&artifact_names) {
        let mut drift = reason("slot_missing", "FSL-DOC-MARKER-MALFORMED");
        drift.slot = Some((*name).to_owned());
        reasons.push(drift);
    }
    for name in artifact_names.difference(&expected_names) {
        let mut drift = reason("slot_unknown", "FSL-DOC-MARKER-MALFORMED");
        drift.slot = Some((*name).to_owned());
        reasons.push(drift);
    }
}

/// Compares residue (non-marker) text position-by-position, but only when
/// the two segment sequences otherwise line up exactly (same length, same
/// claim id / slot name at every marker position) — a structural mismatch
/// already produces its own reason above, and a positional residue diff
/// against a misaligned sequence would only add noise.
fn check_residue(
    artifact_segments: &[Segment],
    fresh_segments: &[Segment],
    renderer_changed: bool,
    reasons: &mut Vec<DriftReason>,
) {
    if renderer_changed || artifact_segments.len() != fresh_segments.len() {
        return;
    }
    let aligned = artifact_segments
        .iter()
        .zip(fresh_segments)
        .all(|pair| match pair {
            (Segment::Residue(_), Segment::Residue(_)) => true,
            (Segment::Claim { id: a, .. }, Segment::Claim { id: b, .. })
            | (Segment::Slot { name: a, .. }, Segment::Slot { name: b, .. }) => a == b,
            _ => false,
        });
    if !aligned {
        return;
    }
    let diverges = artifact_segments
        .iter()
        .zip(fresh_segments)
        .any(|pair| matches!(pair, (Segment::Residue(a), Segment::Residue(b)) if a != b));
    if diverges {
        reasons.push(reason("edit_outside_slot", "FSL-DOC-EDIT-OUTSIDE-SLOT"));
    }
}
