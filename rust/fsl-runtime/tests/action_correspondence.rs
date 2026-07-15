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
