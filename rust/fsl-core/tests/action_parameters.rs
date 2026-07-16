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
