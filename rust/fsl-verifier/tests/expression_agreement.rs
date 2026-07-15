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
  state {
    x: Count,
    flag: Bool,
    empty: Option<Count>,
    also_empty: Option<Count>,
    first: Option<Count>,
    same: Option<Count>,
    different: Option<Count>
  }
  init {
    x = -2
    flag = false
    empty = none
    also_empty = none
    first = some(-2)
    same = some(-2)
    different = some(-1)
  }
  action advance() { requires x < 2  x = x + 1  flag = not flag }
  invariant Arithmetic { (x / 2) * 2 + (x % 2) == x }
  invariant Mixed { (x < 0) or flag or not flag }
  invariant Conditional { (if flag then x else -x) >= -2 }
  invariant UnselectedPartialOperation { if true then x == x else x / 0 == 0 }
  invariant OptionTruthTable {
    empty == none and empty == also_empty and empty != first and first != none and
    first == same and first != different and first == some(-2)
  }
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
    let explicit = fsl_runtime::verify_explicit(model, 8, 100).expect("verify explicitly");
    assert!(explicit.violation.is_none(), "{explicit:?}");
    assert!(explicit.closure, "{explicit:?}");
}

#[test]
fn nested_option_equality_uses_the_existing_option_capability() {
    let source = r"
spec NestedOptionEquality {
  type Bit = 0..1
  state { x: Option<Option<Bit>> }
  init { x = none }
  action stay() { x = x }
  invariant Structural { x == none and x != some(none) }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let symbolic = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2))
        .expect("verify symbolically");
    assert!(symbolic.violation.is_none(), "{symbolic:?}");
    let explicit = fsl_runtime::verify_explicit(model, 2, 10).expect("verify explicitly");
    assert!(explicit.violation.is_none(), "{explicit:?}");
    assert!(explicit.closure, "{explicit:?}");
}

#[test]
fn binder_aggregates_agree_for_ranges_sets_and_duplicate_sequences() {
    let source = r"
spec BinderAggregateAgreement {
  type Item = 0..2
  state {
    selected: Set<Item>,
    queue: Seq<Item, 4>,
    empty: Seq<Item, 4>
  }
  init {
    selected = Set { 0, 2 }
    queue = Seq { 1, 1, 2 }
    empty = Seq {}
  }
  action stay() { selected = selected }
  invariant AggregateValues {
    count(item in selected) == 2 and
    count(item in selected where item > 0) == 1 and
    sum(item in selected of item) == 2 and
    count(item in queue) == 3 and
    count(item in queue where item == 1) == 2 and
    sum(item in queue of item) == 4 and
    sum(item in queue of item where item > 1) == 2 and
    count(item in empty) == 0 and
    sum(item in empty of item) == 0 and
    count(item in 0..2 where item > 0) == 2 and
    sum(item in 0..2 of item where item > 0) == 3 and
    count(item in queue where count(other in queue where other == item) >= 1) == 3 and
    count(item in queue where count(item in selected where item == 0) == 1) == 3 and
    unique(item in queue where item == 2) and
    exactlyOne(item in selected where item == 0)
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let property = model
        .invariants
        .iter()
        .find(|property| property.name == "AggregateValues")
        .expect("aggregate invariant");
    let concrete = fsl_runtime::eval(
        &property.expr,
        &fsl_runtime::Monitor::new(model.clone())
            .expect("create monitor")
            .state,
        &mut BTreeMap::new(),
        &model,
        None,
    )
    .expect("evaluate aggregate invariant");
    assert_eq!(concrete, fsl_core::FslValue::Bool(true));

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let symbolic = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2))
        .expect("verify symbolically");
    assert!(symbolic.violation.is_none(), "{symbolic:?}");
    let explicit = fsl_runtime::verify_explicit(model, 2, 10).expect("verify explicitly");
    assert!(explicit.violation.is_none(), "{explicit:?}");
    assert!(explicit.closure, "{explicit:?}");
}
