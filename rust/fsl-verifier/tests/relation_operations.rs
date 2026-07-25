// SPDX-License-Identifier: Apache-2.0

//! Native symbolic verifier coverage for `relation A -> B` state and its
//! seven operations (issue #467). Ports `tests/test_relation.py`'s
//! `RELATION_SRC` (positive), `CYCLIC_SRC` (negative control: a genuine
//! `acyclic` violation must be reported, not silently accepted), and
//! `BAD_SELF_RELATION_SRC` (rejected: `acyclic`/`reachable` require a
//! self-relation), plus a symbolic/concrete agreement check on the 0->1->0
//! cycle trace.

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

const RELATION_SRC: &str = r"
spec RelationDemo {
  type User = 0..1
  enum Role { Manager, Staff }

  state {
    delegates: relation User -> User,
    roles: relation User -> Role
  }

  init {
    delegates = Set {}
    roles = Set {}
  }

  action delegate(a: User, b: User) {
    requires a != b
    requires not reachable(delegates, b, a)
    delegates = delegates.add(a, b)
  }

  action revoke(a: User, b: User) {
    delegates = delegates.remove(a, b)
  }

  action grant(u: User, r: Role) {
    roles = roles.add(u, r)
  }

  invariant DelegatesAcyclic { acyclic(delegates) }
  invariant DelegatesFunctional { functional(delegates) }
  invariant RoleRangeBounded { range(roles).size() <= 2 }
  invariant DelegateDomainBounded { domain(delegates).size() <= 2 }
  reachable CanDelegate { delegates.contains(0, 1) }
}
";

const CYCLIC_SRC: &str = r"
spec CyclicRelation {
  type User = 0..1
  state { delegates: relation User -> User }
  init { delegates = Set {} }
  action delegate(a: User, b: User) {
    requires a != b
    delegates = delegates.add(a, b)
  }
  invariant DelegatesAcyclic { acyclic(delegates) }
}
";

const BAD_SELF_RELATION_SRC: &str = r"
spec BadSelfRelation {
  type User = 0..1
  enum Role { Manager, Staff }
  state { assignments: relation User -> Role }
  init { assignments = Set {} }
  action grant(u: User, r: Role) { assignments = assignments.add(u, r) }
  invariant BadAcyclic { acyclic(assignments) }
}
";

fn build(source: &str) -> fsl_core::KernelModel {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    build_model(kernel).expect("build model")
}

#[test]
fn relation_helpers_verify_and_reachable_witness() {
    let model = build(RELATION_SRC);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result =
        block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2)).expect("verify_bounded");
    assert!(result.violation.is_none(), "{result:?}");
    let can_delegate = result
        .reachables
        .get("CanDelegate")
        .and_then(Option::as_ref)
        .unwrap_or_else(|| panic!("expected CanDelegate witnessed, got {result:?}"));
    assert_eq!(can_delegate.step, 1);
}

#[test]
fn cyclic_relation_acyclic_violation_is_reported() {
    // Negative control: `acyclic(delegates)` genuinely fails on the 0->1->0
    // trace and must be reported, not silently accepted or reported as
    // `error`/`unknown`.
    let model = build(CYCLIC_SRC);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result =
        block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2)).expect("verify_bounded");
    let violation = result
        .violation
        .as_ref()
        .unwrap_or_else(|| panic!("expected a violation, got {result:?}"));
    assert_eq!(violation.name, "DelegatesAcyclic");
}

#[test]
fn acyclic_on_a_non_self_relation_is_rejected() {
    let model = build(BAD_SELF_RELATION_SRC);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect_err("acyclic() on a non-self relation must be rejected, not silently accepted");
    assert!(error.to_string().contains("self-relation"), "{error}");
}

#[test]
fn symbolic_transition_agrees_with_the_concrete_monitor_on_the_cycle_trace() {
    // The 0->1->0 cycle trace from issue #467's evidence: the symbolic
    // transition relation (used by `fslc verify`) must accept exactly the
    // successor states the concrete Monitor (used by `fslc replay`)
    // produces, for both `delegate(0, 1)` and `delegate(1, 0)`.
    let model = build(CYCLIC_SRC);
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");

    for (a, b) in [(0_i64, 1_i64), (1_i64, 0_i64)] {
        let current = monitor.state.clone();
        let enabled = monitor
            .enabled()
            .expect("enumerate enabled actions")
            .into_iter()
            .find(|candidate| {
                candidate.action == "delegate"
                    && candidate.params.get("a") == Some(&fsl_core::FslValue::Int(a))
                    && candidate.params.get("b") == Some(&fsl_core::FslValue::Int(b))
            })
            .unwrap_or_else(|| panic!("delegate({a}, {b}) must be enabled"));
        let result = monitor.step(&enabled).expect("step monitor");
        // `delegate(1, 0)` violates `DelegatesAcyclic`, so the Monitor rolls
        // back `state` to its pre-transition value; the actually-computed
        // successor (what the transition relation itself produces, before
        // the separate invariant check) is `attempted_state`. Transition
        // agreement is about the guard+effect relation, not invariants.
        let next = result.attempted_state.as_ref().unwrap_or(&result.state);

        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        assert!(
            block_on(fsl_verifier::transition_matches_step(
                &model,
                &mut solver,
                &current,
                &enabled.action,
                &enabled.params,
                next,
            ))
            .expect("check transition agreement"),
            "delegate({a}, {b}): symbolic transition relation disagrees with the concrete Monitor"
        );
    }
}
