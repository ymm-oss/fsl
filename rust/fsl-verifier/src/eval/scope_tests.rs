// SPDX-License-Identifier: Apache-2.0

use super::*;
use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_solver::{SatResult, SmtSolver};

#[path = "../../tests/support/observation.rs"]
mod observations;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("native solver yielded Pending"),
    }
}

fn scope_model(expression: &str) -> KernelModel {
    build_model(
        parse_kernel_source(
            &format!(
                "spec Scope {{ struct Box {{flag:Bool}} state {{
                 s:Seq<Bool,1> = Seq {{true}}, t:Set<Bool> = Set {{true}} }}
                 action tick() {{}} invariant Q {{ {expression} }} }}"
            ),
            &FsResolver::new("."),
        )
        .expect("parse"),
    )
    .expect("checked model")
}

#[test]
fn closed_children_restore_values_and_declared_types() {
    let solver = fsl_solver_z3::Z3Solver::new().expect("solver");
    for (index, expression) in [
        "some(1) is some(v) and v == 1",
        "not (some(1) is some(v))",
        "if false then (some(1) is some(v)) else true",
        "(some(1) is some(v)) == true",
        "some(some(1) is some(v)) == some(true)",
        "Seq {some(1) is some(v)} == s",
        "Set {some(1) is some(v)} == t",
        "s.contains(some(1) is some(v))",
        "t.contains(some(1) is some(v))",
        "(Box {flag: some(1) is some(v)}).flag",
        "forall k in 0..0 { some(1) is some(v) }",
        "exactlyOne(k in 0..0 where (some(1) is some(v)))",
    ]
    .into_iter()
    .enumerate()
    {
        let model = scope_model(expression);
        let state = crate::value::symbolic_state(&solver, &model, 0).expect("state");
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        for contextual in [false, true] {
            let mut bindings = Bindings::new();
            // Declared type deliberately differs from the value's runtime type.
            bindings.insert(
                "v".into(),
                TypedBinding(
                    int_value(&solver, solver.int_value(3)),
                    Some(TypeRef::Range(0, 5)),
                ),
            );
            let before = format!("{bindings:?}");
            let result = if contextual {
                eval_expected(
                    &solver,
                    &model,
                    expr,
                    &TypeRef::Bool,
                    &state,
                    &mut bindings,
                    None,
                )
            } else {
                eval(&solver, &model, expr, &state, &mut bindings, None)
            }
            .unwrap_or_else(|error| panic!("{expression}: {error:?}"));
            observations::capture(
                &format!("closed-unit-{index}-contextual-{contextual}"),
                "full",
                &(expression, &model, &state, &before, &bindings, result),
            );
            assert_eq!(
                format!("{bindings:?}"),
                before,
                "{expression} contextual={contextual}"
            );
            assert_eq!(
                binding_types(&bindings),
                vec![("v".into(), TypeRef::Range(0, 5))]
            );

            let mut empty = Bindings::new();
            eval(&solver, &model, expr, &state, &mut empty, None).expect("closed eval");
            assert!(empty.keys().next().is_none(), "leaked name in {expression}");
        }
    }
}

#[test]
fn error_scope_closes_and_local_match_uses_new_payload() {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("solver");
    // The error path must close the same scope, not only successful evaluation.
    let model = scope_model("true");
    let expr = Expr::Binary {
        op: "and".into(),
        left: Box::new(Expr::Is {
            expr: Box::new(Expr::Some(Box::new(Expr::Num(1)))),
            pattern: Pattern::Some("v".into()),
        }),
        right: Box::new(Expr::Var("missing".into())),
    };
    let mut bindings = Bindings::new();
    assert!(
        eval(
            &solver,
            &model,
            &expr,
            &SymbolicState::new(),
            &mut bindings,
            None
        )
        .is_err()
    );
    assert!(bindings.keys().next().is_none());
    // A rewritten local match shadows an outer binder within its continuation.
    let model = scope_model("some(1) is some(v) and v == 1");
    let expr = &model
        .invariants
        .iter()
        .find(|p| p.name == "Q")
        .unwrap()
        .expr;
    bindings.insert("v".into(), int_value(&solver, solver.int_value(3)));
    let value = eval(
        &solver,
        &model,
        expr,
        &SymbolicState::new(),
        &mut bindings,
        None,
    )
    .expect("shadow");
    solver
        .assert(&solver.not(bool_term(&value).unwrap()).unwrap())
        .unwrap();
    assert_eq!(block_on(solver.check()).unwrap(), SatResult::Unsat);
}

