// SPDX-License-Identifier: Apache-2.0
//! Authored aggregate admission and strict execution/projection guards.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceStep, build_model, parse_kernel_source,
};
use fsl_verifier::{
    BmcViolation, VerifyError, expression_matches_value as matches_value,
    verify_bounded_from_state as bounded,
};
use std::collections::{BTreeMap, BTreeSet};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}
fn model(domain: &str, predicate: &str) -> KernelModel {
    let source = format!(
        "spec U4 {{ type K = {domain} state {{ n: K }} init {{ n = 1 }} action idle() {{}} invariant Q {{ {predicate} }} }}"
    );
    let parsed = parse_kernel_source(&source, &FsResolver::new(".")).unwrap();
    let result = build_model(parsed);
    eprintln!("U4_SOURCE source={source:?} result={result:?}");
    result.unwrap()
}
fn state(value: i64) -> BTreeMap<String, V> {
    BTreeMap::from([("n".into(), V::Int(value))])
}
fn expression(text: &str) -> fsl_core::KernelExpr {
    fsl_syntax::parse_expr(text).unwrap() // Parser only: raw public Kernel input.
}

#[test]
fn concrete_actual_members_and_static_agreement_bool_are_distinct() {
    let base = model("0..2", "true");
    for repeat in 0..2 {
        for (domain, input, text, expected) in [
            ("0..2", 3, "forall i in 0..n { i <= 2 }", V::Bool(false)),
            ("0..2", 3, "count(i in 0..n)", V::Int(4)),
            ("1..2", 0, "forall i in n..0 { false }", V::Bool(false)),
            ("1..2", 0, "count(i in n..0)", V::Int(1)),
            ("1..2", 1, "forall i in n..0 { false }", V::Bool(true)),
            ("1..2", 1, "count(i in n..0)", V::Int(0)),
        ] {
            let current = state(input);
            let declared = model(domain, "true");
            let expr = expression(text);
            let actual = fsl_runtime::eval(&expr, &current, &mut BTreeMap::new(), &declared, None);
            eprintln!(
                "U4_CONCRETE repeat={repeat} source={text:?} state={current:?} actual={actual:?}"
            );
            assert_eq!(actual, Ok(expected.clone()));
            let wrong = match expected {
                V::Bool(value) => V::Bool(!value),
                V::Int(value) => V::Int(value + 1),
                _ => unreachable!(),
            };
            assert_ne!(actual, Ok(wrong));
        }
        for (text, expected, agrees) in [
            ("forall i in 1..1 { false }", V::Bool(false), true),
            ("forall i in 1..1 { false }", V::Bool(true), false),
            ("forall i in 2..1 { false }", V::Bool(true), true),
            ("count(i in 0..1)", V::Int(2), true),
            ("count(i in 0..1)", V::Int(3), false),
        ] {
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(matches_value(
                &base,
                &mut solver,
                &expression(text),
                &state(1),
                &expected,
            ));
            eprintln!(
                "U4_AGREEMENT repeat={repeat} source={text:?} expected_value={expected:?} actual={actual:?}"
            );
            assert_eq!(actual, Ok(agrees)); // true means agreement, not the FSL Bool.
            assert_ne!(actual, Ok(!agrees));
        }
    }
}

#[test]
fn public_dynamic_routes_require_eligibility_and_keep_full_bound_witness() {
    const ELIGIBILITY: &str = "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope";
    for repeat in 0..2 {
        for (domain, input, text) in [
            ("0..2", 3, "forall i in 0..n { i <= 2 }"),
            ("1..2", 0, "forall i in n..0 { false }"),
            ("1..1", 0, "forall i in n..n { i == 1 }"),
        ] {
            let m = model(domain, text);
            let current = state(input);
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(bounded(&m, &mut solver, 0, None, &current));
            eprintln!(
                "U4_FULL_BOUND repeat={repeat} model={m:?} state={current:?} actual={actual:?}"
            );
            let result = actual.unwrap();
            let expected = BmcViolation {
                kind: "type_bound".into(),
                name: "_bounds_n".into(),
                step: 0,
                last_action: None,
                trace: vec![TraceStep {
                    step: 0,
                    state: current.clone(),
                    action: None,
                    changes: BTreeMap::new(),
                }],
                leads_to: None,
            };
            assert_eq!(result.violation, Some(expected.clone()));
            let mut wrong = expected;
            wrong.kind = "invariant".into();
            assert_ne!(result.violation, Some(wrong));
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let selected = BTreeSet::new();
            let excluded = block_on(bounded(&m, &mut solver, 0, Some(&selected), &current));
            eprintln!(
                "U4_EXCLUDED repeat={repeat} source={text:?} state={current:?} actual={excluded:?}"
            );
            assert_eq!(excluded, Err(VerifyError::new(ELIGIBILITY)));
            for value in [V::Bool(false), V::Bool(true)] {
                let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
                let agreement = block_on(matches_value(
                    &m,
                    &mut solver,
                    &expression(text),
                    &current,
                    &value,
                ));
                eprintln!(
                    "U4_PUBLIC repeat={repeat} source={text:?} state={current:?} expected_value={value:?} actual={agreement:?}"
                );
                assert_eq!(agreement, Err(VerifyError::new(ELIGIBILITY)));
            }
        }
    }
}

