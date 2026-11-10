// SPDX-License-Identifier: Apache-2.0

//! #1531: replay's implicit-bound selection must control rollback without
//! disabling transition/state/change validation. No solver generates this trace.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    FsResolver, FslValue, KernelModel, SyntheticName, TraceAction, TraceChange, TraceStep,
    build_model, parse_kernel_source,
};
use fsl_runtime::{replay_trace, replay_trace_with_bounds};

fn model() -> KernelModel {
    let source = "spec ReplayRange { state { n: 0..1 } init { n = 0 } action inc() { n = n + 1 } }";
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse literal model"))
        .expect("build literal model")
}

fn trace() -> Vec<TraceStep> {
    // The third inc needs n=2 to have committed. With all bounds, step two
    // instead rolls back to n=1 (its attempted state still matches step two).
    (0..=3)
        .enumerate()
        .map(|(step, n)| TraceStep {
            step,
            state: BTreeMap::from([("n".to_owned(), FslValue::Int(n))]),
            action: (n > 0).then(|| TraceAction {
                name: "inc".to_owned(),
                params: BTreeMap::new(),
            }),
            changes: if n == 0 {
                BTreeMap::new()
            } else {
                BTreeMap::from([(
                    "n".to_owned(),
                    TraceChange {
                        from: FslValue::Int(n - 1),
                        to: FslValue::Int(n),
                    },
                )])
            },
        })
        .collect()
}

#[test]
fn unselected_range_bound_allows_replay_past_capacity() {
    replay_trace_with_bounds(model(), &trace(), Some(&BTreeSet::new()))
        .expect("unselected range bound must not roll back n=2");
}

#[test]
fn all_bounds_wrapper_still_rejects_the_same_trace() {
    let error = replay_trace(model(), &trace()).expect_err("all bounds rolls back at n=2");
    assert_eq!(error.to_string(), "trace state mismatch at step 3");
    let error = replay_trace_with_bounds(model(), &trace(), None)
        .expect_err("None preserves the all-bounds API");
    assert_eq!(error.to_string(), "trace state mismatch at step 3");
}

#[test]
fn explicitly_selected_range_bound_still_rejects_the_same_trace() {
    let checked = BTreeSet::from([SyntheticName::Bounds { state: "n" }.internal()]);
    let error = replay_trace_with_bounds(model(), &trace(), Some(&checked))
        .expect_err("a selected bound still rolls back at n=2");
    assert_eq!(error.to_string(), "trace state mismatch at step 3");
}

#[test]
fn selection_does_not_accept_a_corrupted_state() {
    let mut invalid = trace();
    invalid[3].state.insert("n".to_owned(), FslValue::Int(4));
    let error = replay_trace_with_bounds(model(), &invalid, Some(&BTreeSet::new()))
        .expect_err("selection must not bypass state validation");
    assert_eq!(error.to_string(), "trace state mismatch at step 3");
}

#[test]
fn selection_does_not_accept_a_corrupted_change_set() {
    let mut invalid = trace();
    invalid[2].changes.clear();
    let error = replay_trace_with_bounds(model(), &invalid, Some(&BTreeSet::new()))
        .expect_err("selection must not bypass change validation");
    assert_eq!(error.to_string(), "trace change set mismatch at step 2");
}
