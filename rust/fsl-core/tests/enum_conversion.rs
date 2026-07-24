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

const ABSTRACTION: &str = r"refinement R {
  impl Impl
  abs Abs
  enum abstraction stage ImplStage -> AbsStage {
    Received -> Received
    Loaded -> Loaded
    Conflict -> Loaded
  }
  map status = abstract(stage, stage)
  action load() -> load()
}";

#[test]
fn source_total_abstraction_allows_repeated_and_unused_targets() {
    let (implementation, abstraction) = models();
    let mapping = parse_refinement(ABSTRACTION, &implementation, &abstraction)
        .expect("many-to-one abstraction is source-total");
    let rendered = fsl_core::expr_text(&mapping.state_maps["status"].expr);
    assert!(rendered.contains("ImplStage.Conflict"), "{rendered}");
    assert!(rendered.contains("AbsStage.Loaded"), "{rendered}");
    let mut context = implementation.clone();
    context.types.extend(abstraction.types.clone());
    let position = SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    let public = public_kernel_expression(
        &mapping.state_maps["status"].expr,
        &context,
        "mapping.fsl",
        Span {
            start: position,
            end: position,
        },
        Some(&TypeRef::Named("AbsStage".to_owned())),
    )
    .expect("abstraction uses existing typed Public Kernel expressions")
    .to_string();
    assert!(public.contains(r#""name":"ImplStage""#), "{public}");
    assert!(public.contains(r#""name":"AbsStage""#), "{public}");
    assert!(!public.contains("enum_abstraction"), "{public}");
    assert!(!public.contains("abstract"), "{public}");

    let reversed_implementation = build(
        "spec Impl { enum ImplStage { Received, Loaded, Conflict } state { stage: ImplStage } init { stage = Received } action load() { requires stage == Received stage = Loaded } }",
    );
    let reversed_abstraction = build(
        "spec Abs { enum AbsStage { Conflict, Loaded, Received } state { status: AbsStage } init { status = Received } action load() { requires status == Received status = Loaded } }",
    );
    let reversed = parse_refinement(ABSTRACTION, &reversed_implementation, &reversed_abstraction)
        .expect("declaration order does not affect nominal mapping");
    assert_eq!(
        fsl_core::expr_text(&mapping.state_maps["status"].expr),
        fsl_core::expr_text(&reversed.state_maps["status"].expr)
    );
}

#[test]
fn source_total_abstraction_rejects_incomplete_or_wrong_nominal_sources() {
    let (implementation, abstraction) = models();
    for (source, expected) in [
        (
            ABSTRACTION.replace("    Conflict -> Loaded\n", ""),
            "missing source: [Conflict]",
        ),
        (
            ABSTRACTION.replace("Conflict -> Loaded", "Received -> Loaded"),
            "maps source member 'Received' more than once",
        ),
        (
            ABSTRACTION.replace("Conflict -> Loaded", "Missing -> Loaded"),
            "unknown enum member 'ImplStage.Missing'",
        ),
        (
            ABSTRACTION.replace("Conflict -> Loaded", "Conflict -> Missing"),
            "unknown enum member 'AbsStage.Missing'",
        ),
    ] {
        let error = parse_refinement(&source, &implementation, &abstraction)
            .expect_err("invalid abstraction must fail statically");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(error.span.is_some());
    }

    let wrong_nominal = build(
        "spec Impl { enum ImplStage { Conflict, Loaded, Received } enum OtherStage { X, Y, Z } state { stage: ImplStage } init { stage = Received } action load() { requires stage == Received stage = Loaded } }",
    );
    let source = r"refinement R {
      impl Impl
      abs Abs
      enum abstraction other OtherStage -> AbsStage {
        X -> Received
        Y -> Loaded
        Z -> Loaded
      }
      map status = abstract(other, stage)
      action load() -> load()
    }";
    let error = parse_refinement(source, &wrong_nominal, &abstraction)
        .expect_err("same-spelled members from another nominal enum remain incompatible");
    assert!(
        error.message.contains("is not assignable"),
        "{}",
        error.message
    );
    assert!(error.span.is_some());
}

#[test]
fn abstraction_and_conversion_calls_cannot_be_interchanged() {
    let (implementation, abstraction) = models();
    for (source, expected) in [
        (
            ABSTRACTION.replace("abstract(stage, stage)", "convert(stage, stage)"),
            "enum abstraction 'stage' must be invoked with abstract",
        ),
        (
            MAPPING.replace("convert(stage, stage)", "abstract(stage, stage)"),
            "enum conversion 'stage' must be invoked with convert",
        ),
        (
            ABSTRACTION.replace("abstract(stage, stage)", "abstract(stage)"),
            "abstract expects exactly two arguments",
        ),
        (
            ABSTRACTION.replace("abstract(stage, stage)", "abstract(missing, stage)"),
            "unknown enum abstraction 'missing'",
        ),
    ] {
        let error = parse_refinement(&source, &implementation, &abstraction)
            .expect_err("assurance-specific calls fail closed");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(error.span.is_some());
    }

    let duplicate = ABSTRACTION.replace(
        "  enum abstraction stage",
        "  enum conversion stage ImplStage -> AbsStage { Received -> Received Loaded -> Loaded Conflict -> Conflict }\n  enum abstraction stage",
    );
    let error = parse_refinement(&duplicate, &implementation, &abstraction)
        .expect_err("conversion and abstraction names share one namespace");
    assert!(
        error.message.contains("duplicate enum mapping 'stage'"),
        "{}",
        error.message
    );
    assert!(error.span.is_some());
}

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
    assert!(error.message.contains("enum conversion/convert"));
    assert!(error.message.contains("enum abstraction/abstract"));
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

    let error = parse_refinement(
        "refinement R { impl Impl abs Abs enum abstraction empty EmptyImpl -> EmptyAbs {} action send(s) -> send(abstract(empty, s)) }",
        &implementation,
        &abstraction,
    )
    .expect_err("empty abstractions must not reach fallback-based elaboration");
    assert!(error.message.contains("EmptyImpl' has no members"));
    assert!(error.span.is_some());

    let non_empty_implementation = build(
        "spec Impl { enum NonEmptyImpl { A } state { ready: Bool } init { ready = false } action send(s: NonEmptyImpl) { ready = true } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs enum abstraction empty NonEmptyImpl -> EmptyAbs {} map ready = ready action send(s) -> send(abstract(empty, s)) }",
        &non_empty_implementation,
        &abstraction,
    )
    .expect_err("an empty abstraction target has no representable result");
    assert!(error.message.contains("EmptyAbs' has no members"));
    assert!(error.span.is_some());
}
