// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, TypeRef, build_model, parse_kernel_source};

fn build(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    build_model(kernel)
}

#[test]
fn action_parameters_accept_only_finite_scalar_types() {
    let model = build(
        "spec Valid { state { done: Bool } init { done = false } action apply(v: Bool) { } }",
    )
    .expect("Bool action parameter is finite");
    assert!(matches!(
        model.actions[0].params[0],
        fsl_core::ParamDef::Typed {
            ty: TypeRef::Bool,
            ..
        }
    ));

    for source in [
        "spec Invalid { state { done: Bool } init { done = false } action apply(v: Int) { } }",
        "spec Invalid { struct Payload { value: Int, } state { done: Bool } init { done = false } action apply(v: Payload) { } }",
    ] {
        let error = build(source).expect_err("unbounded or non-scalar parameter must fail");
        assert!(
            error.message.contains("finite scalar"),
            "unexpected error: {error:?}"
        );
    }
}

#[test]
fn action_parameters_reject_duplicate_names_and_empty_inline_ranges() {
    for (source, expected) in [
        (
            "spec Duplicate { type Small = 0..1 state { done: Bool } init { done = false } action apply(v: Small, v: Small) { } }",
            "duplicate action parameter 'v'",
        ),
        (
            "spec Empty { state { done: Bool } init { done = false } action apply(v in 1..0) { } }",
            "action parameter range is empty",
        ),
    ] {
        let error = build(source).expect_err("ambiguous or empty parameter domain must fail");
        assert!(
            error.message.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn actions_reject_duplicate_names_before_runtime_dispatch() {
    let error = build(
        "spec DuplicateAction { state { done: Bool } init { done = false } action change() { requires not done done = true } action change() { requires done done = false } }",
    )
    .expect_err("name-based runtime dispatch requires unique actions");

    assert!(
        error.message.contains("duplicate action 'change'"),
        "unexpected error: {error:?}"
    );
}

/// Hand-built or generated actions carry no formal spans; the refusal then falls
/// back to the action instead of losing its location (#1680).
#[test]
fn duplicate_formal_without_authored_spans_points_at_the_action() {
    let source = "spec Duplicate { type Small = 0..1 state { done: Bool } init { done = false }\n  action apply(a: Small, v: Small, v: Small) { } }";
    let parse = || {
        let fsl_syntax::SurfaceDocument::Spec(spec) =
            fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source))
                .expect("parse")
                .surface
        else {
            panic!("expected spec")
        };
        spec
    };
    let located = |spec| {
        let error = fsl_core::build_surface_model(spec).expect_err("repeated formal");
        assert!(
            error.message.contains("duplicate action parameter 'v'"),
            "{error:?}"
        );
        let span = error.span.expect("located");
        (span.start.line, span.start.column)
    };
    assert_eq!(located(parse()), (2, 36), "the repeated name");
    let mut spec = parse();
    for item in &mut spec.items {
        if let fsl_syntax::SpecItem::Action { param_spans, .. } = item {
            param_spans.clear();
        }
    }
    assert_eq!(located(spec), (2, 3), "the action");
}
