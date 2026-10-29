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
    pub related: Option<DependencyDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyDiagnostic {
    pub file: String,
    pub source: String,
    pub span: Span,
    pub error: bool,
}

struct ResolverRef<'a>(&'a dyn fsl_core::FileResolver);
impl fsl_core::FileResolver for ResolverRef<'_> {
    fn read(&self, path: &str) -> Result<String, fsl_core::CoreError> {
        self.0.read(path)
    }
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
///
/// # Panics
/// Panics if edition classification or parsed AI diagnostics omit required
/// diagnostic metadata.
#[must_use]
pub fn diagnostics_with_model(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
    options: &fsl_syntax::ParseOptions,
) -> (Vec<SourceDiagnostic>, Option<fsl_core::KernelModel>) {
    let edition = match crate::migration::admit_source(
        source,
        source_file,
        None,
        options,
        &serde_json::Map::new,
        false,
    ) {
        Ok(edition) => edition,
        Err((refusal, _)) => {
            let diagnostics = if refusal["kind"] == "parse" {
                let span = edition_parse_error(source, *options).span;
                vec![message_diagnostic(
                    "parse",
                    refusal["diagnostic_code"].as_str().expect("parse code"),
                    refusal["message"]
                        .as_str()
                        .expect("parse message")
                        .to_owned(),
                    span,
                )]
            } else {
                refusal["findings"]
                    .as_array()
                    .expect("edition findings")
                    .iter()
                    .map(|finding| edition_diagnostic(source, finding))
                    .collect()
            };
            return (diagnostics, None);
        }
    };
    let log = crate::migration::DependencyRecords::default();
    let admitted = crate::migration::DependencyResolver::new(
        ResolverRef(resolver),
        log.clone(),
        *options,
        &serde_json::Map::new,
    );
    let (mut diagnostics, mut model) =
        diagnostics_admitted(source, source_file, &admitted, *options, &edition);
    let log = log.borrow();
    if log.refusal.is_some() {
        diagnostics.clear();
        model = None;
    }
    diagnostics.extend(
        edition
            .warnings()
            .iter()
            .map(|finding| edition_diagnostic(source, finding)),
    );
    diagnostics.extend(dependency_diagnostics(
        source,
        source_file,
        resolver,
        *options,
        &log,
    ));
    (diagnostics, model)
}

fn edition_parse_error(source: &str, options: fsl_syntax::ParseOptions) -> fsl_syntax::ParseError {
    match fsl_syntax::document_edition(source, None, fsl_syntax::Edition::Fsl4, &options) {
        Err(error) => error,
        Ok(edition) => fsl_syntax::parse_surface_document_with(source, &options)
            .expect_err("edition parse refusal")
            .for_edition(source, edition, &options),
    }
}

fn dependency_diagnostics(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
    options: fsl_syntax::ParseOptions,
    log: &crate::migration::DependencyLog,
) -> Vec<SourceDiagnostic> {
    if log.sources.is_empty() {
        return Vec::new();
    }
    let SurfaceDocument::Compose(compose) =
        fsl_syntax::parse_surface_document_with(source, &options).expect("read compose")
    else {
        unreachable!("editor dependencies are compose uses")
    };
    let mut diagnostics = Vec::new();
    for (path, child) in &log.sources {
        let span = compose
            .items
            .iter()
            .find_map(|item| match item {
                fsl_syntax::ComposeItem::Use {
                    path: reference,
                    span,
                    ..
                } if reference == path => Some(*span),
                _ => None,
            })
            .expect("read dependency has a use declaration");
        let refused = log.refusal.as_ref().is_some_and(|(output, _)| {
            output["loc"]["file"] == *path || output["findings"][0]["loc"]["file"] == *path
        });
        // The same snapshot already refused admission, so this reuses the root
        // refusal-to-diagnostic projection without lowering the child.
        let mut children = if refused {
            diagnostics_with_model(child, path, resolver, &options).0
        } else {
            Vec::new()
        };
        children.extend(
            log.warnings
                .iter()
                .filter(|finding| finding["loc"]["file"] == *path)
                .map(|finding| edition_diagnostic(child, finding)),
        );
        for mut diagnostic in children {
            if diagnostic.kind == "parse" {
                diagnostic.message = edition_parse_error(child, options).message;
            }
            diagnostic.related = Some(DependencyDiagnostic {
                file: std::path::Path::new(source_file)
                    .with_file_name(path)
                    .to_string_lossy()
                    .into_owned(),
                source: child.clone(),
                span: diagnostic.span,
                error: refused,
            });
            diagnostic.span = span;
            diagnostic.quick_fix = None;
            diagnostics.push(diagnostic);
        }
    }
    diagnostics
}

pub(crate) fn diagnostics_admitted(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
    options: fsl_syntax::ParseOptions,
    _edition: &crate::migration::SourceEdition,
) -> (Vec<SourceDiagnostic>, Option<fsl_core::KernelModel>) {
    if crate::frontend_output::is_ai_project(source) {
        return (ai_project_diagnostics(source, source_file, options), None);
    }
    // Standalone causal models bypass dialect dispatch (docs/design/DESIGN-causal.md):
    // surface their own parse errors, never FSL-DIALECT-UNKNOWN, and no kernel.
    if fsl_syntax::is_causal_source(source) {
        return match fsl_syntax::parse_causal(source) {
            Ok(_) => (Vec::new(), None),
            Err(error) => (
                vec![message_diagnostic(
                    "parse",
                    error.code(),
                    error.to_string(),
                    error.span,
                )],
                None,
            ),
        };
    }
    let parsed =
        match fsl_syntax::parse_document_with(fsl_syntax::SourceFile::new(source), &options) {
            Ok(parsed) => parsed,
            Err(error) => {
                return (
                    vec![message_diagnostic(
                        "parse",
                        error.code(),
                        error.to_string(),
                        error.span,
                    )],
                    None,
                );
            }
        };
    if matches!(
        parsed.surface,
        SurfaceDocument::Agent(_) | SurfaceDocument::Refinement(_)
    ) {
        return (migration_diagnostics(source, options), None);
    }
    let kernel = match fsl_core::parse_kernel_source_with_file_with_options(
        source,
        resolver,
        source_file,
        &options,
    ) {
        Ok(kernel) => kernel,
        Err(error) => return (vec![core_diagnostic(source, &error)], None),
    };
    match fsl_core::build_model(kernel) {
        Ok(model) => (migration_diagnostics(source, options), Some(model)),
        Err(error) => (vec![model_diagnostic(source, &error, options)], None),
    }
}

