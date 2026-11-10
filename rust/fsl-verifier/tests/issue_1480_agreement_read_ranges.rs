// SPDX-License-Identifier: Apache-2.0
//! Agreement authorizes only the concrete, validated frame that each API reads.
use fsl_core::{FsResolver, FslValue as V, KernelModel, build_model, parse_kernel_source};
use fsl_verifier::{VerifyError, transition_matches_step, transition_outcome_matches_step};
use std::collections::BTreeMap;
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
fn model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("AGREEMENT_RANGE_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("AGREEMENT_RANGE_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn state(n: i64) -> BTreeMap<String, V> {
    BTreeMap::from([("n".into(), V::Int(n))])
}
fn count_state(n: i64, count: i64) -> BTreeMap<String, V> {
    BTreeMap::from([("n".into(), V::Int(n)), ("c".into(), V::Int(count))])
}
fn admitted(
    model: &KernelModel,
    current: &BTreeMap<String, V>,
    next: &BTreeMap<String, V>,
) -> Result<bool, VerifyError> {
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(transition_matches_step(
        model,
        &mut solver,
        current,
        "go",
        &BTreeMap::new(),
        next,
    ));
    eprintln!("AGREEMENT_RANGE_RELATION current={current:#?} next={next:#?} actual={actual:#?}");
    actual
}
fn outcome(
    model: &KernelModel,
    current: &BTreeMap<String, V>,
    committed: &BTreeMap<String, V>,
    attempted: Option<&BTreeMap<String, V>>,
    kind: &str,
) -> Result<bool, VerifyError> {
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(transition_outcome_matches_step(
        model,
        &mut solver,
        current,
        "go",
        &BTreeMap::new(),
        committed,
        attempted,
        kind,
    ));
    eprintln!(
        "AGREEMENT_RANGE_OUTCOME kind={kind} current={current:#?} committed={committed:#?} attempted={attempted:#?} actual={actual:#?}"
    );
    actual
}
#[test]
fn concrete_count_assignment_retains_all_three_members() {
    let model = model(
        "spec CountStep {state {n:0..2,c:Int} init {n=0 c=0} action go(){c=count(i in 0..n)}}",
    );
    let current = count_state(2, 0);
    let outputs = [3, 2].map(|count| admitted(&model, &current, &count_state(2, count)));
    let invalid = admitted(&model, &count_state(3, 0), &count_state(2, 3));
    assert_eq!(outputs, [Ok(true), Ok(false)]);
    assert_eq!(
        invalid,
        Err(VerifyError::new("agreement value is outside its range"))
    );
}
#[test]
fn requires_failed_checks_the_actual_disabled_guard_with_exact_rollback() {
    let current = state(2);
    let outputs = [4, 3].map(|count| {
        let source = format!("spec GuardCount {{state {{n:0..2}} init {{n=0}} action go(){{requires count(i in 0..n)=={count}}}}}");
        outcome(&model(&source), &current, &current, None, "requires_failed")
    });
    assert_eq!(outputs, [Ok(true), Ok(false)]);
}
#[test]
fn committed_property_range_truth_is_checked_after_transition_admission() {
    let current = state(2);
    let outputs = [2, 1].map(|limit| {
        let source = format!("spec PropertyRange {{state {{n:0..2}} init {{n=0}} action go(){{}} invariant P {{forall i in 0..n {{i<={limit}}}}}}}");
        outcome(&model(&source), &current, &current, None, "ok")
    });
    assert_eq!(outputs, [Ok(true), Ok(false)]);
}
#[test]
fn transition_and_ensures_old_counts_use_their_distinct_valid_snapshots() {
    let current = state(2);
    let next = state(1);
    let outputs = [(false, 1), (false, 2), (true, 1), (true, 2)].map(|(ensure, delta)| {
        let relation = format!("count(i in 0..n)==old(count(i in 0..n))-{delta}");
        let source = if ensure {
            format!("spec OldEnsure {{state {{n:0..2}} init {{n=2}} action go(){{requires n>0 n=n-1 ensures {relation}}}}}")
        } else {
            format!("spec OldTransition {{state {{n:0..2}} init {{n=2}} action go(){{requires n>0 n=n-1}} trans T {{{relation}}}}}")
        };
        outcome(&model(&source), &current, &next, None, "ok")
    });
    assert_eq!(outputs, [Ok(true), Ok(false), Ok(true), Ok(false)]);
    // Current n2 has roster0,1,2; successor n1 has roster0,1. The correct
    // relation is2==3-1. Rebinding old to successor makes even that false.
}
#[test]
fn no_range_membership_errors_and_explicit_bound_failure_exception_are_preserved() {
    let model = model("spec Membership {state {n:0..2} init {n=0} action go(){n=n+1}}");
    let no_range = [1, 2].map(|next| admitted(&model, &state(0), &state(next)));
    let invalid = [
        admitted(&model, &state(2), &state(3)),
        outcome(&model, &state(2), &state(3), None, "ok"),
    ];
    let bound_failure = outcome(&model, &state(2), &state(2), Some(&state(3)), "type_bound");
    assert_eq!(no_range, [Ok(true), Ok(false)]);
    for actual in invalid {
        assert_eq!(
            actual,
            Err(VerifyError::new("agreement value is outside its range"))
        );
    }
    assert_eq!(bound_failure, Ok(true));
    // Ordinary successors must conform; an explicit attempted type_bound
    // outcome is the existing narrow exception, with committed rollback.
}

#[test]
fn validated_parameter_membership_survives_the_guard_binding_adapter() {
    let current = state(2);
    let outputs = [(4, 2), (3, 2), (4, 3)].map(|(count, parameter)| {
        let source = format!("spec ParameterCount {{type K=0..2 state {{n:0..2}} init {{n=0}} action go(p:K){{requires count(i in 0..p)=={count}}}}}");
        let model = model(&source);
        let params = BTreeMap::from([("p".into(), V::Int(parameter))]);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(transition_outcome_matches_step(
            &model, &mut solver, &current, "go", &params, &current, None, "requires_failed",
        ));
        eprintln!("AGREEMENT_PARAMETER count={count} current={current:#?} committed={current:#?} attempted=None params={params:#?} actual={actual:#?}");
        actual
    });
    assert_eq!(
        outputs,
        [
            Ok(true),
            Ok(false),
            Err(VerifyError::new(
                "agreement parameter 'p' is outside the action domain"
            )),
        ]
    );
}
