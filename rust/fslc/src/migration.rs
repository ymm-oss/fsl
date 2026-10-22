// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{
    Annotation, CanonicalRewriteKind, CorrespondenceOrigin, Edition, LosslessKind, ParseOptions,
    RequirementActionItem, RequirementsItem, SourceEdit, Span, SurfaceDocument, TokenKind,
    canonical_rewrites_with, lossless_document_with, source_position, source_span,
};
use std::path::Path;

use serde_json::{Map, Value, json};

mod process_form;

pub use process_form::{
    ConvertedEvent, ConvertedProcess, LEGACY_PROCESS_FORM, MIGRATE_MANUAL_REQUIRED, ManualRequired,
    ProcessFormConversion, ProcessFormOutcome, Site, convert_process_forms,
    ignore_derived_terminal, plan_process_conversion, process_form_contract,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IdKind {
    Requirement,
    Acceptance,
    Forbidden,
    Policy,
    Goal,
    Control,
    Model,
    Assumption,
}

impl IdKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requirement => "requirement",
            Self::Acceptance => "acceptance",
            Self::Forbidden => "forbidden",
            Self::Policy => "policy",
            Self::Goal => "goal",
            Self::Control => "control",
            Self::Model => "model",
            Self::Assumption => "assumption",
        }
    }

    /// Parse a manifest key into a semantic ID kind.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is not part of the closed policy schema.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "requirement" => Ok(Self::Requirement),
            "acceptance" => Ok(Self::Acceptance),
            "forbidden" => Ok(Self::Forbidden),
            "policy" => Ok(Self::Policy),
            "goal" => Ok(Self::Goal),
            "control" => Ok(Self::Control),
            "model" => Ok(Self::Model),
            "assumption" => Ok(Self::Assumption),
            _ => Err(format!("unknown ID policy kind '{value}'")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdPolicy {
    patterns: std::collections::BTreeMap<IdKind, Vec<String>>,
}

impl Default for IdPolicy {
    fn default() -> Self {
        Self {
            patterns: std::collections::BTreeMap::from([
                (
                    IdKind::Requirement,
                    [
                        "REQ-{scope}-{number:3}",
                        "NFR-{scope}-{number:3}",
                        "INV-{scope}-{number:3}",
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                ),
                (IdKind::Acceptance, vec!["AC-{scope}-{number:3}".to_owned()]),
                (IdKind::Forbidden, vec!["FB-{scope}-{number:3}".to_owned()]),
                (IdKind::Policy, vec!["POL-{scope}-{number:3}".to_owned()]),
                (IdKind::Goal, vec!["GOAL-{scope}-{number:3}".to_owned()]),
                (IdKind::Control, vec!["CTRL-{scope}-{number:3}".to_owned()]),
                (IdKind::Model, vec!["MODEL-{scope}-{number:3}".to_owned()]),
                (
                    IdKind::Assumption,
                    vec!["ASSUME-{scope}-{number:3}".to_owned()],
                ),
            ]),
        }
    }
}

impl IdPolicy {
    /// Replace the accepted templates for one semantic ID kind.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty template list or an unsupported placeholder.
    pub fn set_patterns(&mut self, kind: IdKind, patterns: Vec<String>) -> Result<(), String> {
        if patterns.is_empty() {
            return Err(format!(
                "ID policy '{}' must define at least one template",
                kind.as_str()
            ));
        }
        for pattern in &patterns {
            parse_id_template(pattern)?;
            if matches!(kind, IdKind::Model | IdKind::Assumption)
                && template_literal_prefix(pattern).is_none()
            {
                return Err(format!(
                    "ID policy '{}' template '{pattern}' must begin with a literal prefix",
                    kind.as_str()
                ));
            }
        }
        self.patterns.insert(kind, patterns);
        Ok(())
    }

    /// Validate relationships between independently configured ID kinds.
    ///
    /// # Errors
    ///
    /// Returns an error when model and assumption prefixes overlap and a typed
    /// requirement annotation could not be classified deterministically.
    pub fn validate(&self) -> Result<(), String> {
        let requirement_prefixes = self
            .patterns(IdKind::Requirement)
            .iter()
            .map(|pattern| template_literal_prefix(pattern))
            .collect::<Vec<_>>();
        for model in self.patterns(IdKind::Model) {
            let Some(model_prefix) = template_literal_prefix(model) else {
                return Err(format!(
                    "ID policy model template '{model}' must begin with a literal prefix"
                ));
            };
            validate_special_prefix("model", model_prefix, &requirement_prefixes)?;
            for assumption in self.patterns(IdKind::Assumption) {
                let Some(assumption_prefix) = template_literal_prefix(assumption) else {
                    return Err(format!(
                        "ID policy assumption template '{assumption}' must begin with a literal prefix"
                    ));
                };
                validate_special_prefix("assumption", assumption_prefix, &requirement_prefixes)?;
                if model_prefix.starts_with(assumption_prefix)
                    || assumption_prefix.starts_with(model_prefix)
                {
                    return Err(format!(
                        "ID policy model prefix '{model_prefix}' overlaps assumption prefix '{assumption_prefix}'"
                    ));
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn patterns(&self, kind: IdKind) -> &[String] {
        self.patterns
            .get(&kind)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    #[must_use]
    pub fn json(&self) -> Value {
        let patterns = [
            IdKind::Requirement,
            IdKind::Acceptance,
            IdKind::Forbidden,
            IdKind::Policy,
            IdKind::Goal,
            IdKind::Control,
            IdKind::Model,
            IdKind::Assumption,
        ]
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), json!(self.patterns(kind))))
        .collect::<serde_json::Map<_, _>>();
        Value::Object(patterns)
    }

    fn accepts(&self, kind: IdKind, id: &str) -> bool {
        self.patterns(kind)
            .iter()
            .any(|pattern| id_template_matches(pattern, id))
    }

    fn tag_kind(&self, id: &str) -> IdKind {
        if self.patterns(IdKind::Model).iter().any(|pattern| {
            template_literal_prefix(pattern).is_some_and(|prefix| id.starts_with(prefix))
        }) {
            IdKind::Model
        } else if self.patterns(IdKind::Assumption).iter().any(|pattern| {
            template_literal_prefix(pattern).is_some_and(|prefix| id.starts_with(prefix))
        }) {
            IdKind::Assumption
        } else {
            IdKind::Requirement
        }
    }
}

fn validate_special_prefix(
    kind: &str,
    prefix: &str,
    requirement_prefixes: &[Option<&str>],
) -> Result<(), String> {
    for requirement_prefix in requirement_prefixes {
        if requirement_prefix.is_none_or(|requirement_prefix| {
            prefix.starts_with(requirement_prefix) || requirement_prefix.starts_with(prefix)
        }) {
            return Err(format!(
                "ID policy {kind} prefix '{prefix}' overlaps a requirement template"
            ));
        }
    }
    Ok(())
}

fn template_literal_prefix(template: &str) -> Option<&str> {
    let end = template.find('{').unwrap_or(template.len());
    (end > 0).then_some(&template[..end])
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum IdTemplatePart {
    Literal(String),
    Scope,
    Number(Option<usize>),
}

fn parse_id_template(template: &str) -> Result<Vec<IdTemplatePart>, String> {
    if template.is_empty() {
        return Err("ID policy template must not be empty".to_owned());
    }
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            parts.push(IdTemplatePart::Literal(rest[..open].to_owned()));
        }
        let after_open = &rest[open + 1..];
        let close = after_open.find('}').ok_or_else(|| {
            format!("ID policy template '{template}' has an unclosed placeholder")
        })?;
        let placeholder = &after_open[..close];
        let part = if placeholder == "scope" {
            IdTemplatePart::Scope
        } else if placeholder == "number" {
            IdTemplatePart::Number(None)
        } else if let Some(width) = placeholder.strip_prefix("number:") {
            let width = width.parse::<usize>().map_err(|_| {
                format!("ID policy template '{template}' has an invalid number width")
            })?;
            if width == 0 {
                return Err(format!(
                    "ID policy template '{template}' requires a positive number width"
                ));
            }
            IdTemplatePart::Number(Some(width))
        } else {
            return Err(format!(
                "ID policy template '{template}' uses unsupported placeholder '{{{placeholder}}}'"
            ));
        };
        parts.push(part);
        rest = &after_open[close + 1..];
    }
    if rest.contains('}') {
        return Err(format!(
            "ID policy template '{template}' has an unmatched closing brace"
        ));
    }
    if !rest.is_empty() {
        parts.push(IdTemplatePart::Literal(rest.to_owned()));
    }
    Ok(parts)
}

fn id_template_matches(template: &str, id: &str) -> bool {
    parse_id_template(template).is_ok_and(|parts| match_id_parts(&parts, id))
}

fn match_id_parts(parts: &[IdTemplatePart], value: &str) -> bool {
    let Some((part, rest)) = parts.split_first() else {
        return value.is_empty();
    };
    match part {
        IdTemplatePart::Literal(literal) => value
            .strip_prefix(literal)
            .is_some_and(|value| match_id_parts(rest, value)),
        IdTemplatePart::Scope | IdTemplatePart::Number(_) => (1..=value.len())
            .filter(|offset| value.is_char_boundary(*offset))
            .any(|offset| {
                placeholder_matches(part, &value[..offset])
                    && match_id_parts(rest, &value[offset..])
            }),
    }
}

fn placeholder_matches(part: &IdTemplatePart, value: &str) -> bool {
    match part {
        IdTemplatePart::Literal(_) => false,
        IdTemplatePart::Scope => value.split('-').all(|segment| {
            let mut characters = segment.chars();
            characters
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
                && characters
                    .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        }),
        IdTemplatePart::Number(width) => {
            !value.is_empty()
                && width.is_none_or(|width| value.len() == width)
                && value.bytes().all(|byte| byte.is_ascii_digit())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Taxonomy {
    Deprecated,
    NonCanonical,
    AmbiguousIntent,
    UnsupportedInEdition,
}

impl Taxonomy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Deprecated => "deprecated",
            Self::NonCanonical => "non_canonical",
            Self::AmbiguousIntent => "ambiguous_intent",
            Self::UnsupportedInEdition => "unsupported_in_edition",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationDiagnostic {
    pub code: &'static str,
    pub taxonomy: Taxonomy,
    pub message: String,
    pub span: Span,
    pub canonical_replacement: String,
    pub machine_applicable: bool,
    /// Why a [`MIGRATE_MANUAL_REQUIRED`] finding cannot be rewritten, as a
    /// stable word (`None` for every other finding).
    pub reason: Option<&'static str>,
    pub edits: Vec<SourceEdit>,
}

impl MigrationDiagnostic {
    #[must_use]
    pub fn json(&self, path: &str, edition: Edition) -> Value {
        let mut value = json!({
            "code": self.code,
            "kind": self.code,
            "taxonomy": self.taxonomy.as_str(),
            "severity": if self.code == fsl_syntax::MISSING_EDITION_HEADER {
                "info"
            } else if [
                fsl_syntax::MIXED_NAMES,
                fsl_syntax::FULLWIDTH_SPACE,
                fsl_syntax::NON_NFC_IDENTIFIER,
            ]
            .contains(&self.code)
            {
                "warning"
            } else if edition == Edition::Fsl5 {
                "error"
            } else {
                "warning"
            },
            "edition": edition.as_str(),
            "message": self.message,
            "loc": {
                "file": path,
                "line": self.span.start.line,
                "column": self.span.start.column,
                "end_line": self.span.end.line,
                "end_column": self.span.end.column,
            },
            "canonical_replacement": self.canonical_replacement,
            "machine_applicable": self.machine_applicable,
            "edits": self.edits.iter().map(|edit| json!({
                "start": edit.span.start.offset,
                "end": edit.span.end.offset,
                "replacement": edit.replacement,
            })).collect::<Vec<_>>(),
        });
        if let Some(reason) = self.reason {
            value["reason"] = json!(reason);
        }
        if self.edits.len() == 1 {
            value["suggestion"] = json!({
                "kind":"replace",
                "replacement":self.edits[0].replacement,
                "span":{
                    "start":self.edits[0].span.start.offset,
                    "end":self.edits[0].span.end.offset,
                },
                "machine_applicable":self.machine_applicable,
            });
        }
        value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    pub diagnostics: Vec<MigrationDiagnostic>,
    pub migrated_source: Option<String>,
}

impl MigrationPlan {
    /// A finding the planner recognizes but cannot machine-apply, other
    /// than [`MIGRATE_MANUAL_REQUIRED`].
    #[must_use]
    pub fn refused(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            !diagnostic.machine_applicable && diagnostic.code != MIGRATE_MANUAL_REQUIRED
        })
    }

    /// A process form the next edition cannot take that the migrator cannot
    /// rewrite ([`MIGRATE_MANUAL_REQUIRED`]).
    #[must_use]
    pub fn manual_required(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == MIGRATE_MANUAL_REQUIRED)
    }
}

/// Plan all edition migrations against one immutable source snapshot.
///
/// # Errors
///
/// Returns a parse or edit error. Unsupported-but-recognizable legacy syntax
/// is represented as a non-machine-applicable diagnostic instead.
#[allow(clippy::too_many_lines)]
pub fn plan_migration(
    source: &str,
    path: &str,
    edition: Edition,
    options: &ParseOptions,
) -> Result<MigrationPlan, String> {
    let unsupported = unsupported_double_ampersands(source);
    if !unsupported.is_empty() {
        return Ok(MigrationPlan {
            diagnostics: unsupported,
            migrated_source: None,
        });
    }

    let document = fsl_syntax::parse_surface_document_with(source, options)
        .map_err(|error| error.to_string())?;
    if matches!(document, SurfaceDocument::Agent(_)) {
        return Ok(MigrationPlan {
            diagnostics: Vec::new(),
            migrated_source: None,
        });
    }
    let mut diagnostics = Vec::new();
    match canonical_rewrites_with(source, options) {
        Ok(rewrites) => diagnostics.extend(rewrites.into_iter().map(|rewrite| {
            let (code, taxonomy, message) = match rewrite.kind {
                CanonicalRewriteKind::DomainEnum => (
                    "deprecated_domain_enum_union",
                    Taxonomy::Deprecated,
                    "legacy domain enum union syntax is deprecated".to_owned(),
                ),
                CanonicalRewriteKind::LogicalOperator => (
                    "legacy_logical_operator",
                    Taxonomy::NonCanonical,
                    "legacy logical operator spelling is non-canonical".to_owned(),
                ),
                CanonicalRewriteKind::Quantifier => (
                    "legacy_quantifier_colon",
                    Taxonomy::Deprecated,
                    "legacy colon quantifier syntax is deprecated".to_owned(),
                ),
            };
            MigrationDiagnostic {
                code,
                taxonomy,
                message,
                span: rewrite.span,
                canonical_replacement: rewrite.canonical_replacement,
                machine_applicable: true,
                reason: None,
                edits: rewrite.edits,
            }
        })),
        Err(fsl_syntax::FormatError::Unsafe {
            kind,
            message,
            span,
        }) => {
            // Exhaustive, so a new refusal kind states its code here rather
            // than inheriting one from a word in its message (issue 1167).
            let (code, canonical_replacement) = match kind {
                fsl_syntax::UnsafeFormatKind::DomainEnumInteriorComment => (
                    "deprecated_domain_enum_union",
                    "move the interior comment, then use `enum Name { ... }`".to_owned(),
                ),
                fsl_syntax::UnsafeFormatKind::UnbracedQuantifierEnd
                | fsl_syntax::UnsafeFormatKind::QuantifierWithoutBody => (
                    "legacy_quantifier_colon",
                    "add an explicit brace-delimited quantifier body".to_owned(),
                ),
                fsl_syntax::UnsafeFormatKind::AgentBody
                | fsl_syntax::UnsafeFormatKind::EditOutsideUtf8Boundary
                | fsl_syntax::UnsafeFormatKind::OverlappingEdits
                | fsl_syntax::UnsafeFormatKind::PhraseInteriorComment => (
                    "unsafe_source_attachment",
                    "move the comment, then rerun migrate".to_owned(),
                ),
            };
            diagnostics.push(MigrationDiagnostic {
                code,
                taxonomy: Taxonomy::AmbiguousIntent,
                message,
                span,
                canonical_replacement,
                machine_applicable: false,
                reason: None,
                edits: Vec::new(),
            });
        }
        Err(error) => return Err(error.to_string()),
    }
    diagnostics.extend(metadata_diagnostics(source, *options));
    diagnostics.extend(default_diagnostics(source, path, *options));
    diagnostics.extend(action_map_diagnostics(source, &document, *options));

    if diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.machine_applicable)
    {
        return Ok(MigrationPlan {
            diagnostics,
            migrated_source: None,
        });
    }
    if diagnostics.is_empty() {
        return Ok(MigrationPlan {
            diagnostics,
            migrated_source: None,
        });
    }
    let edits = diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.edits.clone())
        .collect::<Vec<_>>();
    let rewritten =
        fsl_syntax::apply_source_edits(source, edits).map_err(|error| error.to_string())?;
    let formatted = fsl_syntax::format_source_with(&rewritten, edition, options)
        .map_err(|error| error.to_string())?;
    Ok(MigrationPlan {
        migrated_source: (formatted != source).then_some(formatted),
        diagnostics,
    })
}

/// The rendered `edition_conflict` rejection when `source`'s edition header
/// and the requested `--edition` differ.
///
/// # Errors
///
/// Returns the parse-error envelope (built on `envelope`) and exit status 2.
pub fn edition_conflict(
    source: &str,
    requested: Option<Edition>,
    options: &ParseOptions,
    envelope: Map<String, Value>,
) -> Result<(), (Value, i32)> {
    match fsl_syntax::document_edition(source, requested, Edition::Fsl4, options) {
        Err(error) if error.code() == fsl_syntax::EDITION_CONFLICT => Err((
            crate::frontend_output::render_surface_parse_error(envelope, &error),
            2,
        )),
        _ => Ok(()),
    }
}

/// Apply the edition `source` is read in to a finished `check` / `verify`
/// result: in the next edition every migration finding is an error, otherwise
/// the deprecated domain enum union (and, with `implicit_warnings`, an
/// omitted initial value) is a warning.
///
/// `requested` is the `--edition` value (`None` = `current`). The document is
/// read in its header's edition when it has one; an unreadable header already
/// failed the document's own parse, so the request decides then. A header
/// adds `fsl_edition`; without one the envelope keeps its published shape.
///
/// # Panics
///
/// Panics if `output` already carries a `warnings` member that is not an
/// array.
pub fn apply_edition(
    (mut output, status): (Value, i32),
    source: &str,
    path: &str,
    requested: Option<Edition>,
    options: &ParseOptions,
    envelope: &dyn Fn() -> Map<String, Value>,
    implicit_warnings: bool,
) -> (Value, i32) {
    if let Err(conflict) = edition_conflict(source, requested, options, envelope()) {
        return conflict;
    }
    let declared = fsl_syntax::edition_header(source, options)
        .ok()
        .flatten()
        .map(|header| header.edition);
    let edition = declared.or(requested).unwrap_or_default();
    let next = edition == Edition::Fsl5;
    let Ok(plan) = plan_migration(source, path, edition, options) else {
        return (with_declared_edition(output, declared), status);
    };
    let mut additions = plan
        .diagnostics
        .iter()
        .filter(|finding| next || finding.code == "deprecated_domain_enum_union")
        .map(|finding| finding.json(path, edition))
        .collect::<Vec<_>>();
    if !next && implicit_warnings {
        additions.extend(crate::frontend_output::implicit_initial_value_warnings(
            source, path, options,
        ));
    }
    if next && !additions.is_empty() {
        let kind = if additions.iter().all(|finding| {
            finding.get("code").and_then(Value::as_str) == Some("deprecated_domain_enum_union")
        }) {
            "deprecated_domain_enum_union"
        } else {
            "unsupported_in_edition"
        };
        let mut error = envelope();
        error.insert("result".to_owned(), json!("error"));
        error.insert("kind".to_owned(), json!(kind));
        error.insert("edition".to_owned(), json!(edition.as_str()));
        error.insert("findings".to_owned(), Value::Array(additions));
        return (with_declared_edition(Value::Object(error), declared), 2);
    }
    if !additions.is_empty()
        && let Value::Object(envelope) = &mut output
    {
        envelope
            .entry("warnings")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("warnings is an array")
            .extend(additions);
    }
    if next && let Value::Object(envelope) = &mut output {
        envelope.insert("edition".to_owned(), json!(edition.as_str()));
    }
    (with_declared_edition(output, declared), status)
}

/// Record the edition a document's header declares as `fsl_edition`. A
/// header is read only when the options enable it, so without them (or
/// without a header) the envelope is unchanged.
fn with_declared_edition(mut output: Value, declared: Option<Edition>) -> Value {
    if let (Some(edition), Value::Object(envelope)) = (declared, &mut output) {
        envelope.insert("fsl_edition".to_owned(), json!(edition.number()));
    }
    output
}

/// The `missing_edition_header` finding of a document read for `edition`
/// without an edition header: one edit inserting the header line before the
/// first significant line. `None` unless the options read headers, the
/// edition is the next one, and the document has no header.
#[must_use]
pub fn missing_edition_header(
    source: &str,
    edition: Edition,
    options: &ParseOptions,
) -> Option<MigrationDiagnostic> {
    if edition != Edition::Fsl5
        || !options
            .unstable()
            .contains(fsl_syntax::UnstableFeature::Fsl5)
        || !matches!(fsl_syntax::edition_header(source, options), Ok(None))
    {
        return None;
    }
    let first = fsl_syntax::first_significant_offset(source);
    let line_start = source[..first].rfind('\n').map_or_else(
        || first.min(if source.starts_with('\u{feff}') { 3 } else { 0 }),
        |newline| newline + 1,
    );
    let span = source_span(source, line_start, line_start);
    let header = format!("fsl {}", edition.number());
    Some(MigrationDiagnostic {
        code: fsl_syntax::MISSING_EDITION_HEADER,
        taxonomy: Taxonomy::NonCanonical,
        message: format!("the document has no edition header; add '{header}'"),
        span,
        canonical_replacement: header.clone(),
        machine_applicable: true,
        reason: None,
        edits: vec![SourceEdit {
            span,
            replacement: format!("{header}\n"),
        }],
    })
}

fn unsupported_double_ampersands(source: &str) -> Vec<MigrationDiagnostic> {
    let mut diagnostics = raw_operator_offsets(source, "&&")
        .into_iter()
        .map(|offset| {
            let span = source_span(source, offset, offset + 2);
            MigrationDiagnostic {
                code: "unsupported_double_ampersand",
                taxonomy: Taxonomy::UnsupportedInEdition,
                message:
                    "`&&` is not valid FSL, so pre-migration semantic equivalence cannot be proven"
                        .to_owned(),
                span,
                canonical_replacement: "and".to_owned(),
                machine_applicable: false,
                reason: None,
                edits: vec![SourceEdit {
                    span,
                    replacement: "and".to_owned(),
                }],
            }
        })
        .collect::<Vec<_>>();
    if !diagnostics.is_empty() {
        diagnostics.extend(raw_operator_offsets(source, "||").into_iter().map(|offset| {
            let span = source_span(source, offset, offset + 2);
            MigrationDiagnostic {
                code: "legacy_logical_operator",
                taxonomy: Taxonomy::NonCanonical,
                message: "`||` is non-canonical, but no edit is applied while the source also contains invalid `&&`"
                    .to_owned(),
                span,
                canonical_replacement: "or".to_owned(),
                machine_applicable: false,
                reason: None,
                edits: vec![SourceEdit {
                    span,
                    replacement: "or".to_owned(),
                }],
            }
        }));
        diagnostics.sort_by_key(|diagnostic| diagnostic.span.start.offset);
    }
    diagnostics
}

fn raw_operator_offsets(source: &str, operator: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let needle = operator.as_bytes();
    let mut offsets = Vec::new();
    let mut index = 0;
    let mut in_string = false;
    let mut in_comment = false;
    while index < bytes.len() {
        if in_comment {
            if bytes[index] == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if in_string {
            if bytes[index] == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            in_comment = true;
            index += 2;
            continue;
        }
        if bytes[index..].starts_with(needle) {
            offsets.push(index);
            index += needle.len();
        } else {
            index += 1;
        }
    }
    offsets
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IdOccurrence {
    kind: IdKind,
    id: String,
    span: Span,
    is_tag: bool,
}

/// `lint --strict-names`: each place a canonical name is written as itself
/// in a document that also writes it as its glossary alias. Off by default,
/// because mixing is how a document moves from one spelling to the other.
#[must_use]
pub fn mixed_names_diagnostics(source: &str, options: &ParseOptions) -> Vec<MigrationDiagnostic> {
    let Ok(fsl_syntax::SurfaceDocument::Requirements(requirements)) =
        fsl_syntax::parse_surface_document_with(source, options)
    else {
        return Vec::new();
    };
    let Some(glossary) = requirements.items.iter().find_map(|item| match item {
        fsl_syntax::RequirementsItem::Glossary(glossary) => Some(glossary),
        _ => None,
    }) else {
        return Vec::new();
    };
    let via_alias = glossary
        .uses
        .iter()
        .filter(|name| name.via_alias)
        .map(|name| name.canonical.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    glossary
        .uses
        .iter()
        .filter(|name| !name.via_alias && via_alias.contains(name.canonical.as_str()))
        .filter_map(|name| {
            let alias = glossary
                .entries
                .iter()
                .find(|entry| entry.canonical.name == name.canonical)?
                .alias
                .clone();
            Some(MigrationDiagnostic {
                code: fsl_syntax::MIXED_NAMES,
                taxonomy: Taxonomy::NonCanonical,
                message: format!(
                    "'{}' is written both as itself and as its alias '{alias}'; write one spelling",
                    name.canonical
                ),
                span: name.span,
                canonical_replacement: alias,
                machine_applicable: false,
                reason: None,
                edits: Vec::new(),
            })
        })
        .collect()
}

/// The lexer's warnings (`fullwidth_space`, `non_nfc_identifier`) as lint
/// findings: each carries the text that reads the same without it, which
/// replaces the span without changing the document's meaning. Empty outside
/// the next edition's identifier rule.
#[must_use]
pub fn lexical_warning_diagnostics(
    source: &str,
    options: &ParseOptions,
) -> Vec<MigrationDiagnostic> {
    fsl_syntax::lexical_warnings(source, options)
        .into_iter()
        .map(|warning| MigrationDiagnostic {
            code: warning.code,
            taxonomy: Taxonomy::NonCanonical,
            message: warning.message,
            span: warning.span,
            canonical_replacement: warning.replacement.clone(),
            machine_applicable: true,
            reason: None,
            edits: vec![SourceEdit {
                span: warning.span,
                replacement: warning.replacement,
            }],
        })
        .collect()
}

/// Report project-policy violations without changing source identity.
///
#[must_use]
pub fn id_policy_diagnostics(
    source: &str,
    policy: &IdPolicy,
    options: &ParseOptions,
) -> Vec<MigrationDiagnostic> {
    let mut diagnostics = id_occurrences(source, *options)
        .into_iter()
        .map(|mut occurrence| {
            if occurrence.is_tag {
                occurrence.kind = policy.tag_kind(&occurrence.id);
            }
            occurrence
        })
        .filter(|occurrence| !policy.accepts(occurrence.kind, &occurrence.id))
        .map(|occurrence| {
            let expected = policy.patterns(occurrence.kind).join(" or ");
            MigrationDiagnostic {
                code: "non_canonical_id",
                taxonomy: Taxonomy::NonCanonical,
                message: format!(
                    "{} ID '{}' does not match the active ID policy",
                    occurrence.kind.as_str(),
                    occurrence.id
                ),
                span: occurrence.span,
                canonical_replacement: expected,
                machine_applicable: false,
                reason: None,
                edits: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by_key(|diagnostic| diagnostic.span.start.offset);
    diagnostics.dedup_by(|left, right| left.span == right.span && left.message == right.message);
    diagnostics
}

fn id_occurrences(source: &str, options: ParseOptions) -> Vec<IdOccurrence> {
    let Ok(tokens) = fsl_syntax::lex_with(source, &options) else {
        // Edition diagnostics own unsupported-token reporting. Do not replace
        // those findings with a secondary ID-policy failure.
        return Vec::new();
    };
    let mut occurrences = Vec::new();
    let document = fsl_syntax::dialect_keyword_with(source, &options).ok();
    let next_requirements = document == Some("requirements")
        && matches!(
            fsl_syntax::edition_header(source, &options),
            Ok(Some(header)) if header.edition == Edition::Fsl5
        );
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::Ident(keyword) = &token.kind else {
            continue;
        };
        if next_requirements && let Some(kind) = next_requirements_id_kind(keyword) {
            if let Some((id, span, next)) = requirement_id_at(source, &tokens, index + 1)
                && matches!(
                    tokens.get(next).map(|token| &token.kind),
                    Some(TokenKind::String(_))
                )
            {
                occurrences.push(IdOccurrence {
                    kind,
                    id,
                    span,
                    is_tag: false,
                });
            }
            continue;
        }
        if keyword == "requirement"
            && index
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
                .is_some_and(
                    |token| matches!(&token.kind, TokenKind::Symbol(value) if value == "@"),
                )
            && tokens.get(index + 1).is_some_and(
                |token| matches!(&token.kind, TokenKind::Symbol(value) if value == "("),
            )
            && let Some(fsl_syntax::Token {
                kind: TokenKind::String(id),
                span,
            }) = tokens.get(index + 2)
        {
            occurrences.push(IdOccurrence {
                kind: IdKind::Requirement,
                id: id.clone(),
                span: *span,
                is_tag: true,
            });
            continue;
        }
        if keyword == "satisfies" {
            extend_id_list(
                source,
                &tokens,
                index + 1,
                IdKind::Control,
                &mut occurrences,
            );
            continue;
        }
        if document == Some("governance")
            && governance_reference_at(source, &tokens, index, keyword, &mut occurrences)
        {
            continue;
        }
        let kind = match keyword.as_str() {
            "requirement" | "covers" => Some(IdKind::Requirement),
            "acceptance" => Some(IdKind::Acceptance),
            "forbidden" => Some(IdKind::Forbidden),
            "policy" => Some(IdKind::Policy),
            "goal" => Some(IdKind::Goal),
            "control" => Some(IdKind::Control),
            _ => None,
        };
        let Some(kind) = kind else {
            continue;
        };
        if let Some((id, span, next)) = requirement_id_at(source, &tokens, index + 1)
            && matches!(
                tokens.get(next).map(|token| &token.kind),
                Some(TokenKind::String(_))
            )
        {
            occurrences.push(IdOccurrence {
                kind,
                id,
                span,
                is_tag: false,
            });
        }
    }
    occurrences.extend(legacy_requirement_ids(source, options));
    occurrences
}

/// The kind of the ID a declaration of a next-edition requirements document
/// starts with, `keyword ID "text"`, in either spelling: a `rule`, a
/// decision table and a row's or an event's `covers` declare a requirement, a
/// `scenario` an acceptance trace, a `forbidden` a forbidden one, and `goal`
/// and `policy` their own kinds.
fn next_requirements_id_kind(keyword: &str) -> Option<IdKind> {
    use fsl_syntax::keywords::Kw;
    [
        (Kw::Rule, IdKind::Requirement),
        (Kw::Decide, IdKind::Requirement),
        (Kw::Covers, IdKind::Requirement),
        (Kw::Scenario, IdKind::Acceptance),
        (Kw::Forbidden, IdKind::Forbidden),
        (Kw::Goal, IdKind::Goal),
        (Kw::Policy, IdKind::Policy),
    ]
    .into_iter()
    .find(|(kw, _)| keyword == kw.english() || keyword == kw.japanese())
    .map(|(_, kind)| kind)
}

fn governance_reference_at(
    source: &str,
    tokens: &[fsl_syntax::Token],
    index: usize,
    keyword: &str,
    occurrences: &mut Vec<IdOccurrence>,
) -> bool {
    let reference_kind = match keyword {
        "require" | "preserve" => Some(IdKind::Control),
        "policy" => Some(IdKind::Policy),
        "goal" => Some(IdKind::Goal),
        _ => None,
    };
    if let Some(kind) = reference_kind {
        push_id_at(source, tokens, index + 1, kind, occurrences);
        return true;
    }
    if keyword == "owns" {
        extend_id_list(source, tokens, index + 1, IdKind::Control, occurrences);
        return true;
    }
    if let Some((id, span, next)) = requirement_id_at(source, tokens, index)
        && tokens
            .get(next)
            .is_some_and(|token| matches!(&token.kind, TokenKind::Ident(value) if value == "is"))
        && tokens.get(next + 1).is_some_and(
            |token| matches!(&token.kind, TokenKind::Ident(value) if value == "satisfied_by"),
        )
    {
        occurrences.push(IdOccurrence {
            kind: IdKind::Control,
            id,
            span,
            is_tag: false,
        });
        return true;
    }
    false
}

fn extend_id_list(
    source: &str,
    tokens: &[fsl_syntax::Token],
    mut start: usize,
    kind: IdKind,
    occurrences: &mut Vec<IdOccurrence>,
) {
    loop {
        let Some(next) = push_id_at(source, tokens, start, kind, occurrences) else {
            return;
        };
        if !tokens
            .get(next)
            .is_some_and(|token| matches!(&token.kind, TokenKind::Symbol(symbol) if symbol == ","))
        {
            return;
        }
        start = next + 1;
    }
}

fn push_id_at(
    source: &str,
    tokens: &[fsl_syntax::Token],
    start: usize,
    kind: IdKind,
    occurrences: &mut Vec<IdOccurrence>,
) -> Option<usize> {
    let (id, span, next) = requirement_id_at(source, tokens, start)?;
    occurrences.push(IdOccurrence {
        kind,
        id,
        span,
        is_tag: false,
    });
    Some(next)
}

fn requirement_id_at(
    source: &str,
    tokens: &[fsl_syntax::Token],
    start: usize,
) -> Option<(String, Span, usize)> {
    let first = tokens.get(start)?;
    let mut value = match &first.kind {
        TokenKind::Ident(_) | TokenKind::Int(_) => source
            .get(first.span.start.offset..first.span.end.offset)?
            .to_owned(),
        _ => return None,
    };
    let mut end = first.span.end;
    let mut index = start + 1;
    while matches!(tokens.get(index).map(|token| &token.kind), Some(TokenKind::Symbol(symbol)) if symbol == "-")
    {
        let component_token = tokens.get(index + 1)?;
        let component = match &component_token.kind {
            TokenKind::Ident(_) | TokenKind::Int(_) => {
                source.get(component_token.span.start.offset..component_token.span.end.offset)?
            }
            _ => break,
        };
        value.push('-');
        value.push_str(component);
        end = tokens[index + 1].span.end;
        index += 2;
    }
    Some((
        value,
        Span {
            start: first.span.start,
            end,
        },
        index,
    ))
}

fn legacy_requirement_ids(source: &str, options: ParseOptions) -> Vec<IdOccurrence> {
    let tree = lossless_document_with(source, &options);
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let declaration_keywords = [
        "init",
        "action",
        "invariant",
        "trans",
        "reachable",
        "until",
        "unless",
        "leadsTo",
    ];
    let mut occurrences = Vec::new();
    for (index, node) in tokens.iter().enumerate() {
        let LosslessKind::Token(TokenKind::String(value)) = &node.kind else {
            continue;
        };
        if tokens.get(index + 1).and_then(|node| node.symbol()) != Some("{") {
            continue;
        }
        let boundary = (0..index)
            .rev()
            .find(|position| matches!(tokens[*position].symbol(), Some("{" | "}" | ";")));
        let range_start = boundary.map_or(0, |position| position + 1);
        if !(range_start..index).any(|position| {
            tokens[position]
                .ident()
                .is_some_and(|keyword| declaration_keywords.contains(&keyword))
        }) {
            continue;
        }
        let metadata = fsl_syntax::MetaTag::parse(value, node.span);
        if !metadata.id.eq_ignore_ascii_case("undecided") {
            occurrences.push(IdOccurrence {
                kind: IdKind::Requirement,
                id: metadata.id,
                span: node.span,
                is_tag: true,
            });
        }
    }
    occurrences
}

fn metadata_diagnostics(source: &str, options: ParseOptions) -> Vec<MigrationDiagnostic> {
    let tree = lossless_document_with(source, &options);
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let declaration_keywords = [
        "spec",
        "init",
        "action",
        "invariant",
        "trans",
        "reachable",
        "until",
        "unless",
        "leadsTo",
    ];
    let mut diagnostics = Vec::new();
    for (index, node) in tokens.iter().enumerate() {
        let LosslessKind::Token(TokenKind::String(value)) = &node.kind else {
            continue;
        };
        if tokens.get(index + 1).and_then(|node| node.symbol()) != Some("{") {
            continue;
        }
        let boundary = (0..index)
            .rev()
            .find(|position| matches!(tokens[*position].symbol(), Some("{" | "}" | ";")));
        let range_start = boundary.map_or(0, |position| position + 1);
        let Some(keyword_index) = (range_start..index).find(|position| {
            tokens[*position]
                .ident()
                .is_some_and(|keyword| declaration_keywords.contains(&keyword))
        }) else {
            continue;
        };
        let keyword = tokens[keyword_index].ident().expect("matched keyword");
        let start_index = if keyword == "action"
            && keyword_index > range_start
            && tokens[keyword_index - 1].ident() == Some("fair")
        {
            keyword_index - 1
        } else {
            keyword_index
        };
        let declaration_start = tokens[start_index].span.start.offset;
        let attachment_start = boundary.map_or(0, |position| tokens[position].span.end.offset);
        let attachment = &source[attachment_start..node.span.start.offset];
        if attachment.contains("//") || source[attachment_start..declaration_start].contains('@') {
            diagnostics.push(MigrationDiagnostic {
                code: "legacy_string_metadata",
                taxonomy: Taxonomy::AmbiguousIntent,
                message: "cannot move string metadata across an attached comment or annotation"
                    .to_owned(),
                span: node.span,
                canonical_replacement: "move the comment, then use a typed annotation".to_owned(),
                machine_applicable: false,
                reason: None,
                edits: Vec::new(),
            });
            continue;
        }
        let meta = fsl_syntax::MetaTag::parse(value, node.span);
        let annotation = if keyword == "spec" {
            Annotation::from_legacy_kind(meta.id, meta.text, node.span)
        } else {
            Annotation::from_legacy(meta.id, meta.text, node.span)
        };
        let replacement = annotation.render_source();
        let indent = " ".repeat(tokens[start_index].span.start.column.saturating_sub(1) as usize);
        let insertion = format!("{replacement}\n{indent}");
        diagnostics.push(MigrationDiagnostic {
            code: "legacy_string_metadata",
            taxonomy: Taxonomy::Deprecated,
            message: "legacy string metadata is deprecated; use a typed annotation".to_owned(),
            span: node.span,
            canonical_replacement: replacement,
            machine_applicable: true,
            reason: None,
            edits: vec![
                SourceEdit {
                    span: Span {
                        start: tokens[start_index].span.start,
                        end: tokens[start_index].span.start,
                    },
                    replacement: insertion,
                },
                SourceEdit {
                    span: node.span,
                    replacement: String::new(),
                },
            ],
        });
    }
    diagnostics
}

fn default_diagnostics(
    source: &str,
    path: &str,
    options: ParseOptions,
) -> Vec<MigrationDiagnostic> {
    crate::frontend_output::implicit_initial_value_warnings(source, path, &options)
        .into_iter()
        .filter_map(|warning| {
            let suggestion = warning.get("suggestion")?;
            let edit_span = suggestion.get("span")?;
            let start = usize::try_from(edit_span.get("start")?.as_u64()?).ok()?;
            let end = usize::try_from(edit_span.get("end")?.as_u64()?).ok()?;
            let replacement = suggestion.get("replacement")?.as_str()?.to_owned();
            let location = warning.get("loc")?;
            let line = u32::try_from(location.get("line")?.as_u64()?).ok()?;
            let column = u32::try_from(location.get("column")?.as_u64()?).ok()?;
            let end_line = u32::try_from(location.get("end_line")?.as_u64()?).ok()?;
            let end_column = u32::try_from(location.get("end_column")?.as_u64()?).ok()?;
            let diagnostic_span = source_span(
                source,
                source_offset(source, line, column)?,
                source_offset(source, end_line, end_column)?,
            );
            Some(MigrationDiagnostic {
                code: "implicit_initial_value",
                taxonomy: Taxonomy::NonCanonical,
                message: warning.get("message")?.as_str()?.to_owned(),
                span: diagnostic_span,
                canonical_replacement: warning.get("canonical_replacement")?.as_str()?.to_owned(),
                machine_applicable: true,
                reason: None,
                edits: vec![SourceEdit {
                    span: source_span(source, start, end),
                    replacement,
                }],
            })
        })
        .collect()
}

fn source_offset(source: &str, line: u32, column: u32) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let line_start = if line == 1 {
        0
    } else {
        source
            .match_indices('\n')
            .nth(usize::try_from(line - 2).ok()?)?
            .0
            + 1
    };
    let tail = &source[line_start..];
    let line_text = tail.split_once('\n').map_or(tail, |(head, _)| head);
    let column_bytes = line_text
        .chars()
        .take(usize::try_from(column - 1).ok()?)
        .map(char::len_utf8)
        .sum::<usize>();
    (column_bytes <= line_text.len()).then_some(line_start + column_bytes)
}

fn action_map_diagnostics(
    source: &str,
    document: &SurfaceDocument,
    options: ParseOptions,
) -> Vec<MigrationDiagnostic> {
    let SurfaceDocument::Requirements(requirements) = document else {
        return Vec::new();
    };
    let implements = requirements
        .items
        .iter()
        .filter_map(|item| match item {
            RequirementsItem::Implements { items, span, .. } => Some((items, *span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    for item in &requirements.items {
        let action = match item {
            RequirementsItem::Action(action) => action,
            RequirementsItem::Requirement { items, .. } => {
                for nested in items {
                    if let fsl_syntax::RequirementBlockItem::Action(action) = nested {
                        diagnostics.extend(action_map_diagnostic(
                            source,
                            action,
                            &implements,
                            options,
                        ));
                    }
                }
                continue;
            }
            _ => continue,
        };
        diagnostics.extend(action_map_diagnostic(source, action, &implements, options));
    }
    diagnostics
}

#[allow(clippy::too_many_lines)]
fn action_map_diagnostic(
    source: &str,
    action: &fsl_syntax::RequirementAction,
    implements: &[(&Vec<fsl_syntax::RefinementItem>, Span)],
    options: ParseOptions,
) -> Vec<MigrationDiagnostic> {
    let mut diagnostics = Vec::new();
    for item in &action.items {
        if let RequirementActionItem::Branches { branches, .. } = item {
            for branch in branches {
                diagnostics.push(MigrationDiagnostic {
                    code: "inline_action_maps",
                    taxonomy: Taxonomy::AmbiguousIntent,
                    message:
                        "branch-specific maps cannot be moved to one unconditional correspondence"
                            .to_owned(),
                    span: branch.maps.span,
                    canonical_replacement: "write an explicit refinement correspondence".to_owned(),
                    machine_applicable: false,
                    reason: None,
                    edits: Vec::new(),
                });
            }
        }
    }
    let Some(maps) = &action.maps else {
        return diagnostics;
    };
    if implements.len() != 1 {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "inline maps require exactly one local implements block for a safe move"
                .to_owned(),
            span: maps.span,
            canonical_replacement: "move the mapping to an explicit implements or refinement block"
                .to_owned(),
            machine_applicable: false,
            reason: None,
            edits: Vec::new(),
        });
        return diagnostics;
    }
    let (items, implements_span) = implements[0];
    if items.iter().any(|item| {
        matches!(item, fsl_syntax::RefinementItem::Action { name, origin: CorrespondenceOrigin::ImplementsBlock, .. } if name == &action.name)
    }) {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "an explicit correspondence already exists for this action".to_owned(),
            span: maps.span,
            canonical_replacement: "remove the duplicate after choosing one correspondence".to_owned(),
            machine_applicable: false,
            reason: None,
            edits: Vec::new(),
        });
        return diagnostics;
    }
    let Some((maps_end, maps_text)) = maps_clause_text(source, maps.span.start.offset, options)
    else {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot determine the complete inline maps clause".to_owned(),
            span: maps.span,
            canonical_replacement: "write an explicit correspondence".to_owned(),
            machine_applicable: false,
            reason: None,
            edits: Vec::new(),
        });
        return diagnostics;
    };
    let Some(close) = block_close_offset(source, implements_span.start.offset, options) else {
        return diagnostics;
    };
    if comment_immediately_before(source, close, options) {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot insert a correspondence after a comment attached to the implements block closing brace"
                .to_owned(),
            span: maps.span,
            canonical_replacement: "move the comment, then rerun migrate".to_owned(),
            machine_applicable: false,
            reason: None,
            edits: Vec::new(),
        });
        return diagnostics;
    }
    let Some(params) = action_params_text(source, action.span.start.offset, options) else {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot determine the action parameter list without losing source trivia"
                .to_owned(),
            span: maps.span,
            canonical_replacement: "write an explicit correspondence".to_owned(),
            machine_applicable: false,
            reason: None,
            edits: Vec::new(),
        });
        return diagnostics;
    };
    let target = maps_text.strip_prefix("maps ").unwrap_or(&maps_text);
    let replacement = format!("action {}({params}) -> {target}", action.name);
    let indent = " ".repeat(source_position(source, close).column.saturating_sub(1) as usize + 2);
    diagnostics.push(MigrationDiagnostic {
        code: "inline_action_maps",
        taxonomy: Taxonomy::Deprecated,
        message: "inline maps are deprecated; use an explicit action correspondence".to_owned(),
        span: source_span(source, maps.span.start.offset, maps_end),
        canonical_replacement: replacement.clone(),
        machine_applicable: true,
        reason: None,
        edits: vec![
            SourceEdit {
                span: source_span(source, maps.span.start.offset, maps_end),
                replacement: String::new(),
            },
            SourceEdit {
                span: source_span(source, close, close),
                replacement: format!("{indent}{replacement}\n"),
            },
        ],
    });
    diagnostics
}

