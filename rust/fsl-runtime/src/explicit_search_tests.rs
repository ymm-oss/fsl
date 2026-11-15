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
