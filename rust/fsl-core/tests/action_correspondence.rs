// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    ActionCorrespondenceTarget, CoreError, CorrespondenceOrigin, FileResolver, FsResolver,
    build_model, filter_scope_overrides_for_abstraction, parse_kernel_source,
    parse_kernel_source_with_bounds, parse_refinement, requirements_implements,
    requirements_implements_with_bounds,
};
use std::collections::BTreeMap;

fn build(source: &str) -> fsl_core::KernelModel {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
    build_model(kernel).expect("build model")
}

struct Resolver(String);

impl FileResolver for Resolver {
    fn read(&self, _: &str) -> Result<String, CoreError> {
        Ok(self.0.clone())
    }
}

const ABS: &str = "spec Abs { type N = 0..1 state { done: Bool } init { done = false } action settle(x: N) { done = true } }";
const IMPL: &str = "spec Impl { type N = 0..1 state { paid: Bool } init { paid = false } action pay(x: N, retry: N) { paid = true } }";

#[test]
fn standalone_correspondence_resolves_typed_params_once() {
    let mapping = parse_refinement(
        "refinement R { impl Impl abs Abs map done = paid action pay(x: N, retry: N) -> settle(x) }",
        &build(IMPL),
        &build(ABS),
    )
    .expect("typed correspondence");
    let correspondence = &mapping.action_correspondences["pay"];
    assert_eq!(correspondence.origin, CorrespondenceOrigin::RefinementFile);
    assert_eq!(correspondence.impl_params.len(), 2);
    assert!(matches!(
        &correspondence.target,
        ActionCorrespondenceTarget::Action { action, args }
            if action.0 == "settle" && args.len() == 1
    ));

    let error = parse_refinement(
        "refinement R { impl Impl abs Abs map done = paid action pay(x: Bool, retry: N) -> settle(x) }",
        &build(IMPL),
        &build(ABS),
    )
    .expect_err("typed params must match the implementation");
    assert!(error.message.contains("pay.x' type does not match"));

    let stutter = parse_refinement(
        "refinement R { impl Impl abs Abs map done = paid action pay(x: N, retry: N) -> stutter }",
        &build(IMPL),
        &build(ABS),
    )
    .expect("stutter correspondence");
    assert!(matches!(
        stutter.action_correspondences["pay"].target,
        ActionCorrespondenceTarget::Stutter
    ));
}

