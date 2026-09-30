// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1149: which ranked `leadsTo` properties a BMC run may drop the
//! bounded fair-lasso search for (`fsl_verifier::ranked_leadsto_lasso_discharges`),
//! and that dropping it leaves the bounded verdict unchanged
//! (`fsl_verifier::verify_bounded_discharging`).
//!
//! The CLI-level verdict, witness, and cost evidence lives in
//! `rust/fslc/tests/issue_1149_ranked_leadsto_lasso.rs`; this file pins the
//! two library-level premises the soundness argument in
//! `docs/design/DESIGN-induction.md` §2.5 rests on.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, KernelModel, build_model, parse_kernel_source};

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

fn discharges(model: &KernelModel, checked_bounds: Option<&BTreeSet<String>>) -> BTreeSet<usize> {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    block_on(fsl_verifier::ranked_leadsto_lasso_discharges(
        model,
        &mut solver,
        checked_bounds,
    ))
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn positions(positions: &[usize]) -> BTreeSet<usize> {
    positions.iter().copied().collect()
}

/// `5 - x` is non-negative only because `x: 0..5` is bounded above: the
/// ranking needs the implicit `_bounds_x` premise. A BMC run that checks that
/// bound at every unrolled step may use it; a run whose property selection
/// drops it (`--property`, `--exclude-property _bounds_x`) must not, because
/// its unrolled states are then free to leave the range.
const NEEDS_BOUND: &str = r"
spec NeedsBound {
  state { x: 0..5 }
  init { x = 0 }
  action inc() {
    requires x != 5
    x = x + 1
  }
  leadsTo Reach { x != 5 ~> x == 5 decreases 5 - x }
}
";

#[test]
fn a_ranking_that_needs_a_type_bound_is_used_only_when_the_run_checks_that_bound() {
    let model = model(NEEDS_BOUND);
    assert_eq!(discharges(&model, None), positions(&[0]));
    let selected = names(&["_bounds_x"]);
    assert_eq!(discharges(&model, Some(&selected)), positions(&[0]));
    // Negative control: without the bound premise `x = 6` is pending with
    // measure -1, so the ranking fails and nothing is discharged.
    assert_eq!(discharges(&model, Some(&BTreeSet::new())), BTreeSet::new());
}

/// Two ranked properties, the first false: the ranking stops at its first
/// failure, so the second (true, rankable) property is not discharged either.
/// Only what was proved before the failure may skip its search.
const FIRST_FAILS: &str = r"
spec FirstFails {
  state { x: 0..1, y: 0..3 }
  init { x = 0  y = 0 }
  fair action flip() {
    x = 1 - x
  }
  action climb() {
    requires y < 3
    y = y + 1
  }
  leadsTo Never { x == 0 ~> x == 2 decreases x }
  leadsTo Climbs { y < 3 ~> y == 3 decreases 3 - y }
}
";

#[test]
fn a_failed_ranking_discharges_nothing_after_it() {
    assert_eq!(discharges(&model(FIRST_FAILS), None), BTreeSet::new());
    // Swapping the declaration order lets `Climbs` be proved first. It is
    // still not discharged, because `flip` leaves `y < 3` pending without
    // decreasing `3 - y`: every action must make progress when no `helpful`
    // is declared.
    let swapped = FIRST_FAILS.replace(
        "  leadsTo Never { x == 0 ~> x == 2 decreases x }\n  leadsTo Climbs { y < 3 ~> y == 3 decreases 3 - y }",
        "  leadsTo Climbs { y < 3 ~> y == 3 decreases 3 - y }\n  leadsTo Never { x == 0 ~> x == 2 decreases x }",
    );
    assert_ne!(swapped, FIRST_FAILS, "fixture edit must apply");
    assert_eq!(discharges(&model(&swapped), None), BTreeSet::new());
}

/// An unranked `leadsTo` is never discharged, and a spec without any ranked
/// one asks the solver nothing.
#[test]
fn an_unranked_leadsto_is_never_discharged() {
    let unranked = NEEDS_BOUND.replace(" decreases 5 - x", "");
    assert_ne!(unranked, NEEDS_BOUND, "fixture edit must apply");
    let model = model(&unranked);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let discharged = block_on(fsl_verifier::ranked_leadsto_lasso_discharges(
        &model,
        &mut solver,
        None,
    ));
    assert!(discharged.is_empty());
    assert_eq!(fsl_solver::SmtSolver::statistics(&solver).solver.checks, 0);
}

/// A discharged property keeps its verdict: the full and the discharging
/// search agree on a true ranked property at several depths.
#[test]
fn a_discharged_lasso_search_keeps_the_bounded_verdict() {
    let model = model(NEEDS_BOUND);
    let discharged = discharges(&model, None);
    assert_eq!(discharged, positions(&[0]));
    for depth in [1, 4, 8] {
        let mut full_solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let full = block_on(fsl_verifier::verify_bounded(
            &model,
            &mut full_solver,
            depth,
        ))
        .expect("full search");
        let mut fast_solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let fast = block_on(fsl_verifier::verify_bounded_discharging(
            &model,
            &mut fast_solver,
            depth,
            None,
            None,
            &discharged,
        ))
        .expect("discharging search");
        assert_eq!(fast, full, "depth {depth}");
        assert!(
            fsl_solver::SmtSolver::statistics(&fast_solver)
                .solver
                .checks
                < fsl_solver::SmtSolver::statistics(&full_solver)
                    .solver
                    .checks,
            "depth {depth}: the lasso probes must actually be skipped"
        );
    }
}

/// `check` accepts two `leadsTo` blocks with the same name, so a discharge is
/// a position in `model.leadstos`, never a name: the rankable `L` must not
/// withdraw the search of the unranked (and false) `L` next to it.
const DUPLICATE_NAMES: &str = r"
spec DuplicateNames {
  state { x: 0..1, y: 0..5 }
  init { x = 0  y = 0 }
  action flip() {
    requires y == 5
    x = 1 - x
  }
  action inc() {
    requires y < 5
    y = y + 1
  }
  leadsTo L { y < 5 ~> y == 5 decreases 5 - y }
  leadsTo L { x == 0 ~> x == 2 }
}
";

#[test]
fn a_discharge_names_a_position_not_a_leadsto_name() {
    let model = model(DUPLICATE_NAMES);
    assert_eq!(model.leadstos.len(), 2);
    let discharged = discharges(&model, None);
    assert_eq!(discharged, positions(&[0]));
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded_discharging(
        &model,
        &mut solver,
        8,
        None,
        None,
        &discharged,
    ))
    .expect("discharging search");
    let violation = result.leadsto_violation.expect("the unranked L is false");
    assert_eq!(violation.name, "L");
    let details = violation.leads_to.expect("leadsTo details");
    assert!(details.loop_start.is_some(), "{details:?}");
}
