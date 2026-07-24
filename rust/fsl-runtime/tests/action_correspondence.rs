// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    CoreError, FileResolver, FsResolver, build_model, parse_kernel_source, parse_refinement,
    requirements_implements,
};
use fsl_runtime::check_refinement;

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

#[test]
fn explicit_and_auto_correspondences_have_the_same_concrete_verdict() {
    const ABS: &str =
        "spec Abs { type N = 0..1 state { x: N } init { x = 0 } action step(v: N) { x = v } }";
    let implementation = build(
        "spec Impl { type N = 0..1 state { x: N } init { x = 0 } action step(v: N) { x = v } }",
    );
    let abstraction = build(ABS);
    let explicit = parse_refinement(
        "refinement R { impl Impl abs Abs map x = x action step(v: N) -> step(v) }",
        &implementation,
        &abstraction,
    )
    .expect("explicit correspondence");
    let automatic = parse_refinement(
        "refinement R { impl Impl abs Abs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("automatic correspondence");

    let explicit_result = check_refinement(&implementation, &abstraction, &explicit, 2)
        .expect("explicit refinement check");
    let automatic_result = check_refinement(&implementation, &abstraction, &automatic, 2)
        .expect("automatic refinement check");
    assert_eq!(explicit_result, automatic_result);

    for source in [
        r#"requirements Impl {
  implements Abs from "abs.fsl" { map x = x action step(v: N) -> step(v) }
  type N = 0..1 state { x: N } init { x = 0 }
  action step(v: N) { x = v }
}"#,
        r#"requirements Impl {
  implements Abs from "abs.fsl" { map x = x }
  type N = 0..1 state { x: N } init { x = 0 }
  action step(v: N) maps step(v) { x = v }
}"#,
    ] {
        let routed_implementation = build(source);
        let contract = requirements_implements(source, &Resolver(ABS), &routed_implementation)
            .expect("requirements correspondence route")
            .expect("implements contract");
        let routed_result = check_refinement(
            &routed_implementation,
            &contract.abstraction,
            &contract.refinement,
            2,
        )
        .expect("routed refinement check");
        assert_eq!(explicit_result, routed_result);
    }
}

#[test]
fn enum_conversion_agrees_across_concrete_refinement_routes_without_ordinal_coercion() {
    const ABS: &str = "spec Abs { enum AbsStage { A, B, C } state { status: AbsStage } init { status = A } action step() { requires status == A status = B } }";
    let implementation = build(
        "spec Impl { enum ImplStage { C, B, A } state { stage: ImplStage } init { stage = A } action step() { requires stage == A stage = B } }",
    );
    let abstraction = build(ABS);
    let mapping = "refinement R { impl Impl abs Abs enum conversion stage ImplStage -> AbsStage { A -> A B -> B C -> C } map status = convert(stage, stage) action step() -> step() }";
    let refinement =
        parse_refinement(mapping, &implementation, &abstraction).expect("explicit enum conversion");
    let result = check_refinement(&implementation, &abstraction, &refinement, 2)
        .expect("concrete refinement check");
    assert!(result.failure.is_none(), "{result:?}");

    let wrong = parse_refinement(
        "refinement R { impl Impl abs Abs enum conversion stage ImplStage -> AbsStage { A -> B B -> A C -> C } map status = convert(stage, stage) action step() -> step() }",
        &implementation,
        &abstraction,
    )
    .expect("a complete but wrong bijection is statically well typed");
    let wrong = check_refinement(&implementation, &abstraction, &wrong, 2)
        .expect("negative control executes");
    assert!(
        wrong.failure.is_some(),
        "wrong member mapping must not refine"
    );

    let argument_implementation = build(
        "spec Impl { enum ImplStage { A, B } state { stage: ImplStage } init { stage = A } action send(s: ImplStage) { stage = s } }",
    );
    let argument_abstraction = build(
        "spec Abs { enum AbsStage { A, B } state { status: AbsStage } init { status = A } action send(s: AbsStage) { status = s } }",
    );
    let wrong_argument = parse_refinement(
        "refinement R { impl Impl abs Abs enum conversion state_stage ImplStage -> AbsStage { A -> A B -> B } enum conversion argument_stage ImplStage -> AbsStage { A -> B B -> A } map status = convert(state_stage, stage) action send(s) -> send(convert(argument_stage, s)) }",
        &argument_implementation,
        &argument_abstraction,
    )
    .expect("wrong action-argument bijection remains statically complete");
    let wrong_argument = check_refinement(
        &argument_implementation,
        &argument_abstraction,
        &wrong_argument,
        1,
    )
    .expect("action-argument negative control executes");
    assert!(
        wrong_argument.failure.is_some(),
        "member-swapped action argument must not produce a false-green refinement"
    );

    let inline = r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum conversion stage ImplStage -> AbsStage { A -> A B -> B C -> C }
    map status = convert(stage, stage)
    action step() -> step()
  }
  enum ImplStage { C, B, A }
  state { stage: ImplStage }
  init { stage = A }
  action step() { requires stage == A stage = B }
}"#;
    let inline_implementation = build(inline);
    let contract = requirements_implements(inline, &Resolver(ABS), &inline_implementation)
        .expect("inline enum conversion builds")
        .expect("implements contract");
    let inline_result = check_refinement(
        &inline_implementation,
        &contract.abstraction,
        &contract.refinement,
        2,
    )
    .expect("inline concrete refinement check");
    assert!(inline_result.failure.is_none(), "{inline_result:?}");
}
