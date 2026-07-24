// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, TypeRef, build_model, parse_kernel_source, parse_refinement,
    public_kernel_expression,
};
use fsl_syntax::{SourcePos, Span};

fn build(source: &str) -> fsl_core::KernelModel {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
    build_model(kernel).expect("build model")
}

fn models() -> (fsl_core::KernelModel, fsl_core::KernelModel) {
    (
        build(
            "spec Impl { enum ImplStage { Conflict, Loaded, Received } state { stage: ImplStage } init { stage = Received } action load() { requires stage == Received stage = Loaded } }",
        ),
        build(
            "spec Abs { enum AbsStage { Received, Loaded, Conflict } state { status: AbsStage } init { status = Received } action load() { requires status == Received status = Loaded } }",
        ),
    )
}

const MAPPING: &str = r"refinement R {
  impl Impl
  abs Abs
  enum conversion stage ImplStage -> AbsStage {
    Received -> Received
    Loaded -> Loaded
    Conflict -> Conflict
  }
  map status = convert(stage, stage)
  action load() -> load()
}";

#[test]
fn exhaustive_conversion_keeps_nominal_identity_in_checked_and_public_expressions() {
    let (implementation, abstraction) = models();
    let refinement = parse_refinement(MAPPING, &implementation, &abstraction)
        .expect("same-spelled members convert explicitly");
    let expression = &refinement.state_maps["status"].expr;
    let rendered = fsl_core::expr_text(expression);
    assert!(rendered.contains("ImplStage.Received"), "{rendered}");
    assert!(rendered.contains("AbsStage.Received"), "{rendered}");

    let mut context = implementation.clone();
    context.types.extend(abstraction.types.clone());
    let position = SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    let public = public_kernel_expression(
        expression,
        &context,
        "mapping.fsl",
        Span {
            start: position,
            end: position,
        },
        Some(&TypeRef::Named("AbsStage".to_owned())),
    )
    .expect("elaborated conversion has an existing Public Kernel representation");
    let public = public.to_string();
    assert!(public.contains(r#""name":"ImplStage""#), "{public}");
    assert!(public.contains(r#""name":"AbsStage""#), "{public}");
    assert!(!public.contains("enum_convert"), "{public}");
}

#[test]
fn conversion_declaration_fails_closed_for_incomplete_unknown_and_duplicate_members() {
    let (implementation, abstraction) = models();
    for (source, expected) in [
        (
            MAPPING.replace("    Conflict -> Conflict\n", ""),
            "missing source: [Conflict]; missing target: [Conflict]",
        ),
        (
            MAPPING.replace("Conflict -> Conflict", "Missing -> Conflict"),
            "unknown enum member 'ImplStage.Missing'",
        ),
        (
            MAPPING.replace("Conflict -> Conflict", "Conflict -> Missing"),
            "unknown enum member 'AbsStage.Missing'",
        ),
        (
            MAPPING.replace("Conflict -> Conflict", "Received -> Conflict"),
            "maps source member 'Received' more than once",
        ),
        (
            MAPPING.replace("Conflict -> Conflict", "Conflict -> Loaded"),
            "maps target member 'Loaded' more than once",
        ),
    ] {
        let error = parse_refinement(&source, &implementation, &abstraction)
            .expect_err("invalid conversion must fail statically");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(error.span.is_some(), "diagnostic must retain a location");
    }

    let implicit = parse_refinement(
        "refinement R { impl Impl abs Abs map status = stage action load() -> load() }",
        &implementation,
        &abstraction,
    )
    .expect_err("distinct nominal enums remain incompatible without conversion");
    assert!(implicit.message.contains("is not assignable"));

    for (implementation, abstraction, rows, expected) in [
        (
            build(
                "spec Impl { enum ImplStage { A, B, C } state { stage: ImplStage } init { stage = A } }",
            ),
            build(
                "spec Abs { enum AbsStage { X, Y } state { status: AbsStage } init { status = X } }",
            ),
            "A -> X B -> Y",
            "missing source: [C]; missing target: []",
        ),
        (
            build(
                "spec Impl { enum ImplStage { A, B } state { stage: ImplStage } init { stage = A } }",
            ),
            build(
                "spec Abs { enum AbsStage { X, Y, Z } state { status: AbsStage } init { status = X } }",
            ),
            "A -> X B -> Y",
            "missing source: []; missing target: [Z]",
        ),
    ] {
        let source = format!(
            "refinement R {{ impl Impl abs Abs enum conversion stage ImplStage -> AbsStage {{ {rows} }} }}"
        );
        let error = parse_refinement(&source, &implementation, &abstraction)
            .expect_err("source-only and target-only omissions must fail closed");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(error.span.is_some());
    }

    for (source, expected) in [
        (
            MAPPING.replace("convert(stage, stage)", "convert(stage)"),
            "convert expects exactly two arguments",
        ),
        (
            MAPPING.replace("convert(stage, stage)", "convert(missing, stage)"),
            "unknown enum conversion 'missing'",
        ),
    ] {
        let error = parse_refinement(&source, &implementation, &abstraction)
            .expect_err("malformed conversion calls must fail statically");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(error.span.is_some());
    }
}

#[test]
fn conversion_calls_are_checked_in_action_arguments() {
    let implementation = build(
        "spec Impl { enum ImplStage { A, B } state { stage: ImplStage } init { stage = A } action send(s: ImplStage) { stage = s } }",
    );
    let abstraction = build(
        "spec Abs { enum AbsStage { A, B } state { status: AbsStage } init { status = A } action send(s: AbsStage) { status = s } }",
    );
    parse_refinement(
        "refinement R { impl Impl abs Abs enum conversion stage ImplStage -> AbsStage { A -> A B -> B } map status = convert(stage, stage) action send(s) -> send(convert(stage, s)) }",
        &implementation,
        &abstraction,
    )
    .expect("action argument uses the shared checked conversion");
}

#[test]
fn merged_context_rejects_a_bare_member_shared_by_distinct_nominal_enums() {
    let implementation = build(
        "spec Impl { enum AImplStage { Shared, Left } state { stage: AImplStage } init { stage = Shared } }",
    );
    let abstraction = build(
        "spec Abs { enum ZAbsStage { Shared, Zero, One } state { status: ZAbsStage } init { status = Zero } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs map status = if stage == Shared then Zero else One }",
        &implementation,
        &abstraction,
    )
    .expect_err("a merge must not reinterpret a checked impl member as an abs member");

    assert!(
        error.message.contains("ambiguous enum member 'Shared'"),
        "{}",
        error.message
    );
    assert!(error.message.contains("AImplStage"), "{}", error.message);
    assert!(error.message.contains("ZAbsStage"), "{}", error.message);
    assert!(error.message.contains("bijective enum conversion"));
    assert!(error.message.contains("issue #455"));
    assert!(
        error.span.is_some(),
        "diagnostic must retain the map location"
    );
}

#[test]
fn abstraction_constants_cannot_shadow_a_merged_enum_collision() {
    let implementation = build(
        "spec Impl { enum AImplStage { Shared, Left } state { stage: AImplStage } init { stage = Shared } }",
    );
    let abstraction = build(
        "spec Abs { const Shared = 0 enum ZAbsStage { Shared, Zero, One } state { status: ZAbsStage } init { status = One } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs map status = if Shared == 0 then Zero else One }",
        &implementation,
        &abstraction,
    )
    .expect_err("mapping expressions may use implementation constants, not abstraction constants");

    assert!(
        error.message.contains("ambiguous enum member 'Shared'"),
        "{}",
        error.message
    );
    assert!(error.message.contains("AImplStage"), "{}", error.message);
    assert!(error.message.contains("ZAbsStage"), "{}", error.message);
    assert!(error.span.is_some());
}

#[test]
fn ordinary_identifiers_keep_precedence_over_ambiguous_enum_members() {
    let abstraction = build(
        "spec Abs { type Key = 0..1 enum ZAbsStage { Shared, Zero } state { ready: Bool, flags: Map<Key, Key> } init { ready = false forall k: Key { flags[k] = 0 } } }",
    );

    for (implementation, mapping) in [
        (
            build(
                "spec Impl { enum AImplStage { Shared, Left } state { Shared: Bool } init { Shared = true } }",
            ),
            "refinement R { impl Impl abs Abs map ready = Shared map flags[b: Key] = b }",
        ),
        (
            build(
                "spec Impl { const Shared = true enum AImplStage { Shared, Left } state { ready: Bool } init { ready = false } }",
            ),
            "refinement R { impl Impl abs Abs map ready = Shared map flags[b: Key] = b }",
        ),
        (
            build(
                "spec Impl { enum AImplStage { Shared, Left } state { ready: Bool } init { ready = false } }",
            ),
            "refinement R { impl Impl abs Abs map ready = ready map flags[Shared: Key] = Shared }",
        ),
    ] {
        parse_refinement(mapping, &implementation, &abstraction).expect(
            "implementation state/const and local bindings keep precedence over enum members",
        );
    }
}

#[test]
fn empty_enum_conversion_fails_closed_before_call_elaboration() {
    let implementation = build(
        "spec Impl { enum EmptyImpl {} state { ready: Bool } init { ready = false } action send(s: EmptyImpl) { requires false } }",
    );
    let abstraction = build(
        "spec Abs { enum EmptyAbs {} state { ready: Bool } init { ready = false } action send(s: EmptyAbs) { requires false } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs enum conversion empty EmptyImpl -> EmptyAbs {} action send(s) -> send(convert(empty, s)) }",
        &implementation,
        &abstraction,
    )
    .expect_err("empty conversions must not reach an infallible elaboration branch");
    assert!(error.message.contains("EmptyImpl' has no members"));
    assert!(error.span.is_some());
}