fn maps_clause_text(source: &str, start: usize, options: ParseOptions) -> Option<(usize, String)> {
    let tree = lossless_document_with(source, &options);
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let index = tokens
        .iter()
        .position(|node| node.span.start.offset == start)?;
    if tokens[index].ident() != Some("maps") {
        return None;
    }
    let end = if tokens.get(index + 1)?.ident() == Some("stutter") {
        tokens[index + 1].span.end.offset
    } else {
        let open =
            (index + 1..tokens.len()).find(|position| tokens[*position].symbol() == Some("("))?;
        matching_symbol(&tokens, open, "(", ")")?
    };
    Some((end, source[start..end].to_owned()))
}

fn action_params_text(source: &str, start: usize, options: ParseOptions) -> Option<String> {
    let tree = lossless_document_with(source, &options);
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let action = tokens
        .iter()
        .position(|node| node.span.start.offset == start)?;
    let open = (action..tokens.len()).find(|position| tokens[*position].symbol() == Some("("))?;
    let close = matching_symbol_index(&tokens, open, "(", ")")?;
    let params_start = tokens[open].span.end.offset;
    let params_end = tokens[close].span.start.offset;
    if tree.nodes().iter().any(|node| {
        matches!(node.kind, LosslessKind::LineComment)
            && node.span.start.offset >= params_start
            && node.span.end.offset <= params_end
    }) {
        return None;
    }
    Some(source[params_start..params_end].to_owned())
}