fn ai_project_diagnostics(
    source: &str,
    source_file: &str,
    options: fsl_syntax::ParseOptions,
) -> Vec<SourceDiagnostic> {
    let project = match crate::frontend_output::parse_checked_ai_project(
        source,
        crate::frontend_output::ai_project_name(source_file),
    ) {
        Ok(project) => project,
        Err(error) => {
            let (line, column) = error.position.expect("project parse location");
            return vec![message_diagnostic(
                "parse",
                error.diagnostic_code.expect("project parse code"),
                error.message,
                point_span(source, line, column),
            )];
        }
    };
    for component in &project.components {
        if let Err((error, loc)) = fsl_core::validate_ai_component_with_location(component) {
            let loc = loc.expect("parsed component failure location");
            let mut diagnostic = core_diagnostic(source, &error);
            diagnostic.span = point_span(source, loc.line, loc.column);
            diagnostic.located = true;
            return vec![diagnostic];
        }
    }
    migration_diagnostics(source, options)
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

/// The requirements checks the finite-domain enumeration deferred, asked of
/// the solver `new_solver` makes, on a model [`diagnostics_with_model`]
/// built: none when every one holds, otherwise the one error `check` reports
/// for the first it refutes, at its location (decisions/048 J3). `check`
/// asks them after the requirement-trace gate, so a surface showing
/// [`trace_gate_diagnostics`] asks them only when that gate passes.
pub async fn solver_diagnostics<S: fsl_solver::SmtSolver>(
    source: &str,
    model: &fsl_core::KernelModel,
    options: &fsl_syntax::ParseOptions,
    new_solver: &mut crate::requirements_solver::NewSolver<'_, S>,
) -> Vec<SourceDiagnostic> {
    crate::requirements_solver::refute(source, model, options, new_solver)
        .await
        .map(|rejection| {
            let diagnostic = rejection.diagnostic();
            SourceDiagnostic {
                kind: crate::verification_output::diagnostic_kind(
                    &diagnostic.message,
                    &diagnostic.class,
                ),
                code: rejection.code.to_owned(),
                message: diagnostic.message,
                span: rejection.span,
                located: true,
                hint: None,
                quick_fix: None,
                class: diagnostic.class,
                related: None,
            }
        })
        .into_iter()
        .collect()
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
        related: None,
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
            .map_or_else(
                || point_span(source, error.line, error.column),
                |span| {
                    // Line/column-only origins (currently AI's AiLoc) use offset 0.
                    // Recover such spans' byte offsets before LSP UTF-16 conversion,
                    // regardless of dialect, when their position is not 1:1.
                    if span.start.offset == 0 && (span.start.line, span.start.column) != (1, 1) {
                        Span {
                            start: point_span(source, span.start.line, span.start.column).start,
                            end: point_span(source, span.end.line, span.end.column).end,
                        }
                    } else {
                        span
                    }
                },
            ),
        // `line`/`column` also carry legacy placeholders such as `(1, 1)`.
        // Only an authored origin proves that the public location is real.
        located: diagnostic.loc.is_some(),
        hint: None,
        quick_fix: None,
        class: diagnostic.class,
        related: None,
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
        related: None,
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

fn message_diagnostic(
    kind: &'static str,
    code: &str,
    message: String,
    span: Span,
) -> SourceDiagnostic {
    SourceDiagnostic {
        kind,
        code: code.to_owned(),
        message,
        span,
        located: true,
        hint: None,
        quick_fix: None,
        class: DiagnosticClass::Message,
        related: None,
    }
}

fn edition_diagnostic(source: &str, finding: &serde_json::Value) -> SourceDiagnostic {
    let loc = &finding["loc"];
    let position = |line: &str, column: &str| {
        let number = |key: &str| {
            u32::try_from(loc[key].as_u64().expect("edition location"))
                .expect("source position fits u32")
        };
        point_span(source, number(line), number(column)).start
    };
    message_diagnostic(
        "migration",
        finding["code"].as_str().expect("edition code"),
        finding["message"]
            .as_str()
            .expect("edition message")
            .to_owned(),
        Span {
            start: position("line", "column"),
            end: position("end_line", "end_column"),
        },
    )
}

fn migration_diagnostics(source: &str, options: fsl_syntax::ParseOptions) -> Vec<SourceDiagnostic> {
    fsl_syntax::canonical_rewrites_with(source, &options).map_or_else(
        |_| Vec::new(),
        |rewrites| {
            rewrites
                .into_iter()
                .filter(|rewrite| rewrite.kind == CanonicalRewriteKind::LogicalOperator)
                .map(|rewrite| {
                    message_diagnostic(
                        "migration",
                        "legacy_logical_operator",
                        "legacy logical operator spelling is non-canonical".to_owned(),
                        rewrite.span,
                    )
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