#[test]
fn maps_auto_uses_the_common_arity_and_type_checks() {
    let implementation = build(
        "spec Impl { type N = 0..1 state { flag: Bool } init { flag = false } action go(x: N) { flag = true } }",
    );
    let abstraction = build(
        "spec Abs { type N = 0..1 state { flag: Bool } init { flag = false } action go(x: N, y: N) { flag = true } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect_err("incompatible same-name actions must fail during auto lowering");
    assert!(
        error
            .message
            .contains("impl has 1 parameter(s), abstract has 2")
    );
}

/// Issue #494: `maps auto` must never guess a binding for an incompatible
/// same-name action pair — a surplus impl parameter, a renamed parameter, or
/// two same-typed parameters neither of which name-matches must all be a
/// located `kind: "type"` error, never a silently dropped/positionally
/// guessed correspondence.
#[test]
fn maps_auto_rejects_incompatible_parameters_instead_of_guessing() {
    let go_abstraction = "spec Abs { type K = 0..2 state { seen: K } init { seen = 0 } action go(wanted: K) { seen = wanted } }";

    // Surplus: impl has an extra parameter ('extra') with no counterpart on
    // the abs side, and the one shared position is also renamed ('a' vs
    // 'wanted'). The old code silently bound wanted <- a positionally and
    // dropped 'extra' entirely.
    let surplus_implementation = build(
        "spec Impl { type K = 0..2 state { picked: K } init { picked = 0 } action go(a: K, extra: K) { picked = a } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &surplus_implementation,
        &build(go_abstraction),
    )
    .expect_err("a surplus impl parameter must not be silently dropped");
    assert!(
        error
            .message
            .contains("impl has 2 parameter(s), abstract has 1")
    );

    // Renamed: same arity (1 vs 1), but the impl parameter name does not
    // match the abstract parameter name at all.
    let renamed_implementation = build(
        "spec Impl { type K = 0..2 state { picked: K } init { picked = 0 } action go(a: K) { picked = a } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &renamed_implementation,
        &build(go_abstraction),
    )
    .expect_err("a renamed parameter must not be matched positionally");
    assert!(error.message.contains("no impl parameter named 'wanted'"));

    // Ambiguous same-typed: same arity (2 vs 2), both impl parameters share
    // the abstract parameters' type, but neither impl name matches either
    // abstract name. A type-based positional fallback would have to guess
    // which same-typed parameter goes where; the contract forbids it.
    let two_abstraction = "spec Abs { type K = 0..2 state { seen: K } init { seen = 0 } action go(first: K, second: K) { seen = first } }";
    let ambiguous_implementation = build(
        "spec Impl { type K = 0..2 state { picked: K } init { picked = 0 } action go(x: K, y: K) { picked = x } }",
    );
    let error = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &ambiguous_implementation,
        &build(two_abstraction),
    )
    .expect_err("two same-typed but differently-named parameters must not be guessed");
    assert!(error.message.contains("no impl parameter named 'first'"));

    // Positive control: a legitimately auto-mappable pair, including a pure
    // reorder (impl b,a vs abs a,b — same names, different order), must
    // still auto-map by name. Losing this would be its own regression.
    let reordered_implementation = build(
        "spec Impl { type K = 0..2 state { seen: K } init { seen = 0 } action go(second: K, first: K) { seen = first } }",
    );
    let mapping = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &reordered_implementation,
        &build(two_abstraction),
    )
    .expect("a same-named, reordered parameter pair auto-maps by name");
    let correspondence = &mapping.action_correspondences["go"];
    assert!(matches!(
        &correspondence.target,
        ActionCorrespondenceTarget::Action { action, args }
            if action.0 == "go"
                && args.len() == 2
                && args[0] == fsl_core::KernelExpr::Var("first".to_owned())
                && args[1] == fsl_core::KernelExpr::Var("second".to_owned())
    ));
}

#[test]
fn requirements_routes_report_both_duplicate_origins() {
    let source = r#"requirements Impl {
  implements Abs from "abs.fsl" {
    map done = paid
    action pay(x: N, retry: N) -> settle(x)
  }
  type N = 0..1
  state { paid: Bool }
  init { paid = false }
  action pay(x: N, retry: N) maps settle(x) { paid = true }
}"#;
    let implementation = build(source);
    let error = requirements_implements(source, &Resolver(ABS.to_owned()), &implementation)
        .expect_err("the implementation block and maps clause conflict");
    assert!(error.message.contains("implements_block"));
    assert!(error.message.contains("inline_maps_clause"));
    assert!(error.message.contains("conflicts with"));

    let standalone = parse_refinement(
        "refinement R { impl Impl abs Abs map done = paid action pay(wrong: Bool, retry: N) -> settle(wrong) action pay(x: N, retry: N) -> stutter }",
        &build(IMPL),
        &build(ABS),
    )
    .expect_err("duplicates are diagnosed before either entry is resolved");
    assert!(
        standalone
            .message
            .contains("duplicate action correspondence")
    );
    assert!(standalone.message.contains("refinement_file"));
}

#[test]
fn requirements_routes_lower_to_the_same_typed_target() {
    let explicit = r#"requirements Impl {
  implements Abs from "abs.fsl" {
    map done = paid
    action pay(x: N, retry: N) -> settle(x)
  }
  type N = 0..1
  state { paid: Bool }
  init { paid = false }
  action pay(x: N, retry: N) { paid = true }
}"#;
    let inline = r#"requirements Impl {
  implements Abs from "abs.fsl" { map done = paid }
  type N = 0..1
  state { paid: Bool }
  init { paid = false }
  action pay(x: N, retry: N) maps settle(x) { paid = true }
}"#;
    let explicit_contract =
        requirements_implements(explicit, &Resolver(ABS.to_owned()), &build(explicit))
            .expect("explicit implements route")
            .expect("implements contract");
    let inline_contract =
        requirements_implements(inline, &Resolver(ABS.to_owned()), &build(inline))
            .expect("inline maps route")
            .expect("implements contract");
    let explicit_mapping = &explicit_contract.refinement.action_correspondences["pay"];
    let inline_mapping = &inline_contract.refinement.action_correspondences["pay"];
    assert_eq!(explicit_mapping.target, inline_mapping.target);
    assert_eq!(
        explicit_mapping.origin,
        CorrespondenceOrigin::ImplementsBlock
    );
    assert_eq!(
        inline_mapping.origin,
        CorrespondenceOrigin::InlineMapsClause
    );
}

