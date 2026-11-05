// SPDX-License-Identifier: Apache-2.0
//! #1694: an editor refuses a second `implements` where `check` does.
use fsl_core::FsResolver;

const REPEATED: &str = "requirements R {\n  implements A from \"a.fsl\" { }\n  implements B from \"b.fsl\" { }\n  state { done: Bool }\n  init { done = false }\n  action finish() { done = true }\n}\n";

#[test]
fn a_second_clause_is_a_diagnostic_at_that_clause() {
    let diagnostics = fsl_lsp::diagnostics(
        REPEATED,
        "root.fsl",
        &FsResolver::new("."),
        &fsl_syntax::ParseOptions::stable(),
    );
    let refusal = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("duplicate implements 'B'"))
        .unwrap_or_else(|| panic!("{diagnostics:?}"));
    assert!(refusal.located, "{refusal:?}");
    assert_eq!(
        (refusal.span.start.line, refusal.span.start.column),
        (3, 3),
        "{refusal:?}"
    );
}
