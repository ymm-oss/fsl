// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
#[path = "../../fslc/tests/support/issue_1463_aggregate_and_component_locations.rs"]
mod forms;

#[path = "../../fslc/tests/support/issue_1463_nested_aggregates_and_sync_ownership.rs"]
mod nested_forms;

#[test]
fn aggregate_and_component_editor_diagnostics() {
    check_cases(&forms::cases(), "aggregate-and-component-locations");
}

#[test]
fn nested_aggregate_bounds_and_sync_editor_diagnostics() {
    check_cases(
        &nested_forms::cases(),
        "nested-aggregates-and-sync-ownership",
    );
}

fn check_cases(cases: &[forms::Case], label: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1463-{label}-lsp-{}",
        std::process::id()
    ));
    for (index, case) in cases.iter().enumerate() {
        let path = case.materialize(&root, index);
        let resolver = fsl_core::FsResolver::new(path.parent().unwrap());
        let diagnostics = fsl_lsp::diagnostics(
            &case.source,
            "root.fsl",
            &resolver,
            &fsl_syntax::ParseOptions::stable(),
        );
        if case.rejection {
            assert_eq!(diagnostics.len(), 1, "{}: {diagnostics:?}", case.id);
            let diagnostic = &diagnostics[0];
            let (line, column) = case.location();
            assert_eq!(
                (diagnostic.span.start.line, diagnostic.span.start.column),
                (line, column),
                "{}: {diagnostic:?}",
                case.id
            );
            assert_eq!(diagnostic.kind, "semantics");
            assert_eq!(diagnostic.code, "FSL-SEMANTIC");
            nested_forms::assert_message(case, &diagnostic.message);
        } else {
            assert!(
                diagnostics.iter().all(|d| d.kind == "migration"),
                "{}: {diagnostics:?}",
                case.id
            );
        }
    }
}