#[test]
fn requirements_implicit_auto_returns_an_error_instead_of_indexing_past_params() {
    let abstraction = "spec Abs { type N = 0..1 state { done: Bool } init { done = false } action pay(x: N, y: N) { done = true } }";
    let source = r#"requirements Impl {
  implements Abs from "abs.fsl" { map done = paid }
  type N = 0..1
  state { paid: Bool }
  init { paid = false }
  action pay(x: N) { paid = true }
}"#;
    let implementation = build(source);
    let error = requirements_implements(source, &Resolver(abstraction.to_owned()), &implementation)
        .expect_err("arity mismatch must be diagnosed");
    assert!(
        error
            .message
            .contains("impl has 1 parameter(s), abstract has 2")
    );
}

#[test]
fn auto_correspondence_checks_process_actor_compatibility() {
    let abstraction = r"business Abs {
  actor Manager
  entity Case
  process Case {
    stages Open, Done
    initial Open
    transition approve Open -> Done by Manager
  }
}
verify { instances Case = 2 }
";
    let source = r#"requirements Impl {
  implements Abs from "abs.fsl" { }
  process Case {
    stages Open, Done
    initial Open
    transition approve Open -> Done by System
  }
}
verify { instances Case = 2 }
"#;
    let implementation = build(source);
    let error = requirements_implements(source, &Resolver(abstraction.to_owned()), &implementation)
        .expect_err("auto-mapped actors must match");
    assert!(error.message.contains("actor mismatch"));
    assert!(error.message.contains("System"));
    assert!(error.message.contains("Manager"));
}

/// detector (mutation A: empty abstraction overrides -> `map_out_of_bounds`;
/// mutation B: unfiltered overrides -> undeclared-name error on direct `spec`)
#[test]
fn bounds_are_filtered_and_propagated_to_inline_abstraction() {
    let spec_abs = r"spec SharedOnly {
  entity Item
  state { st: Map<Item, Bool> }
  init { forall i: Item { st[i] = false } }
  action finish(i: Item) {
    st[i] = true
  }
}
verify { instances Item = 3 }
";
    let spec_impl = r#"requirements SharedOnlyReq {
  implements SharedOnly from "abs.fsl" { maps auto }
  entity Item
  entity Extra
  state { st: Map<Item, Bool> }
  init { forall i: Item { st[i] = false } }
  action finish(i: Item) maps finish(i) {
    st[i] = true
  }
}
verify {
  instances Item = 1
  instances Extra = 1
}
"#;
    let overrides = BTreeMap::from([("Item".to_owned(), 1), ("Extra".to_owned(), 1)]);
    let filtered = filter_scope_overrides_for_abstraction(spec_abs, &overrides, &BTreeMap::new())
        .expect("filter overrides")
        .expect("shared override");
    assert_eq!(filtered.0, BTreeMap::from([("Item".to_owned(), 1)]));
    assert!(filtered.1.is_empty());

    let implementation = build(spec_impl);
    let contract = requirements_implements_with_bounds(
        spec_impl,
        &Resolver(spec_abs.to_owned()),
        &implementation,
        &overrides,
        &BTreeMap::new(),
    )
    .expect("filtered abstraction parse")
    .expect("implements contract");
    assert_eq!(contract.abstraction.name, "SharedOnly");

    let error = parse_kernel_source_with_bounds(spec_abs, &overrides, &BTreeMap::new())
        .expect_err("unfiltered overrides must fail validation on direct spec");
    assert!(
        error
            .message
            .contains("verify instances references undeclared entity 'Extra'")
    );
}