fn block_close_offset(source: &str, start: usize, options: ParseOptions) -> Option<usize> {
    let tree = lossless_document_with(source, &options);
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let item = tokens
        .iter()
        .position(|node| node.span.start.offset == start)?;
    let open = (item..tokens.len()).find(|position| tokens[*position].symbol() == Some("{"))?;
    let close = matching_symbol_index(&tokens, open, "{", "}")?;
    Some(tokens[close].span.start.offset)
}

fn comment_immediately_before(source: &str, offset: usize, options: ParseOptions) -> bool {
    let tree = lossless_document_with(source, &options);
    let Some(close) = tree
        .nodes()
        .iter()
        .position(|node| node.span.start.offset == offset && node.symbol() == Some("}"))
    else {
        return false;
    };
    tree.nodes()[..close]
        .iter()
        .rev()
        .find(|node| !matches!(node.kind, LosslessKind::Whitespace))
        .is_some_and(|node| matches!(node.kind, LosslessKind::LineComment))
}

fn matching_symbol(
    tokens: &[&fsl_syntax::LosslessNode],
    open: usize,
    left: &str,
    right: &str,
) -> Option<usize> {
    let close = matching_symbol_index(tokens, open, left, right)?;
    Some(tokens[close].span.end.offset)
}

fn matching_symbol_index(
    tokens: &[&fsl_syntax::LosslessNode],
    open: usize,
    left: &str,
    right: &str,
) -> Option<usize> {
    let mut depth = 0_u32;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.symbol() == Some(left) {
            depth += 1;
        } else if token.symbol() == Some(right) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Whether [`kernel_contract`] keeps the kernel's `origin` records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origins {
    /// Keep them (every rewrite but a process-form conversion).
    Kept,
    /// Drop them: a converted process form's events carry their own
    /// lowering origins, which name the new form, not a different meaning.
    Ignored,
}

