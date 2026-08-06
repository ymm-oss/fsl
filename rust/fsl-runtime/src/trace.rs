// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Shared parent-link trace reconstruction, used by any BFS that must avoid
//! cloning a full trace at every visited node (issue #697).
//!
//! A per-node `Vec<TraceStep>` clone makes a BFS frontier's memory grow with
//! both branching factor *and* path length, on top of whatever it costs to
//! clone the node's own state. Recording only a `(parent State, TraceAction)`
//! link per newly discovered state and walking that chain backward once --
//! only when a trace is actually needed -- keeps per-node memory bounded by
//! the state and one action, not the whole path so far. `explicit.rs`
//! originated this pattern for `verify_explicit_selected`; `find_boundary_violation`
//! reuses it rather than duplicating it.

use std::collections::BTreeMap;

use fsl_core::{TraceAction, TraceChange, TraceStep};

use super::State;

/// One step back from a state to its BFS parent and the action that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParentLink {
    pub(crate) parent: State,
    pub(crate) action: TraceAction,
}

/// Walk `parents` back from `final_state` to `initial_state` and render the
/// full forward [`TraceStep`] sequence, recomputing each step's field-level
/// changes from the two states it connects.
pub(crate) fn reconstruct_trace(
    initial_state: &State,
    final_state: &State,
    parents: &BTreeMap<State, ParentLink>,
) -> Vec<TraceStep> {
    let mut cursor = final_state.clone();
    let mut reversed = Vec::<(State, TraceAction)>::new();
    while let Some(link) = parents.get(&cursor) {
        reversed.push((cursor, link.action.clone()));
        cursor = link.parent.clone();
    }
    reversed.reverse();
    let mut trace = vec![TraceStep {
        step: 0,
        state: initial_state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut before = initial_state.clone();
    for (index, (state, action)) in reversed.into_iter().enumerate() {
        trace.push(TraceStep {
            step: index + 1,
            changes: state_changes(&before, &state),
            state: state.clone(),
            action: Some(action),
        });
        before = state;
    }
    trace
}

/// The field-level changes `after` made relative to `before`, keyed by state
/// variable name.
pub(crate) fn state_changes(before: &State, after: &State) -> BTreeMap<String, TraceChange> {
    after
        .iter()
        .filter_map(|(name, value)| {
            let old = &before[name];
            (old != value).then(|| {
                (
                    name.clone(),
                    TraceChange {
                        from: old.clone(),
                        to: value.clone(),
                    },
                )
            })
        })
        .collect()
}
