// SPDX-License-Identifier: Apache-2.0

//! Native `symmetric type` / `symmetric enum` liveness symmetry reduction
//! (issue #461). Mirrors `tests/test_temporal.py`'s
//! `SYMMETRIC_TASKS`/`PLAIN_TASKS` fixtures: the `symmetric`-tagged spec and
//! its non-symmetric twin must agree on both the positive path (still
//! `verified`) and the negative control (a violation whose only stalled
//! entity is one specific `TaskId`, exercising that the canonical-loop-head
//! constraint cannot hide a genuine per-entity counterexample).

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

const SYMMETRIC_TASKS: &str = r"
spec SymmetricTasks {
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
  invariant StatusValid { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
";

fn plain_tasks() -> String {
    SYMMETRIC_TASKS.replace("symmetric type TaskId", "type TaskId")
}

fn symmetric_tasks_unfair() -> String {
    SYMMETRIC_TASKS.replace("fair action finish", "action finish")
}

fn plain_tasks_unfair() -> String {
    plain_tasks().replace("fair action finish", "action finish")
}

const SYMMETRIC_WORKERS: &str = r"
spec SymmetricWorkers {
  symmetric enum Worker { A, B, C }
  state { busy: Set<Worker> }
  init { busy = Set {} }
  fair action mark(w: Worker) {
    requires not busy.contains(w)
    busy = busy.add(w)
  }
  invariant AlwaysTrue { true }
  leadsTo AllMarked {
    forall w: Worker {
      not busy.contains(w) ~> busy.contains(w)
    }
  }
}
";

const SYMMETRIC_WORKERS_UNFAIR: &str = r"
spec SymmetricWorkers {
  symmetric enum Worker { A, B, C }
  state { busy: Set<Worker> }
  init { busy = Set {} }
  action mark(w: Worker) {
    requires not busy.contains(w)
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
";

async fn run(source: &str, depth: usize) -> fsl_verifier::BmcResult {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    fsl_verifier::verify_bounded(&model, &mut solver, depth)
        .await
        .expect("verify_bounded")
}

#[test]
fn symmetric_and_plain_agree_when_fair_and_verified() {
    let symmetric = block_on(run(SYMMETRIC_TASKS, 6));
    let plain = block_on(run(&plain_tasks(), 6));

    assert!(symmetric.violation.is_none(), "{symmetric:?}");
    assert!(symmetric.leadsto_violation.is_none(), "{symmetric:?}");
    assert!(plain.violation.is_none(), "{plain:?}");
    assert!(plain.leadsto_violation.is_none(), "{plain:?}");
}

#[test]
fn symmetric_reduction_does_not_hide_a_single_entity_stall() {
    // Negative control: without `fair`, exactly one TaskId can stall forever
    // (the others may or may not finish -- the solver picks a witness). The
    // canonical-loop-head constraint only fixes the *shape* of the loop-head
    // state under entity renaming; it must never turn this genuine violation
    // into a false `verified`.
    let symmetric = block_on(run(&symmetric_tasks_unfair(), 5));
    let plain = block_on(run(&plain_tasks_unfair(), 5));

    let symmetric_violation = symmetric
        .leadsto_violation
        .as_ref()
        .unwrap_or_else(|| panic!("expected a leadsTo violation, got {symmetric:?}"));
    let plain_violation = plain
        .leadsto_violation
        .as_ref()
        .unwrap_or_else(|| panic!("expected a leadsTo violation, got {plain:?}"));

    assert_eq!(symmetric_violation.kind, "leadsTo");
    assert_eq!(plain_violation.kind, "leadsTo");
    assert_eq!(symmetric_violation.name, "EveryTaskFinishes");
    assert_eq!(plain_violation.name, "EveryTaskFinishes");
}

#[test]
fn symmetric_enum_set_membership_still_verifies_when_fair() {
    // Exercises the `Set<SymmetricType>` row (membership indicator), not just
    // `Map<SymmetricType, V>`.
    let result = block_on(run(SYMMETRIC_WORKERS, 6));
    assert!(result.violation.is_none(), "{result:?}");
    assert!(result.leadsto_violation.is_none(), "{result:?}");
}

#[test]
fn symmetric_enum_set_membership_negative_control_still_finds_single_entity_stall() {
    // Negative control for the `Set<SymmetricType>` row: without `fair`,
    // exactly one Worker can stall forever behind an always-enabled `noop`,
    // and the canonical constraint must not hide it.
    let result = block_on(run(SYMMETRIC_WORKERS_UNFAIR, 5));
    let violation = result
        .leadsto_violation
        .as_ref()
        .unwrap_or_else(|| panic!("expected a leadsTo violation, got {result:?}"));
    assert_eq!(violation.kind, "leadsTo");
    assert_eq!(violation.name, "AllMarked");
}

// A *joint* (non-per-entity) leadsTo property over 7 symmetric entities: the
// trigger/response are single Boolean expressions quantifying over all of
// `TaskId` (`forall t { status[t] == Pending }`), not `forall t { P(t) ~>
// Q(t) }` sugar, so leadsTo_bindings does not decompose this into 7
// independent checks -- the lasso/stall search explores the joint N=7
// per-entity-status configuration space, which is exactly where the
// canonical-loop-head/stalled-state constraint has room to collapse
// permutation-equivalent states.
const SYMMETRIC_JOINT_N7: &str = r"
spec SymmetricJointN7 {
  symmetric type TaskId = 0..6
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init { forall t: TaskId { status[t] = Pending } }
  fair action finish(t: TaskId) {
    requires status[t] == Pending
    status[t] = Done
  }
  action noop() { }
  invariant AlwaysTrue { true }
  leadsTo AllDone {
    (forall t: TaskId { status[t] == Pending }) ~> (forall t: TaskId { status[t] == Done })
  }
}
";

#[test]
fn symmetry_reduction_measurably_reduces_solver_work_on_a_joint_property() {
    // Discriminating test (not merely green-either-way): fails if the
    // `symmetry::canonical_constraint` call sites are removed from
    // `check_leadstos`/`check_leadsto_stagnation` in bmc.rs.
    //
    // Verified by ablation (replacing both call sites with
    // `solver.bool_value(true)` and restoring them): on this exact spec at
    // depth 5, Z3's solver-check count is unaffected (97 either way -- the
    // lasso/stall loop *structure* never depends on symmetry), but
    // `propagations` is 16771 with the reduction active and 34396 with it
    // ablated (conflicts: 449 vs. 775; decisions: 767 vs. 1414). The
    // threshold below sits roughly in the middle, with >30% headroom on
    // both sides, so ordinary Z3-version/build noise should not flip it;
    // only removing the constraint (or another regression of comparable
    // size) would.
    let kernel = parse_kernel_source(SYMMETRIC_JOINT_N7, &FsResolver::new(std::env::temp_dir()))
        .expect("parse");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result =
        block_on(fsl_verifier::verify_bounded(&model, &mut solver, 5)).expect("verify_bounded");
    assert!(result.violation.is_none(), "{result:?}");
    assert!(result.leadsto_violation.is_none(), "{result:?}");

    let statistics = fsl_solver::SmtSolver::statistics(&solver);
    let propagations = statistics
        .solver
        .propagations
        .expect("native-z3 backend reports propagations");
    assert!(
        propagations < 25_000,
        "propagations={propagations} (checks={}); expected well under the ablated-symmetry \
         baseline of ~34396 -- symmetry reduction may not be reaching check_leadstos/\
         check_leadsto_stagnation",
        statistics.solver.checks
    );
}
