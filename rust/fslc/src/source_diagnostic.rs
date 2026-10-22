// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{CanonicalRewriteKind, SourcePos, Span, SurfaceDocument};

use crate::verification_output::DiagnosticClass;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDiagnostic {
    pub kind: &'static str,
    pub code: String,
    pub message: String,
    pub span: Span,
    /// Whether [`Self::span`] is the diagnostic's real location.
    ///
    /// The LSP needs a span for every diagnostic and accepts the start of the
    /// document as a last resort, but the CLI's `loc` must stay absent rather
    /// than claim a position the diagnostic does not have (issue 555).
    pub located: bool,
    pub hint: Option<String>,
    pub quick_fix: Option<fsl_core::DiagnosticEdit>,
    /// The typed classification [`Self::kind`] was derived from, so a surface
    /// that renders this diagnostic again classifies it from the type rather
    /// than from [`Self::message`] (issue 1167).
    pub class: DiagnosticClass,
}

/// Run the authoritative syntax and typed-model gates and return editor diagnostics.
#[must_use]
pub fn diagnostics(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
    options: &fsl_syntax::ParseOptions,
) -> Vec<SourceDiagnostic> {
    diagnostics_with_model(source, source_file, resolver, options).0
}

/// Run frontend diagnostics and retain the checked model for analysis consumers.
#[must_use]
pub fn diagnostics_with_model(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
    options: &fsl_syntax::ParseOptions,
) -> (Vec<SourceDiagnostic>, Option<fsl_core::KernelModel>) {
    if crate::frontend_output::is_ai_project(source) {
        return (migration_diagnostics(source, *options), None);
    }
    // Standalone causal models bypass dialect dispatch (docs/design/DESIGN-causal.md):
    // surface their own parse errors, never FSL-DIALECT-UNKNOWN, and no kernel.
    if fsl_syntax::is_causal_source(source) {
        return match fsl_syntax::parse_causal(source) {
            Ok(_) => (Vec::new(), None),
            Err(error) => (
                vec![SourceDiagnostic {
                    kind: "parse",
                    code: error.code().to_owned(),
                    message: error.to_string(),
                    span: error.span,
                    located: true,
                    hint: None,
                    quick_fix: None,
                    class: DiagnosticClass::Message,
                }],
                None,
            ),
        };
    }
    let parsed = match fsl_syntax::parse_document_with(fsl_syntax::SourceFile::new(source), options)
    {
        Ok(parsed) => parsed,
        Err(error) => {
            return (
                vec![SourceDiagnostic {
                    kind: "parse",
                    code: error.code().to_owned(),
                    message: error.to_string(),
                    span: error.span,
                    located: true,
                    hint: None,
                    quick_fix: None,
                    class: DiagnosticClass::Message,
                }],
                None,
            );
        }
    };
    if matches!(
        parsed.surface,
        SurfaceDocument::Agent(_) | SurfaceDocument::Refinement(_)
    ) {
        return (migration_diagnostics(source, *options), None);
    }
    let kernel = match fsl_core::parse_kernel_source_with_file_with_options(
        source,
        resolver,
        source_file,
        options,
    ) {
        Ok(kernel) => kernel,
        Err(error) => return (vec![core_diagnostic(source, &error)], None),
    };
    match fsl_core::build_model(kernel) {
        Ok(model) => (migration_diagnostics(source, *options), Some(model)),
        Err(error) => (vec![model_diagnostic(source, &error, *options)], None),
    }
}

/// The code of a diagnostic reporting an acceptance / forbidden trace the
/// requirement-trace gate rejected.
pub const REQUIREMENT_TRACE_CODE: &str = "FSL-REQUIREMENT-TRACE";

/// The requirement-trace gate on a model [`diagnostics_with_model`] built, as
/// editor diagnostics: none when the gate passes or `source` declares no
/// traces, otherwise the one error `check` reports, at its location (#1243).
///
/// A rejected trace keeps the `kind` `check` reports (`acceptance`,
/// `forbidden`, `forbidden_setup`) under [`REQUIREMENT_TRACE_CODE`]; a trace
/// that cannot be parsed or replayed is a semantics diagnostic.
#[must_use]
pub fn trace_gate_diagnostics(
    source: &str,
    model: &fsl_core::KernelModel,
    options: &fsl_syntax::ParseOptions,
) -> Vec<SourceDiagnostic> {
    match crate::verification_output::require_trace_gate(
        &serde_json::Map::new(),
        source,
        model,
        options,
    ) {
        Ok(_checked) => Vec::new(),
        Err(refusal) => vec![trace_refusal_diagnostic(source, &refusal)],
    }
}

