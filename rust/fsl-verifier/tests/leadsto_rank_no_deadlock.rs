// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The ranked-`leadsTo` `no_deadlock` obligation (issue #1189).
//!
//! `docs/design/DESIGN-induction.md` §2.3 requires `Inv(s) ∧ P(s) ∧ ¬Q(s) ⇒
//! enabled(s)` for a ranked `leadsTo`, and the frozen Python reference checks
//! it in `_prove_leadsto_rank_no_deadlock` (`rank_failure: "deadlock"`, or
//! `helpful_action_not_enabled` with `helpful`). The native ranking asserted
//! the transition relation on its pre-state, which forces some action to be
//! enabled there, so a pending state with nothing enabled was never examined
//! and a false `leadsTo` was reported proved. The obligation ranges over
//! invariant states, not reachable ones: an unreachable deadlocked pending
//! state blocks the proof until an invariant excludes it (§2.4).

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

fn ranked(source: &str) -> fsl_verifier::RankedLeadstoResult {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    block_on(fsl_verifier::prove_ranked_leadstos(&model, &mut solver))
        .expect("prove_ranked_leadstos")
}

fn failure(source: &str) -> fsl_verifier::RankFailure {
    let result = ranked(source);
    result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs))
}

fn pending_state_x(failure: &fsl_verifier::RankFailure) -> i64 {
    assert_eq!(failure.trace.len(), 1, "{:?}", failure.trace);
    match failure.trace[0].state.get("x") {
        Some(fsl_core::FslValue::Int(value)) => *value,
        other => panic!("expected an Int x in the CTI, got {other:?}"),
    }
}

/// The issue's repro: `x = 2` is pending (`x > 0`, `x != 0`) and `dec` is
/// disabled there, so the run stops short of `x == 0`.
const DEADLOCK_SRC: &str = r"
spec DeadlockRank {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  action dec() { requires x > 2  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

/// No-false-alarm control: `dec` is enabled in every pending state.
const DRAIN_SRC: &str = r"
spec DrainRank {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  action dec() { requires x > 0  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

/// Every pending state with `stuck == true` is deadlocked, but none is
/// reachable from `init`. Induction reasons over invariant states, so the
/// proof must fail here (the Python reference agrees) ...
const UNREACHABLE_DEADLOCK_SRC: &str = r"
spec UnreachableDeadlockRank {
  type X = 0..5
  state { x: X, stuck: Bool }
  init { x = 5  stuck = false }
  action dec() { requires x > 0 and stuck == false  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

/// ... and succeed once an invariant excludes those states.
const UNREACHABLE_DEADLOCK_WITH_INVARIANT_SRC: &str = r"
spec UnreachableDeadlockRankInv {
  type X = 0..5
  state { x: X, stuck: Bool }
  init { x = 5  stuck = false }
  action dec() { requires x > 0 and stuck == false  x = x - 1 }
  invariant NeverStuck { stuck == false }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

/// With `helpful`, the same pre-state restriction hid a pending state in
/// which the only (helpful) action is disabled.
const HELPFUL_DEADLOCK_SRC: &str = r"
spec HelpfulTotalDeadlock {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  fair action dec() { requires x > 2  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 helpful dec() decreases x }
}
";

#[test]
fn a_pending_state_with_no_enabled_action_fails_the_ranking_proof() {
    let failure = failure(DEADLOCK_SRC);
    assert_eq!(failure.name, "Drain");
    assert_eq!(failure.kind, "deadlock");
    assert_eq!(failure.action, None);
    assert!(failure.helpful.is_empty() && failure.helpful_actions.is_empty());
    assert_eq!(
        failure.message,
        "leadsTo 'Drain' can be pending in a state with no enabled action"
    );
    let x = pending_state_x(&failure);
    assert!(
        (1..=2).contains(&x),
        "x = {x} is not a deadlocked pending state"
    );
    assert_eq!(failure.measure_value, Some(x));
}

#[test]
fn a_ranked_leadsto_with_an_enabled_action_in_every_pending_state_still_proves() {
    let result = ranked(DRAIN_SRC);
    assert!(result.failure.is_none(), "{:?}", result.failure);
    assert_eq!(
        result
            .proofs
            .iter()
            .map(|proof| proof.name.as_str())
            .collect::<Vec<_>>(),
        ["Drain"]
    );
}

#[test]
fn an_unreachable_deadlocked_pending_state_blocks_the_proof_until_an_invariant_excludes_it() {
    let failure = failure(UNREACHABLE_DEADLOCK_SRC);
    assert_eq!(failure.kind, "deadlock");
    assert_eq!(
        failure.trace[0].state.get("stuck"),
        Some(&fsl_core::FslValue::Bool(true))
    );

    let result = ranked(UNREACHABLE_DEADLOCK_WITH_INVARIANT_SRC);
    assert!(result.failure.is_none(), "{:?}", result.failure);
    assert_eq!(result.proofs.len(), 1);
}

#[test]
fn a_pending_state_with_no_enabled_helpful_action_fails_even_when_nothing_else_is_enabled() {
    let failure = failure(HELPFUL_DEADLOCK_SRC);
    assert_eq!(failure.kind, "helpful_action_not_enabled");
    assert_eq!(failure.helpful_actions.len(), 1);
    assert_eq!(failure.helpful_actions[0].action, "dec");
    let x = pending_state_x(&failure);
    assert!(
        (1..=2).contains(&x),
        "x = {x} is not a deadlocked pending state"
    );
}
