// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{
    Annotation, CanonicalRewriteKind, CorrespondenceOrigin, LosslessKind, RequirementActionItem,
    RequirementsItem, SourceEdit, Span, SurfaceDocument, TokenKind, canonical_rewrites,
    lossless_document, source_position, source_span,
};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edition {
    Current,
    Next,
}

impl Edition {
    /// Parse the public edition spelling.
    ///
    /// # Errors
    ///
    /// Returns an error for an edition without a migration policy.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "current" => Ok(Self::Current),
            "next" => Ok(Self::Next),
            _ => Err("--edition must be current or next".to_owned()),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Next => "next",
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
    pub edits: Vec<SourceEdit>,
}

impl MigrationDiagnostic {
    #[must_use]
    pub fn json(&self, path: &str, edition: Edition) -> Value {
        let mut value = json!({
            "code": self.code,
            "kind": self.code,
            "taxonomy": self.taxonomy.as_str(),
            "severity": if edition == Edition::Next { "error" } else { "warning" },
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
    #[must_use]
    pub fn refused(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| !diagnostic.machine_applicable)
    }
}

/// Plan all edition migrations against one immutable source snapshot.
///
/// # Errors
///
/// Returns a parse or edit error. Unsupported-but-recognizable legacy syntax
/// is represented as a non-machine-applicable diagnostic instead.
#[allow(clippy::too_many_lines)]
pub fn plan_migration(source: &str, path: &str, edition: Edition) -> Result<MigrationPlan, String> {
    let unsupported = unsupported_double_ampersands(source);
    if !unsupported.is_empty() {
        return Ok(MigrationPlan {
            diagnostics: unsupported,
            migrated_source: None,
        });
    }

    let document = fsl_syntax::parse_surface_document(source).map_err(|error| error.to_string())?;
    if matches!(document, SurfaceDocument::Agent(_)) {
        return Ok(MigrationPlan {
            diagnostics: Vec::new(),
            migrated_source: None,
        });
    }
    let mut diagnostics = Vec::new();
    match canonical_rewrites(source) {
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
                edits: rewrite.edits,
            }
        })),
        Err(fsl_syntax::FormatError::Unsafe { message, span }) => {
            let (code, canonical_replacement) = if message.contains("domain enum") {
                (
                    "deprecated_domain_enum_union",
                    "move the interior comment, then use `enum Name { ... }`".to_owned(),
                )
            } else if message.contains("quantifier") {
                (
                    "legacy_quantifier_colon",
                    "add an explicit brace-delimited quantifier body".to_owned(),
                )
            } else {
                (
                    "unsafe_source_attachment",
                    "move the comment, then rerun migrate".to_owned(),
                )
            };
            diagnostics.push(MigrationDiagnostic {
                code,
                taxonomy: Taxonomy::AmbiguousIntent,
                message,
                span,
                canonical_replacement,
                machine_applicable: false,
                edits: Vec::new(),
            });
        }
        Err(error) => return Err(error.to_string()),
    }
    diagnostics.extend(metadata_diagnostics(source));
    diagnostics.extend(default_diagnostics(source, path));
    diagnostics.extend(action_map_diagnostics(source, &document));

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
    let formatted = fsl_syntax::format_source(
        &rewritten,
        match edition {
            Edition::Current => fsl_syntax::FormatEdition::Current,
            Edition::Next => fsl_syntax::FormatEdition::Next,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(MigrationPlan {
        migrated_source: (formatted != source).then_some(formatted),
        diagnostics,
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

fn metadata_diagnostics(source: &str) -> Vec<MigrationDiagnostic> {
    let tree = lossless_document(source);
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

fn default_diagnostics(source: &str, path: &str) -> Vec<MigrationDiagnostic> {
    crate::frontend_output::implicit_initial_value_warnings(source, path)
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

fn action_map_diagnostics(source: &str, document: &SurfaceDocument) -> Vec<MigrationDiagnostic> {
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
                        diagnostics.extend(action_map_diagnostic(source, action, &implements));
                    }
                }
                continue;
            }
            _ => continue,
        };
        diagnostics.extend(action_map_diagnostic(source, action, &implements));
    }
    diagnostics
}

#[allow(clippy::too_many_lines)]
fn action_map_diagnostic(
    source: &str,
    action: &fsl_syntax::RequirementAction,
    implements: &[(&Vec<fsl_syntax::RefinementItem>, Span)],
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
            edits: Vec::new(),
        });
        return diagnostics;
    }
    let Some((maps_end, maps_text)) = maps_clause_text(source, maps.span.start.offset) else {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot determine the complete inline maps clause".to_owned(),
            span: maps.span,
            canonical_replacement: "write an explicit correspondence".to_owned(),
            machine_applicable: false,
            edits: Vec::new(),
        });
        return diagnostics;
    };
    let Some(close) = block_close_offset(source, implements_span.start.offset) else {
        return diagnostics;
    };
    if comment_immediately_before(source, close) {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot insert a correspondence after a comment attached to the implements block closing brace"
                .to_owned(),
            span: maps.span,
            canonical_replacement: "move the comment, then rerun migrate".to_owned(),
            machine_applicable: false,
            edits: Vec::new(),
        });
        return diagnostics;
    }
    let Some(params) = action_params_text(source, action.span.start.offset) else {
        diagnostics.push(MigrationDiagnostic {
            code: "inline_action_maps",
            taxonomy: Taxonomy::AmbiguousIntent,
            message: "cannot determine the action parameter list without losing source trivia"
                .to_owned(),
            span: maps.span,
            canonical_replacement: "write an explicit correspondence".to_owned(),
            machine_applicable: false,
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

fn maps_clause_text(source: &str, start: usize) -> Option<(usize, String)> {
    let tree = lossless_document(source);
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

fn action_params_text(source: &str, start: usize) -> Option<String> {
    let tree = lossless_document(source);
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

fn block_close_offset(source: &str, start: usize) -> Option<usize> {
    let tree = lossless_document(source);
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

fn comment_immediately_before(source: &str, offset: usize) -> bool {
    let tree = lossless_document(source);
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