fn trace_refusal_diagnostic(source: &str, refusal: &serde_json::Value) -> SourceDiagnostic {
    let field = |name: &str| refusal.get(name).and_then(serde_json::Value::as_str);
    let rejected = match field("kind") {
        Some("acceptance") => Some(("acceptance", "acceptance")),
        Some("forbidden") => Some(("forbidden", "forbidden")),
        Some("forbidden_setup") => Some(("forbidden_setup", "forbidden")),
        _ => None,
    };
    let (kind, code, message) = match rejected {
        Some((kind, label)) => {
            let mut message = format!(
                "{label} '{}' does not pass the requirement-trace gate",
                field("id").unwrap_or_default()
            );
            if let Some(detail) = field("message").or_else(|| field("hint")) {
                message.push_str(": ");
                message.push_str(detail);
            }
            (kind, REQUIREMENT_TRACE_CODE, message)
        }
        None => (
            "semantics",
            "FSL-SEMANTIC",
            field("message").unwrap_or_default().to_owned(),
        ),
    };
    let loc = refusal.get("loc").and_then(|loc| {
        let number = |name: &str| loc.get(name)?.as_u64().and_then(|n| u32::try_from(n).ok());
        Some((number("line")?, number("column")?))
    });
    SourceDiagnostic {
        kind,
        code: code.to_owned(),
        message,
        span: loc.map_or_else(
            || point_span(source, 1, 1),
            |(line, column)| point_span(source, line, column),
        ),
        located: loc.is_some(),
        hint: None,
        quick_fix: None,
        class: DiagnosticClass::Message,
    }
}

fn core_diagnostic(source: &str, error: &fsl_core::CoreError) -> SourceDiagnostic {
    let diagnostic = crate::spec_load::SemanticDiagnostic::from_core_error(error);
    SourceDiagnostic {
        kind: crate::verification_output::diagnostic_kind(&diagnostic.message, &diagnostic.class),
        code: error.diagnostic_code.unwrap_or("FSL-SEMANTIC").to_owned(),
        message: diagnostic.message,
        span: error
            .origin
            .as_deref()
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span)
            .unwrap_or_else(|| point_span(source, error.line, error.column)),
        // `line`/`column` also carry legacy placeholders such as `(1, 1)`.
        // Only an authored origin proves that the public location is real.
        located: diagnostic.loc.is_some(),
        hint: None,
        quick_fix: None,
        class: diagnostic.class,
    }
}

fn model_diagnostic(
    source: &str,
    error: &fsl_core::ModelError,
    options: fsl_syntax::ParseOptions,
) -> SourceDiagnostic {
    let message = error.to_string();
    let class = DiagnosticClass::model(error.name_resolution);
    let kind = crate::verification_output::diagnostic_kind(&message, &class);
    let located = error
        .span
        .or_else(|| {
            error
                .origin
                .as_deref()
                .and_then(|origin| origin.primary.as_ref())
                .and_then(|site| site.span)
        })
        .or_else(|| diagnostic_span_from_message(source, &message, options));
    SourceDiagnostic {
        kind,
        code: error.diagnostic_code.map_or_else(
            || match kind {
                "type" => "FSL-TYPE".to_owned(),
                "name" => "FSL-NAME".to_owned(),
                _ => "FSL-SEMANTIC".to_owned(),
            },
            str::to_owned,
        ),
        message,
        span: located.unwrap_or_else(|| point_span(source, 1, 1)),
        located: located.is_some(),
        hint: error.hint.as_deref().cloned(),
        quick_fix: error.quick_fix.as_deref().cloned(),
        class,
    }
}

fn diagnostic_span_from_message(
    source: &str,
    message: &str,
    options: fsl_syntax::ParseOptions,
) -> Option<Span> {
    let quoted = message.split('\'').nth(1)?;
    fsl_syntax::lex_with(source, &options)
        .ok()?
        .into_iter()
        .find_map(|token| {
            matches!(&token.kind, fsl_syntax::TokenKind::Ident(name) if name == quoted)
                .then_some(token.span)
        })
}

fn migration_diagnostics(source: &str, options: fsl_syntax::ParseOptions) -> Vec<SourceDiagnostic> {
    fsl_syntax::canonical_rewrites_with(source, &options).map_or_else(
        |_| Vec::new(),
        |rewrites| {
            rewrites
                .into_iter()
                .map(|rewrite| {
                    let (code, message) = match rewrite.kind {
                        CanonicalRewriteKind::DomainEnum => (
                            "deprecated_domain_enum_union",
                            "legacy domain enum union syntax is deprecated",
                        ),
                        CanonicalRewriteKind::LogicalOperator => (
                            "legacy_logical_operator",
                            "legacy logical operator spelling is non-canonical",
                        ),
                        CanonicalRewriteKind::Quantifier => (
                            "legacy_quantifier_colon",
                            "legacy colon quantifier syntax is deprecated",
                        ),
                    };
                    SourceDiagnostic {
                        kind: "migration",
                        code: code.to_owned(),
                        message: message.to_owned(),
                        span: rewrite.span,
                        located: true,
                        hint: None,
                        quick_fix: None,
                        class: DiagnosticClass::Message,
                    }
                })
                .collect()
        },
    )
}

