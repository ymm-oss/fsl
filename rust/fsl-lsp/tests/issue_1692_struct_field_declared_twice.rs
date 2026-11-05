// SPDX-License-Identifier: Apache-2.0
//! #1692: a struct that declares a field twice is a name diagnostic at the repeated
//! field, so the document keeps its symbols.
use fsl_core::FsResolver;

fn diagnostics(source: &str) -> Vec<fsl_lsp::SourceDiagnostic> {
    fsl_lsp::diagnostics(
        source,
        "root.fsl",
        &FsResolver::new("."),
        &fsl_syntax::ParseOptions::stable(),
    )
}

fn spec(fields: &str) -> String {
    format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ {fields} }}\n  state {{ p: P }}\n  \
         init {{ p = P {{ x: 1 }} }}\n  action go() {{ p = p }}\n}}\n"
    )
}

#[test]
fn a_repeated_field_is_a_name_diagnostic_at_the_field() {
    let found = diagnostics(&spec("x: K, x: Bool"));
    assert_eq!(found.len(), 1, "{found:?}");
    let diagnostic = &found[0];
    assert_eq!(diagnostic.kind, "name", "{found:?}");
    assert_eq!(diagnostic.code, "FSL-NAME", "{found:?}");
    assert_eq!(
        diagnostic.message, "duplicate struct field 'P.x'",
        "{found:?}"
    );
    assert_eq!(
        (
            diagnostic.span.start.line,
            diagnostic.span.start.column,
            diagnostic.span.end.line,
            diagnostic.span.end.column
        ),
        (3, 20, 3, 21),
        "{found:?}"
    );
}

#[test]
fn a_struct_that_repeats_nothing_has_no_diagnostic() {
    let found = diagnostics(&spec("x: K"));
    assert!(found.is_empty(), "{found:?}");
}