/// The location-free checked Public Kernel a migration must keep: the
/// kernel contract without spans, source nodes and the singular legacy
/// requirement projection, plus each target's typed annotations.
///
/// # Errors
///
/// Returns the parse, model or `implements` error of `source`.
pub fn kernel_contract(
    source: &str,
    path: &Path,
    options: &ParseOptions,
    origins: Origins,
) -> Result<Value, String> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = fsl_core::parse_kernel_source_with_file_with_options(
        source,
        &resolver,
        path.to_string_lossy(),
        options,
    )
    .map_err(|error| error.to_string())?;
    let model = fsl_core::build_model(kernel.clone()).map_err(|error| error.to_string())?;
    if matches!(
        fsl_syntax::parse_surface_document_with(source, options),
        Ok(fsl_syntax::SurfaceDocument::Requirements(_))
    ) {
        fsl_core::requirements_implements_with_options(source, &resolver, &model, options)
            .map_err(|error| error.to_string())?;
    }
    let mut contract = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        &path.to_string_lossy(),
        source_dialect(source, options),
    )
    .map_err(|error| error.to_string())?;
    remove_migration_locations(&mut contract);
    remove_legacy_metadata_projection(&mut contract);
    if origins == Origins::Ignored {
        remove_origins(&mut contract);
    }
    Ok(json!({
        "kernel": contract,
        "annotations": migration_annotation_contract(&model),
    }))
}

