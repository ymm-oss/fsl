// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Generated block markers and frontmatter for `fslc document generate`'s
//! Markdown artifact (issue #329), plus the parser `fslc document check`
//! uses to read a (possibly hand-edited) artifact back in.
//!
//! Markers are HTML comments, so they render invisibly. The grammar is a
//! fixed set of whole-line patterns; a hand-rolled scanner is used rather
//! than a `regex`/YAML dependency (neither is a workspace dependency) since
//! the alphabet is closed — a kernel identifier and a hex digest can never
//! contain `"` or a newline.

use std::collections::BTreeMap;

use crate::document_render::Locale;

/// Versions the *artifact format* (frontmatter key set + marker grammar) —
/// distinct from `RCIR_SCHEMA_VERSION`, which versions the claims JSON, not
/// the Markdown envelope around it.
pub const DOCUMENT_SCHEMA: &str = "fsl-requirements-document-v1";
pub const DOCUMENT_RENDERER: &str = "fslc-document-renderer";
/// Bumped by hand whenever a template string or section skeleton changes.
/// Deliberately independent of the crate version: keying this to
/// `CARGO_PKG_VERSION` would mark every previously generated document
/// drifted on every unrelated `fslc` release, not only when a template did.
pub const DOCUMENT_RENDERER_VERSION: &str = "1.1.0";
pub const NORMATIVE_SCOPE: &str = "generated-claim-blocks-only";

/// The closed set of editable slot names `fslc document generate` emits and
/// `fslc document check` accepts. A slot name outside this set is drift, not
/// a new feature to recognize.
pub const SLOT_NAMES: &[&str] = &["background"];

const FRONTMATTER_DELIMITER: &str = "---";
const CLAIM_BEGIN_PREFIX: &str = "<!-- fsl:claim begin id=\"";
const CLAIM_END: &str = "<!-- fsl:claim end -->";
const SLOT_BEGIN_PREFIX: &str = "<!-- fsl:slot begin name=\"";
const SLOT_END: &str = "<!-- fsl:slot end -->";
const MARKER_PREFIX: &str = "<!-- fsl:";

/// Parsed `key: value` frontmatter, order-insensitive (each key is compared
/// individually by `fslc document check`; nothing depends on line order).
#[derive(Debug)]
pub struct Frontmatter {
    pub schema: String,
    pub view: String,
    pub lang: String,
    pub source: Option<String>,
    pub renderer: String,
    pub renderer_version: String,
    pub normative_scope: String,
    pub spec_digest: String,
    pub claim_set_digest: String,
    /// A plain (non-framed) `sha256:`-prefixed digest of the exact
    /// glossary sidecar file bytes (issue #330) used to generate this
    /// document, present only when a glossary was applied.
    pub glossary_digest: Option<String>,
    /// A plain `sha256:`-prefixed digest of the combined `--evidence`
    /// file set (issue #332), order-independent (each file's own digest is
    /// sorted before combining, so the same files in a different CLI order
    /// digest identically) — present only when at least one evidence file
    /// was supplied.
    pub evidence_digest: Option<String>,
}