/// A zero-width span at a 1-based `line` / `column` as the lexer counts them:
/// lines end at `\n` only, and `column` counts characters, not bytes. The
/// offset is a byte offset on a character boundary, clamped to the line, so
/// the LSP's UTF-16 conversion can slice the source there.
fn point_span(source: &str, line: u32, column: u32) -> Span {
    let line_index = usize::try_from(line.saturating_sub(1)).expect("line fits usize");
    let column_index = usize::try_from(column.saturating_sub(1)).expect("column fits usize");
    let line_start = source
        .split_inclusive('\n')
        .take(line_index)
        .map(str::len)
        .sum::<usize>();
    let rest = &source[line_start..];
    let line_text = rest.split_once('\n').map_or(rest, |(text, _)| text);
    let offset = line_start
        + line_text
            .char_indices()
            .nth(column_index)
            .map_or(line_text.len(), |(offset, _)| offset);
    let point = SourcePos {
        offset,
        line,
        column,
    };
    Span {
        start: point,
        end: point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_authoritative_parse_diagnostic() {
        let source = "spec Broken { state {";
        let found = diagnostics(
            source,
            "broken.fsl",
            &fsl_core::FsResolver::new("."),
            &fsl_syntax::ParseOptions::stable(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "parse");
        assert_eq!(found[0].code, "FSL-PARSE");
        assert!(found[0].message.contains("expected"));
        assert_eq!(found[0].span.start.line, 1);
    }

    #[test]
    fn preserves_typed_model_diagnostic_and_span() {
        let source = r"spec Broken {
  state { value: Missing }
  init { value = 0 }
}";
        let found = diagnostics(
            source,
            "broken.fsl",
            &fsl_core::FsResolver::new("."),
            &fsl_syntax::ParseOptions::stable(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "type");
        assert!(found[0].message.contains("unknown type 'Missing'"));
        assert_eq!(found[0].span.start.line, 2);
    }

    /// An unlocated core error carries a character column; its fallback span
    /// must land on the reported token as a byte offset, after non-ASCII text
    /// on the same line and after CRLF line ends.
    #[test]
    fn unlocated_core_error_offset_counts_characters_and_crlf() {
        for newline in ["\n", "\r\n"] {
            let source = [
                "spec Probe {",
                "  state { x: Int }",
                "  init { x = 0 }",
                "  invariant I \"不変条件😀\" { undefinedPred(x) }",
                "}",
            ]
            .join(newline);
            let found = diagnostics(
                &source,
                "probe.fsl",
                &fsl_core::FsResolver::new("."),
                &fsl_syntax::ParseOptions::stable(),
            );
            let diagnostic = found
                .iter()
                .find(|item| item.message.contains("undefined predicate"))
                .unwrap_or_else(|| panic!("{newline:?}: {found:?}"));
            assert!(!diagnostic.located, "{newline:?}: {diagnostic:?}");
            let start = diagnostic.span.start;
            assert_eq!((start.line, start.column), (4, 25), "{newline:?}");
            assert_eq!(
                source.get(start.offset..start.offset + "undefinedPred".len()),
                Some("undefinedPred"),
                "{newline:?}: offset {}",
                start.offset
            );
        }
    }

    #[test]
    fn accepts_native_annotations_without_python() {
        let source = r#"spec Annotated {
  state { ready: Bool }
  init { ready = false }
  @undecided("pending")
  invariant Ready { ready }
}"#;
        assert!(
            diagnostics(
                source,
                "annotated.fsl",
                &fsl_core::FsResolver::new("."),
                &fsl_syntax::ParseOptions::stable()
            )
            .is_empty()
        );
    }

    #[test]
    fn exposes_authoritative_migration_rewrite() {
        let source = r"domain Orders {
  type Status = Pending | Approved
  aggregate Order { state { status: Status = Pending; } }
}";
        let found = diagnostics(
            source,
            "orders.fsl",
            &fsl_core::FsResolver::new("."),
            &fsl_syntax::ParseOptions::stable(),
        );
        assert!(
            found
                .iter()
                .any(|item| item.code == "deprecated_domain_enum_union")
        );
    }
}
