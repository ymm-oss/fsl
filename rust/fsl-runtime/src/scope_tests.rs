// SPDX-License-Identifier: Apache-2.0

use super::*;
use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn model(expression: &str) -> KernelModel {
    build_model(
        parse_kernel_source(
            &format!(
                "spec Scope {{state {{x:Int=0}} action go() {{
         let matched = some(3) is some(a)
         requires some(1) is some(v) requires v == 1 x=v }}
         invariant Q {{{expression}}} }}"
            ),
            &FsResolver::new("."),
        )
        .expect("parse"),
    )
    .expect("model")
}

#[test]
fn guards_close_let_rhs_and_export_requires_payload() {
    let model = model("true");
    let state = State::from([("x".into(), Value::Int(0))]);
    let expected = Bindings::from([
        ("matched".into(), Value::Bool(true)),
        ("v".into(), Value::Int(1)),
    ]);
    assert_eq!(
        evaluate_action_guards(&model.actions[0], &Bindings::new(), &state, &model).unwrap(),
        Some(expected.clone())
    );
    let monitor = Monitor::new(model.clone()).expect("Monitor");
    let action = refinement_action_instance(&monitor, &model.actions[0], Bindings::new())
        .unwrap()
        .unwrap();
    assert_eq!(action.bindings, expected);
}

#[test]
fn error_restores_outer_scope_and_short_circuit_stays_lazy() {
    let model = model("true");
    let pattern = Expr::Is {
        expr: Box::new(Expr::Some(Box::new(Expr::Num(1)))),
        pattern: fsl_core::Pattern::Some("v".into()),
    };
    let expression = Expr::Binary {
        op: "and".into(),
        left: Box::new(pattern),
        right: Box::new(Expr::Var("missing".into())),
    };
    let mut bindings = Bindings::from([("v".into(), Value::Int(3))]);
    let before = bindings.clone();
    assert!(eval(&expression, &State::new(), &mut bindings, &model, None).is_err());
    assert_eq!(bindings, before);
    for (op, left, expected) in [
        ("and", false, false),
        ("or", true, true),
        ("=>", false, true),
    ] {
        let expression = Expr::Binary {
            op: op.into(),
            left: Box::new(Expr::Bool(left)),
            right: Box::new(Expr::Var("missing".into())),
        };
        assert_eq!(
            eval(&expression, &State::new(), &mut bindings, &model, None).unwrap(),
            Value::Bool(expected)
        );
        assert_eq!(bindings, before);
    }
}
