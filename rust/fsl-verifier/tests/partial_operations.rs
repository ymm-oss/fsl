// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, KernelModel, build_model, parse_kernel_source};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model(source: &str) -> KernelModel {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse partial-op model");
    build_model(kernel).expect("build partial-op model")
}

#[test]
fn selected_bounds_do_not_disable_automatic_partial_operation_checks() {
    let model = model(
        r"
spec SelectedPartial {
  type Small = -3..3
  state { dividend: Small, divisor: Small, quotient: Small }
  init { dividend = -3  divisor = 0  quotient = 0 }
  action divide() { quotient = dividend / divisor }
}
",
    );
    let selected = BTreeSet::new();
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded_selected(
        &model,
        &mut solver,
        1,
        Some(&selected),
    ))
    .expect("verify selected properties");

    let violation = result.violation.expect("partial operation violation");
    assert_eq!(
        (
            violation.kind.as_str(),
            violation.name.as_str(),
            violation.step
        ),
        ("partial_op", "_partial_divide", 1)
    );
}

#[test]
fn supplied_initial_state_controls_partial_operation_reachability() {
    let model = model(
        r"
spec SnapshotPartial {
  type Small = -3..3
  state { dividend: Small, divisor: Small, quotient: Small }
  init { dividend = -3  divisor = 1  quotient = 0 }
  action divide() { quotient = dividend / divisor }
}
",
    );
    let mut ordinary_solver = fsl_solver_z3::Z3Solver::new().expect("create ordinary solver");
    let ordinary = block_on(fsl_verifier::verify_bounded(
        &model,
        &mut ordinary_solver,
        1,
    ))
    .expect("verify spec init");
    assert!(ordinary.violation.is_none(), "{ordinary:?}");

    let snapshot = BTreeMap::from([
        ("dividend".to_owned(), FslValue::Int(-3)),
        ("divisor".to_owned(), FslValue::Int(0)),
        ("quotient".to_owned(), FslValue::Int(0)),
    ]);
    let mut snapshot_solver = fsl_solver_z3::Z3Solver::new().expect("create snapshot solver");
    let result = block_on(fsl_verifier::verify_bounded_from_state(
        &model,
        &mut snapshot_solver,
        1,
        None,
        &snapshot,
    ))
    .expect("verify supplied initial state");

    let violation = result.violation.expect("snapshot partial operation");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.step, 1);
    assert_eq!(violation.trace[0].state, snapshot);
}

#[test]
fn guard_order_short_circuits_unreached_partial_operations() {
    let model = model(
        r"
spec GuardShortCircuit {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action blocked() {
    requires false
    requires queue.head() == 0
    queue = queue
  }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify short-circuited guard");

    assert!(result.violation.is_none(), "{result:?}");
}

#[test]
fn initial_invariants_precede_future_action_partial_operations() {
    let model = model(
        r"
spec InitialPrecedence {
  type Small = 0..1
  state { divisor: Small, quotient: Small }
  init { divisor = 0  quotient = 0 }
  action divide() { quotient = 1 / divisor }
  invariant InitiallyFalse { quotient == 1 }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify failure precedence");

    let violation = result.violation.expect("initial invariant violation");
    assert_eq!(
        (
            violation.kind.as_str(),
            violation.name.as_str(),
            violation.step
        ),
        ("invariant", "InitiallyFalse", 0)
    );
}

#[test]
fn post_state_properties_precede_ensures_partial_operations() {
    let model = model(
        r"
spec PostStatePrecedence {
  type Small = 0..1
  state { value: Small, queue: Seq<Small, 1> }
  init { value = 0  queue = Seq {} }
  action break_post_state() {
    value = 1
    ensures queue.head() == 0
  }
  invariant MustStayZero { value == 0 }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify post-state precedence");

    let violation = result.violation.expect("post-state invariant violation");
    assert_eq!(
        (
            violation.kind.as_str(),
            violation.name.as_str(),
            violation.step
        ),
        ("invariant", "MustStayZero", 1)
    );
}

#[test]
fn reached_requires_and_ensures_partial_operations_are_classified() {
    for (source, action) in [
        (
            r"
spec RequiresPartial {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action inspect() { requires queue.head() == 0  queue = queue }
}
",
            "inspect",
        ),
        (
            r"
spec EnsuresPartial {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action inspect() { queue = queue  ensures queue.head() == 0 }
}
",
            "inspect",
        ),
    ] {
        let model = model(source);
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
            .expect("verify action-context partial operation");
        let violation = result.violation.expect("partial operation violation");
        let expected_name = format!("_partial_{action}");
        fsl_runtime::replay_trace(model, &violation.trace)
            .expect("partial-operation trace must replay through Monitor");
        assert_eq!(
            (
                violation.kind.as_str(),
                violation.name.as_str(),
                violation.step
            ),
            ("partial_op", expected_name.as_str(), 1)
        );
    }
}

#[test]
fn guard_partial_replay_rejects_a_fabricated_successor() {
    let model = model(
        r"
spec GuardReplayControl {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action inspect() { requires queue.head() == 0  queue = queue }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify guard partial operation");
    let mut trace = result.violation.expect("guard partial operation").trace;
    trace
        .last_mut()
        .expect("attempted trace step")
        .state
        .insert("queue".to_owned(), FslValue::Seq(vec![FslValue::Int(0)]));

    let error = fsl_runtime::replay_trace(model, &trace)
        .expect_err("fabricated partial-operation successor must be rejected");
    assert!(error.to_string().contains("state mismatch"), "{error}");
}

#[test]
fn earlier_checked_overflow_is_not_relabelled_by_a_later_body_partial_operation() {
    let model = model(
        r"
spec BodyFailureOrder {
  state { maximum: Int, quotient: Int }
  init { maximum = 9223372036854775807  quotient = 0 }
  action overflow_first() {
    maximum = maximum + 1
    quotient = 1 / 0
  }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect_err("checked overflow must fail closed before the later partial operation");

    assert!(
        error
            .to_string()
            .contains("body evaluation has a non-partial failure"),
        "{error}"
    );
}

#[test]
fn earlier_checked_overflow_inside_ensures_is_not_relabelled_as_partial() {
    let model = model(
        r"
spec EnsuresFailureOrder {
  state { maximum: Int }
  init { maximum = 9223372036854775807 }
  action inspect() {
    maximum = maximum
    ensures (maximum + 1 > 0) and (1 / 0 == 0)
  }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect_err("checked overflow must fail closed before the later ensures partial operation");

    assert!(
        error
            .to_string()
            .contains("ensures evaluation has a non-partial failure"),
        "{error}"
    );
}

#[test]
fn body_overflow_precedes_a_partial_operation_found_only_in_ensures() {
    let model = model(
        r"
spec BodyBeforeEnsuresFailureOrder {
  state { maximum: Int }
  init { maximum = 9223372036854775807 }
  action overflow_first() {
    maximum = maximum + 1
    ensures 1 / 0 == 0
  }
}
",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect_err("body overflow must fail closed before an ensures-only partial operation");

    assert!(
        error
            .to_string()
            .contains("body evaluation has a non-partial failure"),
        "{error}"
    );
}