fn remove_migration_locations(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in ["span", "source_node_id", "source_node_ids", "reverse_index"] {
                object.remove(key);
            }
            object.values_mut().for_each(remove_migration_locations);
        }
        Value::Array(values) => values.iter_mut().for_each(remove_migration_locations),
        _ => {}
    }
}

fn remove_legacy_metadata_projection(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("requirement");
            if let Some(Value::Object(origin)) = object.get_mut("origin") {
                origin.remove("declaration");
            }
            object
                .values_mut()
                .for_each(remove_legacy_metadata_projection);
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(remove_legacy_metadata_projection),
        _ => {}
    }
}

fn migration_annotation_contract(model: &fsl_core::KernelModel) -> Value {
    let mut targets = vec!["spec".to_owned(), "init".to_owned()];
    targets.extend(
        model
            .actions
            .iter()
            .map(|action| fsl_core::action_target(&action.name)),
    );
    for (kind, properties) in [
        ("invariant", &model.invariants),
        ("trans", &model.transitions),
        ("reachable", &model.reachables),
    ] {
        targets.extend(
            properties
                .iter()
                .map(|property| fsl_core::property_target(kind, &property.name)),
        );
    }
    targets.extend(
        model
            .leadstos
            .iter()
            .map(|property| fsl_core::property_target("leadsTo", &property.name)),
    );
    targets.sort();
    targets.dedup();
    Value::Object(
        targets
            .into_iter()
            .filter_map(|target| {
                let mut annotations = model
                    .annotations_for(&target)
                    .source_order()
                    .iter()
                    .map(fsl_syntax::Annotation::render_source)
                    .collect::<Vec<_>>();
                annotations.sort();
                (!annotations.is_empty()).then(|| (target, json!(annotations)))
            })
            .collect(),
    )
}