#[derive(Debug)]
pub enum MarkerIssue {
    MissingFrontmatter,
    UnterminatedFrontmatter,
    MalformedFrontmatterLine(String),
    MissingFrontmatterKey(&'static str),
    UnknownFrontmatterKey(String),
    MalformedMarkerLine(String),
    UnclosedClaimBlock(String),
    UnclosedSlot(String),
    MarkerInsideSlot(String),
}

impl std::fmt::Display for MarkerIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFrontmatter => {
                write!(
                    f,
                    "document has no frontmatter (expected a leading '---' block)"
                )
            }
            Self::UnterminatedFrontmatter => write!(f, "frontmatter '---' block is never closed"),
            Self::MalformedFrontmatterLine(line) => {
                write!(f, "malformed frontmatter line: {line:?}")
            }
            Self::MissingFrontmatterKey(key) => write!(f, "frontmatter is missing '{key}'"),
            Self::UnknownFrontmatterKey(key) => write!(f, "frontmatter has an unknown key '{key}'"),
            Self::MalformedMarkerLine(line) => {
                write!(f, "malformed generated-block marker: {line:?}")
            }
            Self::UnclosedClaimBlock(id) => {
                write!(
                    f,
                    "claim block '{id}' has no matching '<!-- fsl:claim end -->'"
                )
            }
            Self::UnclosedSlot(name) => {
                write!(f, "slot '{name}' has no matching '<!-- fsl:slot end -->'")
            }
            Self::MarkerInsideSlot(name) => write!(
                f,
                "slot '{name}' contains a '<!-- fsl:' marker-like line, which is not allowed inside an editable slot"
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Residue(String),
    Claim {
        id: String,
        digest: String,
        body: String,
    },
    Slot {
        name: String,
        body: String,
    },
}

#[derive(Debug)]
pub struct ParsedDocument {
    pub frontmatter: Frontmatter,
    pub segments: Vec<Segment>,
}

/// Render the frontmatter block `fslc document generate` prepends to every
/// artifact. `source` is omitted when the RCIR claim set carries none.
#[must_use]
pub fn render_frontmatter(
    source: Option<&str>,
    locale: Locale,
    spec_digest: &str,
    claim_set_digest: &str,
    glossary_digest: Option<&str>,
    evidence_digest: Option<&str>,
) -> String {
    let mut lines = vec![
        FRONTMATTER_DELIMITER.to_owned(),
        format!("fsl_document_schema: {DOCUMENT_SCHEMA}"),
        "view: requirements".to_owned(),
        format!("lang: {}", locale.as_str()),
    ];
    if let Some(source) = source {
        lines.push(format!("source: {source}"));
    }
    lines.push(format!("renderer: {DOCUMENT_RENDERER}"));
    lines.push(format!("renderer_version: {DOCUMENT_RENDERER_VERSION}"));
    lines.push(format!("normative_scope: {NORMATIVE_SCOPE}"));
    lines.push(format!("spec_digest: {spec_digest}"));
    lines.push(format!("claim_set_digest: {claim_set_digest}"));
    if let Some(glossary_digest) = glossary_digest {
        lines.push(format!("glossary_digest: {glossary_digest}"));
    }
    if let Some(evidence_digest) = evidence_digest {
        lines.push(format!("evidence_digest: {evidence_digest}"));
    }
    lines.push(FRONTMATTER_DELIMITER.to_owned());
    lines.join("\n")
}

/// Wrap a claim's already-rendered body in `<!-- fsl:claim begin/end -->`
/// markers, digesting exactly the body text with
/// [`crate::document_digest::framed_text_digest`].
#[must_use]
pub fn wrap_claim_block(id: &str, body: &str) -> String {
    let digest = crate::document_digest::framed_text_digest(
        crate::document_digest::CLAIM_BLOCK_DIGEST_ALGORITHM,
        body,
    );
    format!("{CLAIM_BEGIN_PREFIX}{id}\" digest=\"{digest}\" -->\n{body}\n{CLAIM_END}")
}

/// Wrap fixed placeholder content in `<!-- fsl:slot begin/end -->` markers
/// for the one editable, non-normative slot v1 defines (`background`).
#[must_use]
pub fn wrap_slot(name: &str, body: &str) -> String {
    format!("{SLOT_BEGIN_PREFIX}{name}\" normative=\"false\" -->\n{body}\n{SLOT_END}")
}

/// Parse only the leading frontmatter, returning it plus the byte offset the
/// body starts at. `pub(crate)` so `document_check` can distinguish a
/// frontmatter-level failure (hard error) from a body marker-grammar
/// failure (a `marker_malformed` drift reason) — [`parse_generated_document`]
/// does not preserve that distinction.
pub(crate) fn parse_frontmatter(text: &str) -> Result<(Frontmatter, usize), MarkerIssue> {
    let mut lines = text.split('\n');
    match lines.next() {
        Some(FRONTMATTER_DELIMITER) => {}
        _ => return Err(MarkerIssue::MissingFrontmatter),
    }
    let mut consumed_bytes = FRONTMATTER_DELIMITER.len() + 1;
    let mut raw = BTreeMap::new();
    let mut closed = false;
    for line in lines {
        consumed_bytes += line.len() + 1;
        if line == FRONTMATTER_DELIMITER {
            closed = true;
            break;
        }
        let (key, value) = line
            .split_once(": ")
            .ok_or_else(|| MarkerIssue::MalformedFrontmatterLine(line.to_owned()))?;
        raw.insert(key.to_owned(), value.to_owned());
    }
    if !closed {
        return Err(MarkerIssue::UnterminatedFrontmatter);
    }
    let required = |key: &'static str| -> Result<String, MarkerIssue> {
        raw.get(key)
            .cloned()
            .ok_or(MarkerIssue::MissingFrontmatterKey(key))
    };
    let source = raw.get("source").cloned();
    let glossary_digest = raw.get("glossary_digest").cloned();
    let evidence_digest = raw.get("evidence_digest").cloned();
    let mut known: std::collections::BTreeSet<&str> = [
        "fsl_document_schema",
        "view",
        "lang",
        "source",
        "renderer",
        "renderer_version",
        "normative_scope",
        "spec_digest",
        "claim_set_digest",
        "glossary_digest",
        "evidence_digest",
    ]
    .into_iter()
    .collect();
    for key in raw.keys() {
        if !known.remove(key.as_str()) {
            return Err(MarkerIssue::UnknownFrontmatterKey(key.clone()));
        }
    }
    let frontmatter = Frontmatter {
        schema: required("fsl_document_schema")?,
        view: required("view")?,
        lang: required("lang")?,
        source,
        renderer: required("renderer")?,
        renderer_version: required("renderer_version")?,
        normative_scope: required("normative_scope")?,
        spec_digest: required("spec_digest")?,
        claim_set_digest: required("claim_set_digest")?,
        glossary_digest,
        evidence_digest,
    };
    Ok((frontmatter, consumed_bytes))
}

