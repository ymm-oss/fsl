// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{CanonicalRewriteKind, SourcePos, Span, SurfaceDocument};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDiagnostic {
    pub kind: &'static str,
    pub code: String,
    pub message: String,
    pub span: Span,
}

/// Run the authoritative syntax and typed-model gates and return editor diagnostics.
#[must_use]
pub fn diagnostics(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
) -> Vec<SourceDiagnostic> {
    diagnostics_with_model(source, source_file, resolver).0
}

/// Run frontend diagnostics and retain the checked model for analysis consumers.
#[must_use]
pub fn diagnostics_with_model(
    source: &str,
    source_file: &str,
    resolver: &dyn fsl_core::FileResolver,
) -> (Vec<SourceDiagnostic>, Option<fsl_core::KernelModel>) {
    if crate::frontend_output::is_ai_project(source) {
        return (migration_diagnostics(source), None);
    }
    // Standalone causal models bypass dialect dispatch (docs/DESIGN-causal.md):
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
                }],
                None,
            ),
        };
    }
    let parsed = match fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source)) {
        Ok(parsed) => parsed,
        Err(error) => {
            return (
                vec![SourceDiagnostic {
                    kind: "parse",
                    code: error.code().to_owned(),
                    message: error.to_string(),
                    span: error.span,
                }],
                None,
            );
        }
    };
    if matches!(
        parsed.surface,
        SurfaceDocument::Agent(_) | SurfaceDocument::Refinement(_)
    ) {
        return (migration_diagnostics(source), None);
    }
    let kernel = match fsl_core::parse_kernel_source_with_file(source, resolver, source_file) {
        Ok(kernel) => kernel,
        Err(error) => return (vec![core_diagnostic(source, &error)], None),
    };
    match fsl_core::build_model(kernel) {
        Ok(model) => (migration_diagnostics(source), Some(model)),
        Err(error) => (vec![model_diagnostic(source, &error)], None),
    }
}

fn core_diagnostic(source: &str, error: &fsl_core::CoreError) -> SourceDiagnostic {
    let message = error.to_string();
    SourceDiagnostic {
        kind: crate::verification_output::semantic_error_kind(&message),
        code: "FSL-SEMANTIC".to_owned(),
        message,
        span: error
            .origin
            .as_deref()
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span)
            .unwrap_or_else(|| point_span(source, error.line, error.column)),
    }
}

fn model_diagnostic(source: &str, error: &fsl_core::ModelError) -> SourceDiagnostic {
    let message = error.to_string();
    let kind = crate::verification_output::semantic_error_kind(&message);
    let span = error
        .origin
        .as_deref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .or_else(|| diagnostic_span_from_message(source, &message))
        .unwrap_or_else(|| point_span(source, 1, 1));
    SourceDiagnostic {
        kind,
        code: if kind == "type" {
            "FSL-TYPE".to_owned()
        } else {
            "FSL-SEMANTIC".to_owned()
        },
        message,
        span,
    }
}

fn diagnostic_span_from_message(source: &str, message: &str) -> Option<Span> {
    let quoted = message.split('\'').nth(1)?;
    fsl_syntax::lex(source).ok()?.into_iter().find_map(|token| {
        matches!(&token.kind, fsl_syntax::TokenKind::Ident(name) if name == quoted)
            .then_some(token.span)
    })
}

fn migration_diagnostics(source: &str) -> Vec<SourceDiagnostic> {
    fsl_syntax::canonical_rewrites(source).map_or_else(
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
                    }
                })
                .collect()
        },
    )
}

fn point_span(source: &str, line: u32, column: u32) -> Span {
    let offset = source
        .lines()
        .take(usize::try_from(line.saturating_sub(1)).expect("line fits usize"))
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + usize::try_from(column.saturating_sub(1)).expect("column fits usize");
    let point = SourcePos {
        offset: offset.min(source.len()),
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
        let found = diagnostics(source, "broken.fsl", &fsl_core::FsResolver::new("."));
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
        let found = diagnostics(source, "broken.fsl", &fsl_core::FsResolver::new("."));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "type");
        assert!(found[0].message.contains("unknown type 'Missing'"));
        assert_eq!(found[0].span.start.line, 2);
    }

    #[test]
    fn accepts_native_annotations_without_python() {
        let source = r#"spec Annotated {
  state { ready: Bool }
  init { ready = false }
  @undecided("pending")
  invariant Ready { ready }
}"#;
        assert!(diagnostics(source, "annotated.fsl", &fsl_core::FsResolver::new(".")).is_empty());
    }

    #[test]
    fn exposes_authoritative_migration_rewrite() {
        let source = r"domain Orders {
  type Status = Pending | Approved
  aggregate Order { state { status: Status = Pending; } }
}";
        let found = diagnostics(source, "orders.fsl", &fsl_core::FsResolver::new("."));
        assert!(
            found
                .iter()
                .any(|item| item.code == "deprecated_domain_enum_union")
        );
    }
}
