// SPDX-License-Identifier: Apache-2.0

//! `symmetric type` / `symmetric enum` is accepted but not used for any
//! reduction (issue #1935). The former `leadsTo` lasso/stall symmetry
//! reduction (#461) constrained a representative state to a canonical
//! permutation. That is sound only for a model whose transition relation,
//! initial states and properties are equivariant under a renaming of the
//! symmetric type's values, and nothing checks that. Each spec below breaks
//! the premise in a different way and has a genuine `leadsTo` violation; the
//! reduction reported `verified` for every one of them. The regression
//! assertion is that the `symmetric` spec's result equals that of the same
//! spec without `symmetric`, and that the latter is a `leadsTo` violation.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, KernelModel, build_model, parse_kernel_source};
use fsl_verifier::BmcResult;

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
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    build_model(kernel).expect("build model")
}

fn plain(source: &str) -> String {
    let plain = source
        .replace("symmetric type ", "type ")
        .replace("symmetric enum ", "enum ");
    assert!(!plain.contains("symmetric"), "{plain}");
    assert_ne!(plain, source, "fixture must declare a symmetric type");
    plain
}

// Each run gets a fresh Z3 context: models (and so witnesses) can depend on
// what earlier solvers on the same context did, and the comparison below is
// of whole results, witnesses included.
fn verify(source: &str, depth: usize) -> BmcResult {
    let model = model(source);
    fsl_solver_z3::with_fresh_context(|| {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth)).expect("verify_bounded")
    })
}

fn verify_from(source: &str, depth: usize, snapshot: &BTreeMap<String, FslValue>) -> BmcResult {
    let model = model(source);
    fsl_solver_z3::with_fresh_context(|| {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        block_on(fsl_verifier::verify_bounded_from_state(
            &model,
            &mut solver,
            depth,
            None,
            snapshot,
        ))
        .expect("verify_bounded_from_state")
    })
}

/// The plain twin has a `leadsTo` violation named `property`, and the
/// `symmetric` spec's result is identical to it.
fn assert_symmetric_matches_violated_plain(
    symmetric: &BmcResult,
    plain: &BmcResult,
    property: &str,
) {
    assert!(plain.violation.is_none(), "{plain:?}");
    let violation = plain
        .leadsto_violation
        .as_ref()
        .unwrap_or_else(|| panic!("plain twin: expected a leadsTo violation, got {plain:?}"));
    assert_eq!(violation.kind, "leadsTo");
    assert_eq!(violation.name, property);
    assert_eq!(symmetric, plain);
}

fn check(source: &str, depth: usize, property: &str) {
    let symmetric = verify(source, depth);
    let plain = verify(&plain(source), depth);
    assert_symmetric_matches_violated_plain(&symmetric, &plain, property);
}

