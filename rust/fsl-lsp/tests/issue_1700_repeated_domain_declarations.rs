// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1700: a `domain` declaration repeated in a way that loses a copy is a diagnostic at
//! the repeat.
use fsl_core::FsResolver;

fn diagnostics(source: &str) -> Vec<fsl_lsp::SourceDiagnostic> {
    fsl_lsp::diagnostics(
        source,
        "root.fsl",
        &FsResolver::new("."),
        &fsl_syntax::ParseOptions::stable(),
    )
}

fn domain(fields: &str) -> String {
    format!(
        "domain D {{\n  type Q = 0..2\n  aggregate A {{\n    state {{ n: Q = 0; }}\n    \
         command T {{ a: Q; }}\n    event E {{ {fields} }}\n    decide T {{ emits E }}\n    \
         evolve E {{\n      n = a\n    }}\n  }}\n}}\n"
    )
}

#[test]
fn a_repeated_field_is_a_name_diagnostic_at_the_field() {
    let found = diagnostics(&domain("a: Q; a: Bool;"));
    assert_eq!(found.len(), 1, "{found:?}");
    let diagnostic = &found[0];
    assert_eq!(diagnostic.kind, "name", "{found:?}");
    // A domain refusal carries no diagnostic code, so it publishes the default,
    // as a repeated enum member does.
    assert_eq!(diagnostic.code, "FSL-SEMANTIC", "{found:?}");
    assert_eq!(
        diagnostic.message, "duplicate event field 'E.a' at root.fsl:6:21",
        "{found:?}"
    );
    assert_eq!(
        (
            diagnostic.span.start.line,
            diagnostic.span.start.column,
            diagnostic.span.end.line,
            diagnostic.span.end.column
        ),
        (6, 21, 6, 22),
        "{found:?}"
    );
}

#[test]
fn a_repeated_enum_member_publishes_the_same_kind_and_code() {
    let found = diagnostics("domain D {\n  enum Q { A, A }\n}\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "name", "{found:?}");
    assert_eq!(found[0].code, "FSL-SEMANTIC", "{found:?}");
}

#[test]
fn a_repeated_evolve_is_a_name_diagnostic_at_the_repeat() {
    let source = domain("a: Q;").replace(
        "    evolve E {",
        "    evolve E {\n      n = 0\n    }\n    evolve E {",
    );
    let found = diagnostics(&source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "name", "{found:?}");
    assert_eq!(
        (found[0].span.start.line, found[0].span.start.column),
        (11, 5),
        "{found:?}"
    );
}

#[test]
fn a_repeated_clause_is_a_parse_diagnostic_at_the_repeat() {
    let source = domain("a: Q;").replace(
        "  aggregate A {\n",
        "  aggregate A {\n    id AId\n    id OtherId\n",
    );
    let found = diagnostics(&source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "parse", "{found:?}");
    assert_eq!(
        (found[0].span.start.line, found[0].span.start.column),
        (5, 5),
        "{found:?}"
    );
}

#[test]
fn an_event_that_repeats_nothing_has_no_diagnostic() {
    let found = diagnostics(&domain("a: Q;"));
    assert!(found.is_empty(), "{found:?}");
}
