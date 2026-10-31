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

#[test]
fn diagnostic_candidates_retain_reached_payloads_without_lexical_exports() {
    for (filter, body, payloads) in [
        (
            "",
            "o is some(v) and v == 2",
            Some(vec![(0, Some(1)), (1, Some(1))]),
        ),
        ("", "o is some(v) and v == 1", None),
        (
            " where o is some(w) and w == 1",
            "o is some(v) and v == 2",
            Some(vec![(0, Some(1)), (1, Some(1))]),
        ),
        (" where o is some(w) and w == 2", "false", None),
        (
            "",
            "not (o is some(v))",
            Some(vec![(0, Some(1)), (1, Some(1))]),
        ),
        (
            "",
            "(o is some(v)) => false",
            Some(vec![(0, Some(1)), (1, Some(1))]),
        ),
        (
            "",
            "if true then (o is some(v) and false) else true",
            Some(vec![(0, Some(1)), (1, Some(1))]),
        ),
        (
            "",
            "(if k == 0 then some(1) else some(2)) is some(v) and v == 3",
            Some(vec![(0, Some(1)), (1, Some(2))]),
        ),
    ] {
        let model = witness_model("some(1)", &format!("forall k in 0..1{filter} {{{body}}}"));
        let monitor = Monitor::new(model.clone()).unwrap();
        let before = monitor.state.clone();
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        let expected = payloads.map(|items| {
            items
                .into_iter()
                .map(|(k, v)| {
                    let mut bindings = Bindings::from([("k".into(), Value::Int(k))]);
                    if let Some(v) = v {
                        bindings.insert("v".into(), Value::Int(v));
                    }
                    if filter.contains("w == 1") {
                        bindings.insert("w".into(), Value::Int(1));
                    }
                    bindings
                })
                .collect::<Vec<_>>()
        });
        let actual = violating_bindings(expr, &monitor.state, &model).unwrap();
        assert_eq!(actual, expected, "{filter}: {body}");
        assert_eq!(monitor.state, before);
        let mut caller = Bindings::from([("v".into(), Value::Int(9))]);
        let outer = caller.clone();
        eval(expr, &monitor.state, &mut caller, &model, None).unwrap();
        assert_eq!(caller, outer, "diagnostics must not reopen ordinary eval");
    }
}

fn witness_model(option: &str, expression: &str) -> KernelModel {
    build_model(parse_kernel_source(&format!(
        "spec W {{ state {{ o: Option<Int> = {option} }} action tick() {{}} invariant Q {{ {expression} }} }}"
    ), &FsResolver::new(".")).unwrap()).unwrap()
}

#[test]
fn nested_conjunction_diagnostics_keep_all_reached_candidate_payloads() {
    for frame in [
        "$",
        "($) and true",
        "true and ($)",
        "((true and ($)) and true) and true",
        "true and (true and (($) and true))",
    ] {
        for present in [true, false] {
            let body = frame.replace('$', "(if k == 0 then o else some(2)) is some(v) and v == 3");
            let model = witness_model(
                if present { "some(1)" } else { "none" },
                &format!("forall k in 0..1 {{{body}}}"),
            );
            let monitor = Monitor::new(model.clone()).unwrap();
            let expr = &model
                .invariants
                .iter()
                .find(|p| p.name == "Q")
                .unwrap()
                .expr;
            let mut first = Bindings::from([("k".into(), Value::Int(0))]);
            if present {
                first.insert("v".into(), Value::Int(1));
            }
            let expected = vec![
                first,
                Bindings::from([("k".into(), Value::Int(1)), ("v".into(), Value::Int(2))]),
            ];
            assert_eq!(
                violating_bindings(expr, &monitor.state, &model).unwrap(),
                Some(expected),
                "frame={frame} present={present}"
            );
            let mut caller = Bindings::from([("v".into(), Value::Int(9))]);
            let before = caller.clone();
            assert_eq!(
                eval(expr, &monitor.state, &mut caller, &model, None).unwrap(),
                Value::Bool(false)
            );
            assert_eq!(caller, before);
        }
    }
}

#[test]
fn continuation_short_circuit_exports_only_conjunction_and_stays_lazy() {
    for (body, expected, exported) in [
        (
            "(some(1) is some(v) and v == 2) and (1 / 0 == 0)",
            false,
            true,
        ),
        (
            "(some(1) is some(v) and v == 1) or (1 / 0 == 0)",
            true,
            false,
        ),
        (
            "(some(1) is some(v) and v == 2) => (1 / 0 == 0)",
            true,
            false,
        ),
    ] {
        let model = witness_model("some(1)", body);
        let monitor = Monitor::new(model.clone()).unwrap();
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        let mut bindings = Bindings::from([("outer".into(), Value::Int(9))]);
        let before = bindings.clone();
        assert_eq!(
            eval_with_bindings(expr, &monitor.state, &mut bindings, &model, None).unwrap(),
            Value::Bool(expected)
        );
        let mut expected_bindings = before.clone();
        if exported {
            expected_bindings.insert("v".into(), Value::Int(1));
        }
        assert_eq!(bindings, expected_bindings, "{body}");
        let mut closed = before.clone();
        assert_eq!(
            eval(expr, &monitor.state, &mut closed, &model, None).unwrap(),
            Value::Bool(expected)
        );
        assert_eq!(closed, before);
    }
}

#[test]
fn diagnostic_conjunction_search_and_fallback_observe_closed_children() {
    for (expression, expected) in [
        ("false and true", Bindings::new()),
        (
            "not (some(1) is some(v))",
            Bindings::from([("v".into(), Value::Int(1))]),
        ),
        (
            "(forall k in 0..0 { some(1) is some(v) and v == 2 }) and false",
            Bindings::from([("k".into(), Value::Int(0)), ("v".into(), Value::Int(1))]),
        ),
    ] {
        let model = model(expression);
        let monitor = Monitor::new(model.clone()).unwrap();
        let expr = &model
            .invariants
            .iter()
            .find(|p| p.name == "Q")
            .unwrap()
            .expr;
        assert_eq!(
            violating_bindings(expr, &monitor.state, &model).unwrap(),
            Some(vec![expected])
        );
    }
}

#[test]
fn private_old_observation_preserves_truth_caller_and_native_error() {
    let model = witness_model("some(1)", "not (o is some(v))");
    let state = Monitor::new(model.clone()).unwrap().state;
    let old = State::from([("o".into(), Value::Some(Box::new(Value::Int(2))))]);
    let expr = Expr::UnaryNamed {
        name: "old".into(),
        expr: Box::new(
            model
                .invariants
                .iter()
                .find(|p| p.name == "Q")
                .unwrap()
                .expr
                .clone(),
        ),
        span: model.invariants[0].span,
    };
    let before = Bindings::from([("v".into(), Value::Int(9))]);
    for old_state in [Some(&old), None] {
        let mut ordinary = before.clone();
        let expected = eval(&expr, &state, &mut ordinary, &model, old_state);
        let mut caller = before.clone();
        let mut observed = Bindings::new();
        let actual = eval_observing(
            &expr,
            &state,
            &mut caller,
            &model,
            old_state,
            Some(&mut observed),
        );
        assert_eq!(actual, expected);
        assert_eq!(ordinary, before);
        assert_eq!(caller, before);
        if old_state.is_some() {
            assert_eq!(actual, Ok(Value::Bool(false)));
            assert_eq!(observed, Bindings::from([("v".into(), Value::Int(2))]));
        } else {
            assert_eq!(actual, Err(runtime_error("old() used without old state")));
            assert!(observed.is_empty());
        }
    }
}