fn remove_origins(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("origin");
            object.values_mut().for_each(remove_origins);
        }
        Value::Array(values) => values.iter_mut().for_each(remove_origins),
        _ => {}
    }
}

/// The Public Kernel's dialect name for `source`'s root keyword.
#[must_use]
pub fn source_dialect<'a>(source: &'a str, options: &ParseOptions) -> &'a str {
    match fsl_syntax::dialect_keyword_with(source, options) {
        Ok("spec") => "kernel",
        Ok("dbsystem") => "db",
        Ok("ai_component") => "ai-component",
        Ok("agent") => "ai-agent",
        Ok(keyword) => keyword,
        Err(_) => "unknown",
    }
}

#[cfg(test)]
mod id_policy_tests {
    use super::*;

    #[test]
    fn builtin_policy_uses_kind_scope_and_three_digit_number() {
        let policy = IdPolicy::default();
        for (kind, id) in [
            (IdKind::Requirement, "REQ-PAYMENT-001"),
            (IdKind::Requirement, "NFR-PAYMENT-001"),
            (IdKind::Acceptance, "AC-PAYMENT-001"),
            (IdKind::Forbidden, "FB-PAYMENT-001"),
            (IdKind::Policy, "POL-PAYMENT-001"),
            (IdKind::Goal, "GOAL-PAYMENT-001"),
            (IdKind::Control, "CTRL-PAYMENT-001"),
            (IdKind::Model, "MODEL-PAYMENT-001"),
            (IdKind::Assumption, "ASSUME-PAYMENT-001"),
        ] {
            assert!(policy.accepts(kind, id), "{kind:?} should accept {id}");
        }
        assert!(!policy.accepts(IdKind::Requirement, "REQ-1"));
        assert!(!policy.accepts(IdKind::Acceptance, "MONITOR-LOWER-BOUND"));
        assert!(!policy.accepts(IdKind::Goal, "CanPay"));
    }

