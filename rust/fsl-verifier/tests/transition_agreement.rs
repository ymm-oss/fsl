// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, VecDeque};
use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

#[test]
fn collection_and_option_transitions_agree_across_bounded_reachable_states() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fslc/tests/fixtures/kernel_contract.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read comprehensive fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let mut queue = VecDeque::from([(initial, 0_usize)]);
    let mut seen = BTreeSet::new();
    let mut checked = 0_usize;

    while let Some((monitor, depth)) = queue.pop_front() {
        if !seen.insert(monitor.state.clone()) {
            continue;
        }
        for enabled in monitor.enabled().expect("enumerate actions") {
            let current = monitor.state.clone();
            let mut successor = monitor.clone();
            let result = successor.step(&enabled).expect("step monitor");
            if result.violation.is_some() {
                continue;
            }
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            assert!(
                block_on(fsl_verifier::transition_matches_step(
                    &model,
                    &mut solver,
                    &current,
                    &enabled.action,
                    &enabled.params,
                    &result.state,
                ))
                .expect("check transition agreement"),
                "{} disagreed from {current:?}",
                enabled.action
            );
            checked += 1;
            if depth < 3 {
                queue.push_back((successor, depth + 1));
            }
        }
    }
    assert!(checked >= 10, "fixture did not exercise enough transitions");
}

#[test]
fn symbolic_transition_accepts_monitor_successors_and_rejects_other_states() {
    let source = r"
spec Agreement {
  type Count = 0..2
  state { x: Count, flag: Bool }
  init { x = 0  flag = false }
  action advance(by in 1..1) {
    requires x < 2
    x = x + by
    flag = not flag
  }
  invariant Bound { x <= 2 }
}

";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let enabled = monitor.enabled().expect("enumerate actions")[0].clone();
    let result = monitor.step(&enabled).expect("step monitor");
    assert!(result.violation.is_none());

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &current,
            &enabled.action,
            &enabled.params,
            &result.state,
        ))
        .expect("check transition agreement")
    );

    let mut wrong = result.state;
    wrong.insert("flag".to_owned(), FslValue::Bool(false));
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &current,
            &enabled.action,
            &enabled.params,
            &wrong,
        ))
        .expect("reject different successor")
    );
}

#[test]
fn disabled_and_failed_monitor_outcomes_agree_and_rollback() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fslc/tests/fixtures/conformance_failures.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read failure fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");

    for action in &model.actions {
        let current = initial.state.clone();
        let mut monitor = initial.clone();
        let params = std::collections::BTreeMap::new();
        let result = monitor
            .attempt(&action.name, &params)
            .expect("evaluate failure outcome");
        let violation = result
            .violation
            .as_ref()
            .expect("failure fixture action must fail");
        assert_eq!(
            result.state, current,
            "{} committed a failed step",
            action.name
        );
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        assert!(
            block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &current,
                &action.name,
                &params,
                &result.state,
                result.attempted_state.as_ref(),
                &violation.kind,
            ))
            .expect("check failure agreement"),
            "{} {} disagreed",
            action.name,
            violation.kind
        );
    }
}