/// Parse only an artifact's frontmatter (issue #329), for a caller (`fslc
/// document check`) that needs `lang`/`source`/`view` before it can build
/// the fresh re-render [`crate::document_check::check_requirements_document`]
/// compares against. The byte offset the body starts at is only needed
/// within this crate, so this wrapper drops it.
///
/// # Errors
///
/// Returns [`MarkerIssue`] when the frontmatter is missing or malformed.
pub fn parse_frontmatter_only(text: &str) -> Result<Frontmatter, MarkerIssue> {
    parse_frontmatter(text).map(|(frontmatter, _)| frontmatter)
}

fn parse_claim_begin(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(CLAIM_BEGIN_PREFIX)?;
    let (id, rest) = rest.split_once("\" digest=\"")?;
    let digest = rest.strip_suffix("\" -->")?;
    Some((id.to_owned(), digest.to_owned()))
}

fn parse_slot_begin(line: &str) -> Option<String> {
    let rest = line.strip_prefix(SLOT_BEGIN_PREFIX)?;
    let name = rest.strip_suffix("\" normative=\"false\" -->")?;
    Some(name.to_owned())
}

pub(crate) fn parse_body(body: &str) -> Result<Vec<Segment>, MarkerIssue> {
    let mut segments = Vec::new();
    let mut residue: Vec<&str> = Vec::new();
    let mut lines = body.split('\n').peekable();
    let flush = |segments: &mut Vec<Segment>, residue: &mut Vec<&str>| {
        if !residue.is_empty() {
            segments.push(Segment::Residue(residue.join("\n")));
            residue.clear();
        }
    };
    while let Some(line) = lines.next() {
        if let Some((id, digest)) = parse_claim_begin(line) {
            flush(&mut segments, &mut residue);
            let mut body_lines = Vec::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner == CLAIM_END {
                    closed = true;
                    break;
                }
                body_lines.push(inner);
            }
            if !closed {
                return Err(MarkerIssue::UnclosedClaimBlock(id));
            }
            segments.push(Segment::Claim {
                id,
                digest,
                body: body_lines.join("\n"),
            });
        } else if let Some(name) = parse_slot_begin(line) {
            flush(&mut segments, &mut residue);
            let mut body_lines = Vec::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner == SLOT_END {
                    closed = true;
                    break;
                }
                if inner.trim_start().starts_with(MARKER_PREFIX) {
                    return Err(MarkerIssue::MarkerInsideSlot(name));
                }
                body_lines.push(inner);
            }
            if !closed {
                return Err(MarkerIssue::UnclosedSlot(name));
            }
            segments.push(Segment::Slot {
                name,
                body: body_lines.join("\n"),
            });
        } else if line.trim_start().starts_with(MARKER_PREFIX) {
            return Err(MarkerIssue::MalformedMarkerLine(line.to_owned()));
        } else {
            residue.push(line);
        }
    }
    flush(&mut segments, &mut residue);
    Ok(segments)
}

/// Parse a full generated-document artifact: leading frontmatter, then the
/// ordered sequence of residue/claim/slot segments.
///
/// # Errors
///
/// Returns [`MarkerIssue`] when the frontmatter or marker grammar is
/// violated. Does not validate frontmatter *values* (schema/view/lang
/// support, digest agreement) — that is `fslc document check`'s job, since
/// this module only knows the artifact's own grammar.
pub fn parse_generated_document(text: &str) -> Result<ParsedDocument, MarkerIssue> {
    let (frontmatter, consumed) = parse_frontmatter(text)?;
    let body = text.get(consumed..).unwrap_or("");
    let segments = parse_body(body)?;
    Ok(ParsedDocument {
        frontmatter,
        segments,
    })
}