    #[test]
    fn policy_templates_are_configurable_without_a_regex_dependency() {
        let mut policy = IdPolicy::default();
        policy
            .set_patterns(IdKind::Requirement, vec!["PAY-{number}".to_owned()])
            .expect("valid template");
        assert!(policy.accepts(IdKind::Requirement, "PAY-42"));
        assert!(!policy.accepts(IdKind::Requirement, "REQ-PAYMENT-001"));
        assert!(
            policy
                .set_patterns(IdKind::Goal, vec!["GOAL-{unknown}".to_owned()])
                .is_err()
        );
    }

    #[test]
    fn diagnostics_keep_surface_id_kinds_distinct() {
        let source = r#"
requirements Checkout {
  requirement REQ-1 "requirement" { }
  acceptance AC-1 "acceptance" { expect true }
  forbidden NEG-1 "forbidden" { reject() expect rejected }
  @requirement("MODEL-CHECKOUT-1")
  invariant Tagged { true }
  action legacy() "ASSUME-CHECKOUT-1: legacy" { }
}
business CheckoutBusiness {
  control CTRL-1 "control"
  policy PAY-1 "policy" invariant { true }
  goal CanPay "goal" { true }
}
"#;
        let diagnostics =
            id_policy_diagnostics(source, &IdPolicy::default(), &ParseOptions::stable());
        let messages = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "requirement ID 'REQ-1'",
            "acceptance ID 'AC-1'",
            "forbidden ID 'NEG-1'",
            "model ID 'MODEL-CHECKOUT-1'",
            "assumption ID 'ASSUME-CHECKOUT-1'",
            "control ID 'CTRL-1'",
            "policy ID 'PAY-1'",
            "goal ID 'CanPay'",
        ] {
            assert!(
                messages.iter().any(|message| message.contains(expected)),
                "missing {expected}: {messages:?}"
            );
        }
    }

    #[test]
    fn unrelated_forbidden_keyword_is_not_a_scenario_id() {
        let source = r"@acme.note(governance)
ai_component Safe {
  authority { forbidden DeleteCustomerData; }
  fallback { when low_confidence require human_review; }
}";
        assert!(
            id_policy_diagnostics(source, &IdPolicy::default(), &ParseOptions::stable()).is_empty()
        );
    }

    #[test]
    fn relationship_references_are_checked_by_their_semantic_kind() {
        let business = r#"business Claims {
  policy POL-CLAIMS-001 "policy" satisfies CTRL-BAD { true }
}"#;
        assert!(
            id_policy_diagnostics(business, &IdPolicy::default(), &ParseOptions::stable())
                .iter()
                .any(|diagnostic| diagnostic.message.contains("control ID 'CTRL-BAD'"))
        );

        let governance = r#"governance Claims {
  authority Risk owns CTRL-BAD
  delegates Claims from "claims.fsl" {
    require CTRL-BAD
    CTRL-BAD is satisfied_by policy POL-BAD, goal GOAL-BAD
  }
  preservation Release {
    preserve CTRL-BAD
  }
}"#;
        let messages =
            id_policy_diagnostics(governance, &IdPolicy::default(), &ParseOptions::stable())
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>();
        for expected in [
            "control ID 'CTRL-BAD'",
            "policy ID 'POL-BAD'",
            "goal ID 'GOAL-BAD'",
        ] {
            assert!(
                messages.iter().any(|message| message.contains(expected)),
                "missing {expected}: {messages:?}"
            );
        }
    }

    #[test]
    fn model_and_assumption_prefixes_must_be_distinct_and_literal() {
        let mut policy = IdPolicy::default();
        assert!(
            policy
                .set_patterns(IdKind::Model, vec!["{scope}-MODEL-{number}".to_owned()])
                .is_err()
        );
        policy
            .set_patterns(IdKind::Model, vec!["TRACE-{scope}-{number}".to_owned()])
            .expect("literal model prefix");
        policy
            .set_patterns(
                IdKind::Assumption,
                vec!["TRACE-A-{scope}-{number}".to_owned()],
            )
            .expect("literal assumption prefix");
        assert!(policy.validate().is_err());

        let mut policy = IdPolicy::default();
        policy
            .set_patterns(IdKind::Model, vec!["REQ-{scope}-{number:3}".to_owned()])
            .expect("literal model prefix");
        assert!(policy.validate().is_err());
    }
}
