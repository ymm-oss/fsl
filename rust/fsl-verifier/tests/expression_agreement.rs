// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, build_model, parse_kernel_source};

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
fn symbolic_and_monitor_expressions_agree_on_reachable_states() {
    let source = r"
spec Agreement {
  type Count = -2..2
  state { x: Count, flag: Bool }
  init { x = -2  flag = false }
  action advance() { requires x < 2  x = x + 1  flag = not flag }
  invariant Arithmetic { (x / 2) * 2 + (x % 2) == x }
  invariant Mixed { (x < 0) or flag or not flag }
}
";
    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let mut states = vec![monitor.state.clone()];
    while let Some(action) = monitor
        .enabled()
        .expect("enumerate actions")
        .first()
        .cloned()
    {
        monitor.step(&action).expect("step monitor");
        states.push(monitor.state.clone());
    }
    for state in states {
        for property in &model.invariants {
            let expected = fsl_runtime::eval(
                &property.expr,
                &state,
                &mut BTreeMap::default(),
                &model,
                None,
            )
            .expect("evaluate concretely");
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            assert!(
                block_on(fsl_verifier::expression_matches_value(
                    &model,
                    &mut solver,
                    &property.expr,
                    &state,
                    &expected,
                ))
                .expect("prove agreement"),
                "expression {} disagreed in state {state:?}",
                property.name,
            );
        }
    }
}