#[test]
fn static_failed_fold_keeps_priority_and_dynamic_source_checks_definedness() {
    for repeat in 0..2 {
        for (text, error, rejected) in [
            (
                "forall i in (9223372036854775807 + 1)..0 { false }",
                "invalid static integer expression",
                "invariant 'Q' is undefined for a reachable state",
            ),
            (
                "forall i in (n + 1)..0 { false }",
                "invariant 'Q' is undefined for a reachable state",
                "'n' is not an integer const",
            ),
        ] {
            let m = model("(-9223372036854775807-1)..9223372036854775807", text);
            let current = state(i64::MAX);
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(bounded(&m, &mut solver, 0, None, &current));
            eprintln!(
                "U4_PRIORITY repeat={repeat} source={text:?} state={current:?} actual={actual:?}"
            );
            assert_eq!(actual, Err(VerifyError::new(error)));
            assert_ne!(actual, Err(VerifyError::new(rejected)));
        }
    }
}

#[test]
fn authored_aggregate_admission_preserves_ast_and_runtime_but_refuses_strict_consumers() {
    const SOURCE: &str = "spec Count { state { n: Int } init { n = 0 } action idle() {} invariant Q { count(i in 0..n) == 4 } }";
    for repeat in 0..2 {
        let admitted = build_model(parse_kernel_source(SOURCE, &FsResolver::new(".")).unwrap())
            .expect("authored deferred aggregate must be admitted");
        let authored = expression("count(i in 0..n) == 4");
        let predicate = &admitted.invariants[0];
        let stored = &predicate.expr;
        assert_eq!(stored, &authored);
        eprintln!("U5_ADMISSION_BOUNDARY build repeat={repeat} class=Ok ast_equal=true");

        let projection = fsl_core::public_kernel_expression(
            stored,
            &admitted,
            "issue_1480_u4_existing_api.rs",
            predicate.span,
            Some(&fsl_core::TypeRef::Bool),
        );
        assert_eq!(
            projection,
            Err(fsl_core::PublicKernelError {
                message: "'n' is not an integer const".into(),
                span: None,
            })
        );
        let projection = projection.unwrap_err();
        eprintln!(
            "U5_ADMISSION_BOUNDARY projection repeat={repeat} class=Err message={:?} span={:?}",
            projection.message, projection.span
        );

        for (input, expected) in [(0, false), (3, true)] {
            let current = state(input);
            let actual = fsl_runtime::eval(stored, &current, &mut BTreeMap::new(), &admitted, None);
            assert_eq!(actual, Ok(V::Bool(expected)));
            eprintln!(
                "U5_ADMISSION_BOUNDARY runtime repeat={repeat} n={input} expected={expected} actual={actual:?}"
            );
            for expected_value in [expected, !expected] {
                let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
                let refusal = block_on(matches_value(
                    &admitted,
                    &mut solver,
                    stored,
                    &current,
                    &V::Bool(expected_value),
                ));
                assert_eq!(
                    refusal,
                    Err(VerifyError::new(
                        "unsupported integer range execution [planner]: missing-finite-fact"
                    ))
                );
                let refusal = refusal.unwrap_err();
                assert_eq!(
                    refusal.message,
                    "unsupported integer range execution [planner]: missing-finite-fact"
                );
                assert!(!refusal.is_solver_failure());
                eprintln!(
                    "U5_ADMISSION_BOUNDARY query repeat={repeat} n={input} expected={expected_value} class=Err message={:?} solver_failure={}",
                    refusal.message,
                    refusal.is_solver_failure()
                );
            }
        }
    }
}