/// f01: an order comparison on the symmetric type (`t < 1`) in a guard.
#[test]
fn order_comparison_in_guard() {
    check(
        r"
spec OrderLt {
  symmetric type TaskId = 0..1
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish(t: TaskId) {
    requires status[t] == Pending and t < 1
    status[t] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
",
        4,
        "EveryTaskFinishes",
    );
}

/// f02: a literal index of the symmetric type (`status[0]`) in an action.
#[test]
fn literal_index_in_action() {
    check(
        r"
spec LiteralIndex {
  symmetric type TaskId = 0..1
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish0() {
    requires status[0] == Pending
    status[0] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
",
        4,
        "EveryTaskFinishes",
    );
}

/// f17: a fully equivariant spec run from a supplied state (`verify
/// --from-state`). The snapshot is not invariant under renaming, so the
/// canonical constraint excluded the only stuck entity's lasso.
#[test]
fn equivariant_spec_from_an_asymmetric_snapshot() {
    let source = r"
spec FromState {
  symmetric type TaskId = 0..1
  enum Status { Pending, Done, Blocked }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish(t: TaskId) {
    requires status[t] == Pending
    status[t] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] != Done ~> status[t] == Done
    }
  }
}
";
    let status = |member: &str| FslValue::Enum {
        type_name: "Status".to_owned(),
        member: member.to_owned(),
    };
    let snapshot = BTreeMap::from([(
        "status".to_owned(),
        FslValue::Map(BTreeMap::from([
            (FslValue::Int(0), status("Blocked")),
            (FslValue::Int(1), status("Pending")),
        ])),
    )]);
    let symmetric = verify_from(source, 4, &snapshot);
    let plain = verify_from(&plain(source), 4, &snapshot);
    assert_symmetric_matches_violated_plain(&symmetric, &plain, "EveryTaskFinishes");
}

/// f25: two symmetric types whose maps mention each other
/// (`Map<A, Option<B>>`, `Map<B, Option<A>>`); canonicalising each type's
/// rows independently needs invariance under the product of both groups.
#[test]
fn mutually_referencing_maps() {
    check(
        r"
spec MutualMaps {
  symmetric type A = 0..1
  symmetric type B = 0..1
  state { f: Map<A, Option<B>>, g: Map<B, Option<A>>, stuck: Bool }
  init {
    forall a: A { f[a] = none }
    forall b: B { g[b] = none }
    stuck = false
  }
  action linkf(a: A, b: B) {
    requires f[a] == none and (forall x: A { f[x] != some(b) })
    f[a] = some(b)
  }
  action linkg(b: B, a: A) {
    requires g[b] == none and (forall y: B { g[y] != some(a) })
    g[b] = some(a)
  }
  action jam() {
    requires not stuck
    requires (forall a: A { f[a] != none }) and (forall b: B { g[b] != none })
    requires forall a: A { forall b: B { f[a] == some(b) => g[b] != some(a) } }
    stuck = true
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo Unjams {
    stuck ~> not stuck
  }
}
",
        7,
        "Unjams",
    );
}

/// f12d: a guard linking two symmetric types (`t != w`).
#[test]
fn guard_linking_two_symmetric_types() {
    check(
        r"
spec CrossTypeNeq {
  symmetric type TaskId = 0..1
  symmetric type WorkerId = 0..1
  enum Status { Pending, Done }
  state { ts: Map<TaskId, Status>, ws: Map<WorkerId, Status> }
  init {
    forall t: TaskId { ts[t] = Pending }
    forall w: WorkerId { ws[w] = Pending }
  }
  fair action finish(t: TaskId, w: WorkerId) {
    requires ts[t] == Pending and ws[w] == Pending and t != w and count(u: TaskId where ts[u] == Done) == 0
    ts[t] = Done
    ws[w] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      ts[t] == Pending ~> ts[t] == Done
    }
  }
}
",
        4,
        "EveryTaskFinishes",
    );
}

/// f13 (`Set` row variant): a `symmetric enum` member named by a literal in a
/// guard.
#[test]
fn symmetric_enum_member_literal_in_guard() {
    check(
        r"
spec EnumLiteral {
  symmetric enum Worker { A, B }
  state { busy: Set<Worker> }
  init { busy = Set {} }
  fair action mark(w: Worker) {
    requires not busy.contains(w) and w == A
    busy = busy.add(w)
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo AllMarked {
    forall w: Worker {
      not busy.contains(w) ~> busy.contains(w)
    }
  }
}
",
        4,
        "AllMarked",
    );
}

/// A stall, not a lasso: without `noop`, finishing task 0 deadlocks with task
/// 1 still pending. The other fixtures all keep an always-enabled `noop`, so
/// only this one reaches the deadlock-stall query (`check_leadsto_stagnation`).
#[test]
fn stall_with_a_literal_index() {
    let source = r"
spec StallLiteralIndex {
  symmetric type TaskId = 0..1
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish0() {
    requires status[0] == Pending
    status[0] = Done
  }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
";
    let symmetric = verify(source, 4);
    let plain = verify(&plain(source), 4);
    assert_symmetric_matches_violated_plain(&symmetric, &plain, "EveryTaskFinishes");
    let details = plain
        .leadsto_violation
        .as_ref()
        .and_then(|violation| violation.leads_to.as_ref())
        .expect("leadsTo details");
    assert_eq!(
        details.loop_start, None,
        "expected a stall, got {details:?}"
    );
}

/// Positive twin: every task can finish under fairness, so the `symmetric`
/// spec and its plain twin both verify, with identical results.
#[test]
fn fair_symmetric_spec_still_verifies() {
    let source = r"
spec FairTasks {
  symmetric type TaskId = 0..2
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish(t: TaskId) {
    requires status[t] == Pending
    status[t] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
";
    let symmetric = verify(source, 6);
    let plain = verify(&plain(source), 6);
    assert!(plain.violation.is_none(), "{plain:?}");
    assert!(plain.leadsto_violation.is_none(), "{plain:?}");
    assert_eq!(symmetric, plain);
}
