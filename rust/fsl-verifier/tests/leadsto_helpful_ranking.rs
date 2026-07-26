// SPDX-License-Identifier: Apache-2.0

//! Native ranking-induction coverage for `helpful` (issue #473). Mirrors
//! `tests/test_helpful_leadsto.py`'s fixtures against the frozen Python
//! reference, testing `fsl_verifier::prove_ranked_leadstos` directly so the
//! ranking-specific `helpful` obligations (fairness/sticky-enabledness/
//! no-deadlock/progress) are exercised even when a shallow bounded search
//! would otherwise find a more general BMC counterexample first.

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

const HELPFUL_SRC: &str = r"
spec HelpfulPerEntity {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }

  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  action idle(c: Case) {
    level[c] = level[c]
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const NONFAIR_HELPFUL_SRC: &str = r"
spec HelpfulNonfair {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }

  action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  action idle(c: Case) {
    level[c] = level[c]
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const BLOCKED_HELPFUL_SRC: &str = r"
spec BlockedHelpful {
  type Case = 0..1
  type Level = 0..2
  state {
    level: Map<Case, Level>,
    gate: Map<Case, Bool>
  }
  init {
    forall c: Case {
      level[c] = 2
      gate[c] = false
    }
  }

  fair action step(c: Case) {
    requires gate[c]
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const FLICKERING_HELPFUL_SRC: &str = r"
spec HelpfulFlickering {
  type Phase = 0..9
  type Credit = 0..1
  state { phase: Phase, done: Bool, credit: Credit }
  init { phase = 0  done = false  credit = 1 }

  fair action helpEven() {
    requires phase % 2 == 0
    requires done == false
    done = true
    credit = 0
  }

  fair action helpOdd() {
    requires phase % 2 == 1
    requires done == false
    done = true
    credit = 0
  }

  action rotate() {
    requires done == false
    phase = (phase + 1) % 10
  }

  leadsTo Finishes {
    done == false ~> done == true
    helpful helpEven()
    helpful helpOdd()
    decreases credit
  }
}
";

const PUMPED_MEASURE_SRC: &str = r"
spec HelpfulPumpedMeasure {
  state { x: Int }
  init { x = 5 }

  fair action work() {
    requires x > 0
    x = x - 1
  }

  action pump() {
    requires x > 0
    x = x + 2
  }

  invariant NonNeg { x >= 0 }

  leadsTo Finishes {
    x > 0 ~> x == 0
    helpful work()
    decreases x
  }
}
";

fn ranked(source: &str) -> fsl_verifier::RankedLeadstoResult {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    block_on(fsl_verifier::prove_ranked_leadstos(&model, &mut solver))
        .expect("prove_ranked_leadstos")
}

#[test]
fn helpful_per_entity_measure_proves_under_interleaving() {
    let result = ranked(HELPFUL_SRC);
    assert!(result.failure.is_none(), "{:?}", result.failure);
    let proof = result
        .proofs
        .iter()
        .find(|proof| proof.name == "Responds")
        .expect("Responds ranking proof");
    assert_eq!(proof.helpful.len(), 1);
    assert_eq!(proof.helpful[0].action, "step");
}

#[test]
fn helpful_does_not_create_fairness() {
    // Negative control: `helpful step(c)` alone must not license treating
    // `step` as fair. Without a `fair` declaration on `step`, the ranking
    // proof must fail with `progress_action_not_fair`, not silently prove.
    let result = ranked(NONFAIR_HELPFUL_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "progress_action_not_fair");
    assert_eq!(failure.helpful_actions[0].action, "step");
    assert!(
        failure.hint.contains("helpful only identifies"),
        "{}",
        failure.hint
    );
}

#[test]
fn blocked_helpful_action_is_reported() {
    // Negative control: `step` is `fair` but permanently disabled by `gate`.
    // A pending obligation whose only helpful instance is never enabled must
    // fail with `helpful_action_not_enabled`, not silently prove.
    let result = ranked(BLOCKED_HELPFUL_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "helpful_action_not_enabled");
    assert_eq!(failure.helpful.len(), 1);
    assert_eq!(failure.helpful[0].action, "step");
    assert_eq!(failure.bindings.get("c"), Some(&fsl_core::FslValue::Int(0)));
}

#[test]
fn multiple_helpful_actions_with_flickering_enabledness_is_not_falsely_proved() {
    // Regression (soundness): two `helpful` actions whose enabledness
    // alternates by phase parity must not let ranking induction report
    // "proved" -- neither helpEven nor helpOdd is ever *continuously*
    // enabled while pending, so weak fairness never actually obligates
    // either one to fire, and `rotate` can cycle forever.
    let result = ranked(FLICKERING_HELPFUL_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "helpful_action_enabledness_not_sticky");
    assert!(
        failure.helpful_actions[0].action == "helpEven"
            || failure.helpful_actions[0].action == "helpOdd",
        "{:?}",
        failure.helpful_actions
    );
}

#[test]
fn non_helpful_action_pumping_the_measure_is_not_falsely_proved() {
    // Regression (soundness): a non-helpful action must not be allowed to
    // increase the measure. `work` is fair, always enabled while pending,
    // and the sole helpful match (so fairness/sticky/no-deadlock all hold),
    // but `pump` can add more to `x` than `work` ever removes.
    let result = ranked(PUMPED_MEASURE_SRC);
    let failure = result
        .failure
        .unwrap_or_else(|| panic!("expected a rank failure, got {:?}", result.proofs));
    assert_eq!(failure.kind, "non_helpful_action_increases_measure");
    assert_eq!(failure.action.as_deref(), Some("pump"));
}