#[test]
fn concrete_closed_children_restore_environment_and_truth() {
    for (expression, expected) in [
        ("some(1) is some(v) and v == 1", true),
        ("not (some(1) is some(v))", false),
        ("if false then (some(1) is some(v)) else true", true),
        ("(some(1) is some(v)) == true", true),
        ("some(some(1) is some(v)) == some(true)", true),
        ("(Box {flag: some(1) is some(v)}).flag", true),
    ] {
        let model = scope_model(expression);
        let monitor = fsl_runtime::Monitor::new(model.clone()).expect("Monitor");
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        for outer in [false, true] {
            let mut bindings = fsl_runtime::Bindings::new();
            if outer {
                bindings.insert("v".into(), FslValue::Int(3));
            }
            let before = bindings.clone();
            let value =
                fsl_runtime::eval(expr, &monitor.state, &mut bindings, &model, None).expect("eval");
            assert_eq!(
                value,
                FslValue::Bool(expected),
                "{expression} outer={outer}"
            );
            assert_eq!(bindings, before, "{expression} outer={outer}");
        }
    }
}

#[test]
fn guards_keep_declared_context_and_only_export_requires() {
    let model = build_model(
        parse_kernel_source(
            "spec Guards {state {x:Int=0} action go() {
         let matched = some(3) is some(a)
         requires some(1) is some(v) requires v == 1 x=v }}",
            &FsResolver::new("."),
        )
        .unwrap(),
    )
    .unwrap();
    let solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let state = crate::value::symbolic_state(&solver, &model, 0).unwrap();
    let (_, values) = crate::transition::action_guards(
        &solver,
        &model,
        &model.actions[0],
        &state,
        &Bindings::new(),
    )
    .unwrap();
    let status = crate::transition::action_guard_definedness(
        &solver,
        &model,
        &model.actions[0],
        &state,
        &Bindings::new(),
    )
    .unwrap();
    for bindings in [values, status.bindings] {
        assert_eq!(
            bindings.keys().cloned().collect::<Vec<_>>(),
            ["matched", "v"]
        );
        assert_eq!(
            format!("{:?}", int_term(bindings.get("v").unwrap()).unwrap()),
            format!("{:?}", solver.int_value(1))
        );
        assert_eq!(
            binding_types(&bindings),
            [
                ("matched".into(), TypeRef::Bool),
                ("v".into(), TypeRef::Int)
            ]
        );
    }
}

#[test]
fn definedness_reads_outer_zero_after_completed_left_expression() {
    for left in [
        "(some(1) is some(a)) and false",
        "(some(1) is some(a)) => false",
        "not (some(1) is some(a))",
    ] {
        let mut model = build_model(
            parse_kernel_source(
                &format!(
                    "spec Status {{state {{x:Int=0}} action tick() {{}}
             invariant Q {{({left}) or (1 / x == 0)}} }}"
                ),
                &FsResolver::new("."),
            )
            .unwrap(),
        )
        .unwrap();
        let expr = &mut model
            .invariants
            .iter_mut()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        let Expr::Binary { left, .. } = expr else {
            panic!("or")
        };
        let (Expr::Binary { left: pattern, .. } | Expr::Not(pattern)) = left.as_mut() else {
            panic!("left expression")
        };
        let Expr::Is {
            pattern: Pattern::Some(name),
            ..
        } = pattern.as_mut()
        else {
            panic!("pattern")
        };
        *name = "x".into();
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let state = SymbolicState::from([("x".into(), int_value(&solver, solver.int_value(0)))]);
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        let status =
            evaluation_status(&solver, &model, expr, &state, &Bindings::new(), None).unwrap();
        solver.assert(&status.fully_defined).unwrap();
        assert_eq!(
            block_on(solver.check()).unwrap(),
            SatResult::Unsat,
            "outer divisor is zero: {expr:?}"
        );
    }
}
