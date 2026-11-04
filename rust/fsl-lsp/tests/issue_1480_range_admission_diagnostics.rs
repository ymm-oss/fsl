// SPDX-License-Identifier: Apache-2.0
//! #1480: editor diagnostics refuse the range forms `check` refuses, at the same place.
use fsl_core::FsResolver;

fn diagnostics(source: &str) -> Vec<fsl_lsp::SourceDiagnostic> {
    fsl_lsp::diagnostics(
        source,
        "root.fsl",
        &FsResolver::new("."),
        &fsl_syntax::ParseOptions::stable(),
    )
}

const PREFIX: &str =
    "spec M { type T = 0..3 state { n: T } init { n = 0 } action a() { requires n < 3 n = n + 1 } ";

#[test]
fn a_refused_range_form_is_a_located_semantic_diagnostic() {
    for (item, message) in [
        (
            "reachable R { forall i in 0..n { i >= 0 } }",
            "'n' is not an integer const",
        ),
        (
            "reachable R { count(i in 0..n) >= 0 }",
            "invalid model expression: 'n' is not an integer const",
        ),
        (
            "invariant P { forall i: Int { i == i } }",
            "type 'Int' is not a finite scalar",
        ),
    ] {
        let source = format!("{PREFIX}{item} }}");
        let found = diagnostics(&source);
        assert_eq!(found.len(), 1, "{item}: {found:?}");
        assert_eq!(found[0].kind, "semantics", "{item}");
        assert_eq!(found[0].code, "FSL-SEMANTIC", "{item}");
        assert!(found[0].message.contains(message), "{item}: {found:?}");
        assert_eq!(found[0].span.start.line, 1, "{item}");
        assert_eq!(
            found[0].span.start.column,
            u32::try_from(PREFIX.len() + 1).unwrap(),
            "{item}: {found:?}"
        );
    }
}

#[test]
fn the_same_bound_where_a_frame_certifies_it_has_no_diagnostic() {
    for item in [
        "invariant P { forall i in 0..n { i >= 0 } }",
        "trans P { count(i in 0..n) >= 0 }",
        "reachable R { forall i in 0..3 { i >= 0 } }",
    ] {
        let found = diagnostics(&format!("{PREFIX}{item} }}"));
        assert!(
            found.iter().all(|d| d.kind == "migration"),
            "{item}: {found:?}"
        );
    }
}

#[test]
fn a_declared_bool_type_is_a_diagnostic() {
    let found = diagnostics(
        "spec M { enum Bool { A, B } state { n: 0..1 } init { n = 0 } action a() { n = n } }",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("reserved FSL type name"),
        "{found:?}"
    );
}
