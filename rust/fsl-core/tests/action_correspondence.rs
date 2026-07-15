// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    ActionCorrespondenceTarget, CoreError, CorrespondenceOrigin, FileResolver, FsResolver,
    build_model, parse_kernel_source, parse_refinement, requirements_implements,
};

fn build(source: &str) -> fsl_core::KernelModel {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
    build_model(kernel).expect("build model")
}

struct Resolver(&'static str);

impl FileResolver for Resolver {
    fn read(&self, _: &str) -> Result<String, CoreError> {
        Ok(self.0.to_owned())
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
    assert!(error.message.contains("has 1 arguments, expected 2"));
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
    let error = requirements_implements(source, &Resolver(ABS), &implementation)
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
    let explicit_contract = requirements_implements(explicit, &Resolver(ABS), &build(explicit))
        .expect("explicit implements route")
        .expect("implements contract");
    let inline_contract = requirements_implements(inline, &Resolver(ABS), &build(inline))
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
    let error = requirements_implements(source, &Resolver(abstraction), &implementation)
        .expect_err("arity mismatch must be diagnosed");
    assert!(error.message.contains("has 1 arguments, expected 2"));
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
    let error = requirements_implements(source, &Resolver(abstraction), &implementation)
        .expect_err("auto-mapped actors must match");
    assert!(error.message.contains("actor mismatch"));
    assert!(error.message.contains("System"));
    assert!(error.message.contains("Manager"));
}
