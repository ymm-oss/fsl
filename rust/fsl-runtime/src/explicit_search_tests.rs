// SPDX-License-Identifier: Apache-2.0

//! The explicit engine and the concrete boundary probe step each candidate
//! from the state it was enumerated at, without evaluating its guards again
//! (#1851). The counts below are produced by the work counter, so a search
//! that goes back to re-evaluating the guards of every stepped instance
//! fails without a deadline.

use super::evaluation_counts::GUARD_EVALUATIONS;
use super::*;
use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn model(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse")).expect("model")
}

/// Five reachable states (`n` = 0..4), five action instances in each, one
/// of them enabled: `inc(0)` below 4 and `reset` at 4, which returns to an
/// explored state.
const COUNTER: &str = "spec Counter {
  type Slot = 0..3
  state { n: 0..4 }
  init { n = 0 }
  action inc(s: Slot) {
    requires s == 0 and n < 4
    n = n + 1
  }
  action reset() {
    requires n == 4
    n = 0
  }
  invariant Small { n <= 4 }
}";

const STATES: usize = 5;
const INSTANCES: usize = 5;

fn counted<T>(body: impl FnOnce() -> T) -> (T, usize) {
    GUARD_EVALUATIONS.with(|count| count.set(0));
    let result = body();
    (result, GUARD_EVALUATIONS.with(std::cell::Cell::get))
}

/// Detector: every instance's guards are evaluated once per explored
/// state, and stepping the five enabled instances adds none.
#[test]
fn explicit_search_evaluates_each_instance_guard_once_per_state() {
    let (result, evaluations) = counted(|| verify_explicit(model(COUNTER), 10, 1_000));
    let result = result.expect("explicit");
    assert!(result.closure, "{result:?}");
    assert_eq!(result.states_explored, STATES);
    assert_eq!(
        result
            .action_profile
            .values()
            .map(|stats| stats.fired)
            .sum::<usize>(),
        STATES
    );
    assert_eq!(evaluations, STATES * INSTANCES);
}

/// Detector: the same count for the concrete boundary probe.
#[test]
fn boundary_probe_evaluates_each_instance_guard_once_per_state() {
    let (probe, evaluations) =
        counted(|| find_boundary_violation(&model(COUNTER), 10, CONCRETE_PROBE_BUDGET));
    let probe = probe.expect("probe");
    assert_eq!(probe.finding, None);
    assert!(!probe.exhausted);
    assert_eq!(probe.states_explored, STATES);
    assert_eq!(evaluations, STATES * INSTANCES);
}

/// `n = 1` is stored when `inc` first reaches it; `back` reaches it again
/// from `n = 2`, where `property` indexes the empty `xs`.
fn stored_edge(property: &str) -> String {
    format!(
        "spec StoredEdge {{
  state {{
    n: 0..2,
    xs: Seq<0..1, 1>
  }}
  init {{
    n = 0
    xs = Seq {{}}
  }}
  action inc() {{
    requires n < 2
    n = n + 1
  }}
  action back() {{
    requires n == 2
    n = 1
  }}
  {property} FromTop {{ old(n) == 2 => old(xs)[0] == 0 }}
}}"
    )
}

fn stored_edge_finding(property: &str) -> (Violation, Option<String>) {
    let probe = find_boundary_violation(&model(&stored_edge(property)), 5, CONCRETE_PROBE_BUDGET)
        .expect("probe");
    let (violation, trace) = probe.finding.expect("finding");
    let action = trace
        .last()
        .and_then(|step| step.action.as_ref())
        .map(|action| action.name.clone());
    (violation, action)
}

/// A successor the probe already stored skips only its state checks: the
/// `trans` property of the edge that reaches it again is still evaluated.
#[test]
fn boundary_probe_checks_trans_on_an_edge_into_a_stored_state() {
    let (violation, action) = stored_edge_finding("trans");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.step, 3);
    assert_eq!(action.as_deref(), Some("back"));
}

/// An invariant that reads `old()` depends on the edge too, so a stored
/// successor is checked against it again.
#[test]
fn boundary_probe_rechecks_an_old_reading_invariant_on_a_stored_state() {
    let (violation, action) = stored_edge_finding("invariant");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.step, 3);
    assert_eq!(action.as_deref(), Some("back"));
}

/// The explicit engine skips only the state checks of a stored successor:
/// the `trans` property of the edge that reaches it again is still evaluated.
#[test]
fn explicit_search_checks_trans_on_an_edge_into_a_stored_state() {
    let result = verify_explicit(model(&stored_edge("trans")), 5, 1_000).expect("explicit");
    let violation = result.violation.expect("violation");
    assert_eq!(violation.violation.kind, "partial_op");
    assert_eq!(violation.violation.step, 3);
    let action = violation
        .trace
        .last()
        .and_then(|step| step.action.as_ref())
        .map(|action| action.name.clone());
    assert_eq!(action.as_deref(), Some("back"));
}

/// [`COUNTER`] with an invariant that reads `old()` (the explicit engine also
/// checks it without one, so its first operand always holds). No search asks
/// its store whether a successor is already stored, so each encodes the
/// successor when it stores it.
const COUNTER_READING_OLD: &str = "spec Counter {
  type Slot = 0..3
  state { n: 0..4 }
  init { n = 0 }
  action inc(s: Slot) {
    requires s == 0 and n < 4
    n = n + 1
  }
  action reset() {
    requires n == 4
    n = 0
  }
  invariant Bounded { n >= 0 or old(n) >= 0 }
}";

#[test]
fn searches_store_each_successor_when_an_invariant_reads_old() {
    let counter = model(COUNTER_READING_OLD);
    assert!(invariants_read_old(&counter));
    let result = verify_explicit(counter.clone(), 10, 1_000).expect("explicit");
    assert!(result.closure, "{result:?}");
    assert_eq!(result.states_explored, STATES);
    let probe = find_boundary_violation(&counter, 10, CONCRETE_PROBE_BUDGET).expect("probe");
    assert_eq!(probe.finding, None);
    assert_eq!(probe.states_explored, STATES);
    // `n == 4` is reached only after four distinct successors are stored.
    let four = Expr::Binary {
        op: "==".to_owned(),
        left: Box::new(Expr::Var("n".to_owned())),
        right: Box::new(Expr::Num(4)),
    };
    assert_eq!(
        expression_reachability(&counter, &[four], 10, CONCRETE_PROBE_BUDGET).expect("walk"),
        [Reachability::Reachable]
    );
}
