// SPDX-License-Identifier: Apache-2.0
//! #1685: a struct literal that repeats a field is a parse error, so the editor
//! reports it at the repeated field with the parse code. Like any parse error it
//! takes the document's symbols with it until it is fixed.
use fsl_core::FsResolver;

fn diagnostics(source: &str) -> Vec<fsl_lsp::SourceDiagnostic> {
    fsl_lsp::diagnostics(
        source,
        "root.fsl",
        &FsResolver::new("."),
        &fsl_syntax::ParseOptions::stable(),
    )
}

fn spec(literal: &str) -> String {
    format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ x: K, y: K }}\n  state {{ p: P }}\n  \
         init {{ p = {literal} }}\n  action go() {{ p = p }}\n}}\n"
    )
}

#[test]
fn a_repeated_field_is_a_parse_diagnostic_at_the_field() {
    let found = diagnostics(&spec("P { x: 1, x: 2, y: 0 }"));
    assert_eq!(found.len(), 1, "{found:?}");
    let diagnostic = &found[0];
    assert_eq!(diagnostic.kind, "parse", "{found:?}");
    assert_eq!(diagnostic.code, "duplicate_struct_field", "{found:?}");
    assert_eq!(
        diagnostic.message, "duplicate field 'P.x' at 5:24",
        "{found:?}"
    );
    assert_eq!(
        (
            diagnostic.span.start.line,
            diagnostic.span.start.column,
            diagnostic.span.end.line,
            diagnostic.span.end.column
        ),
        (5, 24, 5, 25),
        "{found:?}"
    );
}

#[test]
fn a_literal_that_repeats_nothing_has_no_diagnostic() {
    let found = diagnostics(&spec("P { x: 1, y: 0 }"));
    assert!(found.is_empty(), "{found:?}");
}
