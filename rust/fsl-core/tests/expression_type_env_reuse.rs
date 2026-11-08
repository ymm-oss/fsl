// SPDX-License-Identifier: Apache-2.0
//! `KernelModel::expression_type` and `equality_operand_type` reuse one base
//! type environment per thread. Another model, a changed model, or an earlier
//! call's lexical bindings must never leak into a later inference.
use fsl_core::{FsResolver, KernelExpr, KernelModel, TypeRef, build_model, parse_kernel_source};

fn model(state_type: &str, init: &str) -> KernelModel {
    let source = format!(
        "spec M {{ state {{ x:{state_type} }} init {{ x={init} }} action go() {{}} invariant P {{ true }} }}"
    );
    build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap()
}

fn e(source: &str) -> KernelExpr {
    fsl_syntax::parse_expr(source).unwrap()
}

fn typed(model: &KernelModel, expr: &str, bindings: &[(String, TypeRef)]) -> Option<TypeRef> {
    model.expression_type(&e(expr), bindings, None).unwrap()
}

#[test]
fn reused_environment_follows_the_model_and_drops_bindings() {
    let int = model("Int", "0");
    let mut flag = model("Bool", "false");
    assert_eq!(typed(&int, "x", &[]), Some(TypeRef::Int));
    assert_eq!(typed(&flag, "x", &[]), Some(TypeRef::Bool));
    assert_eq!(typed(&int, "x", &[]), Some(TypeRef::Int));
    let equality = |model: &KernelModel| model.equality_operand_type(&e("x"), &e("x"), &[]);
    assert_eq!(equality(&flag).unwrap(), TypeRef::Bool);
    assert_eq!(equality(&int).unwrap(), TypeRef::Int);

    let shadow = [
        ("x".to_owned(), TypeRef::Int),
        ("y".to_owned(), TypeRef::Bool),
    ];
    assert_eq!(typed(&flag, "x", &shadow), Some(TypeRef::Int));
    assert_eq!(typed(&flag, "y", &shadow), Some(TypeRef::Bool));
    assert_eq!(typed(&flag, "x", &[]), Some(TypeRef::Bool));
    assert!(flag.expression_type(&e("y"), &[], None).is_err());

    flag.state[0].1 = TypeRef::Int;
    assert_eq!(typed(&flag, "x", &[]), Some(TypeRef::Int));
}
