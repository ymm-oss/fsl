// SPDX-License-Identifier: Apache-2.0
//! Future U4 public acceptance; not current339 characterization or U5 support.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceStep, build_model, parse_kernel_source,
};
use fsl_verifier::{
    BmcViolation, VerifyError, expression_matches_value as matches_value,
    transition_matches_step as matches_step, verify_bounded_from_state as bounded,
};
use std::collections::{BTreeMap, BTreeSet};
const ELIGIBILITY: &str = "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope";
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver yielded Pending"),
    }
}
fn model(domain: &str, predicate: &str) -> KernelModel {
    let source = format!(
        "spec U4 {{ type K={domain} state {{n:K}} init {{n=1}} action idle(){{}} invariant Q {{{predicate}}} }}"
    );
    let result = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap());
    eprintln!("U4_FUTURE_SOURCE source={source:?} actual={result:?}");
    result.unwrap()
}
fn state(value: i64) -> BTreeMap<String, V> {
    BTreeMap::from([("n".into(), V::Int(value))])
}
fn violation(input: i64, kind: &str, name: &str) -> BmcViolation {
    BmcViolation {
        kind: kind.into(),
        name: name.into(),
        step: 0,
        last_action: None,
        trace: vec![TraceStep {
            step: 0,
            state: state(input),
            action: None,
            changes: BTreeMap::new(),
        }],
        leads_to: None,
    }
}
#[test]
fn root_quantifiers_keep_real_members_and_invalid_bounds_keep_first_witness() {
    for repeat in 0..2 {
        for (domain, input, text, failure) in [
            ("0..2", 2, "forall i in 0..n {i<=2}", None),
            (
                "0..2",
                2,
                "forall i in 0..n {i<2}",
                Some(("invariant", "Q")),
            ),
            ("0..2", 2, "exists i in 0..n {i==2}", None),
            (
                "0..2",
                2,
                "exists i in 0..n {i>2}",
                Some(("invariant", "Q")),
            ),
            (
                "0..2",
                3,
                "forall i in 0..n {i<=2}",
                Some(("type_bound", "_bounds_n")),
            ),
            (
                "1..2",
                0,
                "forall i in n..0 {false}",
                Some(("type_bound", "_bounds_n")),
            ),
            ("1..2", 1, "forall i in n..0 {false}", None),
            (
                "1..1",
                0,
                "forall i in n..n {i==1}",
                Some(("type_bound", "_bounds_n")),
            ),
            ("1..1", 1, "forall i in n..n {i==1}", None),
        ] {
            let m = model(domain, text);
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(bounded(&m, &mut solver, 0, None, &state(input)));
            eprintln!(
                "U4_ROOT repeat={repeat} source={text:?} state={:?} actual={actual:?}",
                state(input)
            );
            let expected = failure.map(|(kind, name)| violation(input, kind, name));
            assert_eq!(actual.unwrap().violation, expected);
            if let Some(expected) = expected {
                assert_ne!(expected.kind, "partial_op");
            }
        }
    }
}
#[test]
fn selection_public_agreement_and_raw_aggregates_cannot_inherit_eligibility() {
    for repeat in 0..2 {
        for (domain, input, text, aggregate, count) in [
            ("0..2", 3, "forall i in 0..n {i<=2}", "count(i in 0..n)", 4),
            ("0..2", 2, "forall i in 0..n {i<=2}", "count(i in 0..n)", 3),
            ("1..2", 0, "forall i in n..0 {false}", "count(i in n..0)", 1),
            ("1..1", 0, "forall i in n..n {i==1}", "count(i in n..n)", 1),
        ] {
            let m = model(domain, text);
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(bounded(
                &m,
                &mut solver,
                0,
                Some(&BTreeSet::new()),
                &state(input),
            ));
            eprintln!("U4_SELECTED repeat={repeat} source={text:?} actual={actual:?}");
            // An unselected bound is not assumed: the frame proves it, or the read refuses.
            if domain == "0..2" && input == 2 {
                assert_eq!(actual.unwrap().violation, None);
            } else {
                assert_eq!(actual, Err(VerifyError::new(ELIGIBILITY)));
            }
            for (query, value) in [
                (text, V::Bool(false)),
                (text, V::Bool(true)),
                (aggregate, V::Int(count)),
                (aggregate, V::Int(0)),
            ] {
                let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
                let expr = fsl_syntax::parse_expr(query).unwrap();
                let actual = block_on(matches_value(&m, &mut solver, &expr, &state(input), &value));
                eprintln!(
                    "U4_NO_PROOF repeat={repeat} source={query:?} value={value:?} actual={actual:?}"
                );
                if domain == "0..2" && input == 2 {
                    // This one concrete n2 row proves its own referenced bound.
                    // Its forall is true and its inclusive count is exactly3.
                    let agrees = value == V::Bool(true) || value == V::Int(3);
                    assert_eq!(actual, Ok(agrees));
                    assert_ne!(actual, Ok(!agrees));
                } else {
                    assert_eq!(actual, Err(VerifyError::new(ELIGIBILITY)));
                    assert_ne!(actual, Ok(true));
                    assert_ne!(actual, Ok(false));
                }
            }
        }
    }
}
#[test]
fn source_definedness_wins_for_dynamic_overflow_but_static_fold_keeps_its_error() {
    for repeat in 0..2 {
        for (text, message) in [
            (
                "forall i in (n+1)..0 {false}",
                "invariant 'Q' is undefined for a reachable state",
            ),
            (
                "forall i in (9223372036854775807+1)..0 {false}",
                "invalid static integer expression",
            ),
        ] {
            let m = model("(-9223372036854775807-1)..9223372036854775807", text);
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(bounded(&m, &mut solver, 0, None, &state(i64::MAX)));
            eprintln!("U4_ENDPOINT_PRIORITY repeat={repeat} source={text:?} actual={actual:?}");
            assert_eq!(actual, Err(VerifyError::new(message)));
            assert_ne!(actual, Err(VerifyError::new(ELIGIBILITY)));
        }
    }
}
#[test]
fn genuine_parameter_action_has_exact_poststate_and_rejects_extra_candidate_three() {
    let source = "spec Params {type K=0..2 type I=0..3 state {marks:Map<I,Bool>} init {forall i:I {marks[i]=false}} action fill(lim:K){forall i in 0..(lim+1) {marks[i]=true}}}";
    let m = build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap();
    let map = |values: [bool; 4]| {
        V::Map(
            values
                .into_iter()
                .enumerate()
                .map(|(key, value)| (V::Int(i64::try_from(key).unwrap()), V::Bool(value)))
                .collect(),
        )
    };
    let before = BTreeMap::from([("marks".into(), map([false; 4]))]);
    let after = BTreeMap::from([("marks".into(), map([true, true, true, false]))]);
    let wrong = BTreeMap::from([("marks".into(), map([true; 4]))]);
    let parameters = BTreeMap::from([("lim".into(), V::Int(1))]);
    for repeat in 0..2 {
        for (next, expected) in [(&after, true), (&wrong, false)] {
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let actual = block_on(matches_step(
                &m,
                &mut solver,
                &before,
                "fill",
                &parameters,
                next,
            ));
            eprintln!(
                "U4_PARAMETER repeat={repeat} source={source:?} before={before:?} params={parameters:?} after={next:?} actual={actual:?}"
            );
            assert_eq!(actual, Ok(expected));
        }
    }
}
