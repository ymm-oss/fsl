// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Exercising evidence for the four `RankFailure.kind` values that no other
//! checked-in test reaches (issue #646, `rust/fsl-verifier/src/violation_kind.rs`).
//!
//! `tests/leadsto_helpful_ranking.rs` already exercises `progress_action_not_fair`,
//! `helpful_action_enabledness_not_sticky`, `helpful_action_not_enabled`, and
//! `non_helpful_action_increases_measure`. Reading `prove_ranked_leadstos`
//! directly (not the #646 issue text, which under-reported this vocabulary)
//! found four more reachable `kind` values with no exercising fixture
//! anywhere in the corpus: `unbounded_below`, `non_decreasing_action`,
//! `pending_not_preserved`, `non_decreasing_helpful_action`. Each fixture
//! here was confirmed by running it through the real solver and inspecting
//! the resulting `RankFailure.kind`, not derived on paper.

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

/// `x` is an unbounded `Int` measure: a free state satisfying `P` (`done ==
/// false`) can still have `x < 0`, so the unconditional pre-check at the top
/// of `prove_ranked_leadstos` (which asks nothing about reachability, only
/// whether `pending && measure < 0` is satisfiable at all) fires first.
const UNBOUNDED_BELOW_SRC: &str = r"
spec RankUnboundedBelow {
  state { x: Int, done: Bool }
  init { x = 0  done = false }
  action step() {
    requires done == false
    x = x - 1
  }
  action finish() {
    requires x < -3
    done = true
  }
  leadsTo Finishes {
    done == false ~> done == true
    decreases x
  }
}
";

/// No `helpful` clause is declared, so every action -- not only a `helpful`
/// one -- must keep `P` true and strictly decrease the measure, or make `Q`
/// true, whenever it fires while pending. `stall` does neither.
const NON_DECREASING_SRC: &str = r"
spec RankNonDecreasing {
  type Level = 0..5
  state { level: Level, done: Bool }
  init { level = 5  done = false }
  action wiggle() {
    requires level > 0
    level = level - 1
  }
  action stall() {
    requires done == false
    level = level
  }
  leadsTo Finishes {
    done == false ~> done == true
    decreases level
  }
}
";

/// `derail` is a non-`helpful` action that can move the state to `phase ==
/// 5`, where neither `P` (`phase == 0`) nor `Q` (`phase == 2`) holds -- the
/// `pending_not_preserved` branch, distinct from every other rank-failure
/// kind, which all keep `p1` true.
const PENDING_NOT_PRESERVED_SRC: &str = r"
spec RankPendingNotPreserved {
  type Phase = 0..5
  state { phase: Phase }
  init { phase = 0 }
  fair action advance() {
    requires phase == 0
    phase = 2
  }
  action derail() {
    requires phase == 0
    phase = 5
  }
  leadsTo Finishes {
    phase == 0 ~> phase == 2
    helpful advance()
    decreases (2 - phase)
  }
}
";

/// `stallHelpful` is the sole `helpful` action, `fair`, and always enabled
/// while pending, but its firing leaves the measure unchanged and `P` still
/// true -- `non_decreasing_helpful_action`, distinct from
/// `non_helpful_action_increases_measure` (a *non*-helpful action) and from
/// `non_decreasing_action` (no `helpful` declared at all).
const NON_DECREASING_HELPFUL_SRC: &str = r"
spec RankNonDecreasingHelpful {
  type Credit = 0..5
  state { progressed: Bool, credit: Credit }
  init { progressed = false  credit = 3 }
  fair action stallHelpful() {
    requires progressed == false
    credit = credit
  }
  leadsTo Finishes {
    progressed == false ~> progressed == true
    helpful stallHelpful()
    decreases credit
  }
}
";

#[test]
fn unbounded_measure_is_reported_as_unbounded_below() {
    let result = ranked(UNBOUNDED_BELOW_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "unbounded_below");
}

#[test]
fn a_non_helpful_stall_with_no_helpful_declared_is_non_decreasing_action() {
    let result = ranked(NON_DECREASING_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "non_decreasing_action");
    assert_eq!(failure.action.as_deref(), Some("stall"));
}

#[test]
fn a_non_helpful_action_leaving_both_p_and_q_false_is_pending_not_preserved() {
    let result = ranked(PENDING_NOT_PRESERVED_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "pending_not_preserved");
    assert_eq!(failure.action.as_deref(), Some("derail"));
}

#[test]
fn a_helpful_action_that_stalls_the_measure_is_non_decreasing_helpful_action() {
    let result = ranked(NON_DECREASING_HELPFUL_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "non_decreasing_helpful_action");
    assert_eq!(failure.action.as_deref(), Some("stallHelpful"));
}
