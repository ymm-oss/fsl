// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Structural regression for issue #697: `find_boundary_violation` must stop
//! at its `budget` argument -- reporting `exhausted: true` and an exact
//! `states_explored` count -- instead of growing without limit. This is the
//! primary, platform-independent control: it needs no subprocess, no
//! resource limit, and no timing assumption, unlike the memory-anchored
//! controls in `rust/fslc/tests/issue_697_all_properties_memory.rs`.
//!
//! The model below builds a fresh decimal digit onto `count` per step
//! (`count = count * 10 + x`, `x` ranging over one digit): because that
//! mapping from `(count, x)` to the next `count` is injective for every
//! `count` reachable within a handful of steps, each BFS level's distinct
//! state count is exactly `10^level`, so a modest `depth` already proves a
//! bounded reachable set far larger than any budget worth testing --
//! without this test itself doing meaningful work past the budget cutoff.

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn digit_growth_model() -> fsl_core::KernelModel {
    build_model(
        parse_kernel_source(
            "spec DigitGrowth { state { count: Int } init { count = 0 } \
             action step(x in 0..9) { count = count * 10 + x } }",
            &FsResolver::new("."),
        )
        .expect("parse model"),
    )
    .expect("build model")
}

#[test]
fn budget_stops_the_search_at_exactly_the_configured_state_count() {
    let model = digit_growth_model();

    // `10^4 == 10_000` distinct states are reachable by level 4 alone (every
    // `(count, x)` pair yields a distinct next `count`), so this model's
    // depth-8 bounded reachable set is provably far larger than a
    // 10,000-state budget.
    let probe =
        fsl_runtime::find_boundary_violation(&model, 8, 10_000).expect("probe must not error");

    assert!(
        probe.exhausted,
        "expected the search to exhaust a 10,000-state budget on a model whose bounded \
         reachable set is far larger; got {probe:?}"
    );
    assert_eq!(
        probe.states_explored, 10_000,
        "a budgeted search must stop at exactly `budget` states explored, not before or after; \
         got {probe:?}"
    );
    assert!(
        probe.finding.is_none(),
        "this model has no partial operation or type-bound violation to find; a nonempty \
         finding here would mean the budget cutoff corrupted the search, not just bounded it; \
         got {probe:?}"
    );
}

#[test]
fn a_small_budget_still_finds_a_shallow_violation_before_exhausting() {
    // Same growth model, but `count`'s declared range makes the very first
    // step's out-of-range successor a `type_bound` violation reachable well
    // inside a tiny budget -- confirming the budget bounds exploration
    // without weakening what a still-unexhausted search can find.
    let model = build_model(
        parse_kernel_source(
            "spec DigitGrowthBounded { type Digit = 0..3 state { count: Digit } \
             init { count = 0 } action step(x in 0..9) { count = x } }",
            &FsResolver::new("."),
        )
        .expect("parse model"),
    )
    .expect("build model");

    let probe = fsl_runtime::find_boundary_violation(&model, 8, 10_000).expect("probe");
    let exhausted = probe.exhausted;
    let states_explored = probe.states_explored;
    let (violation, trace) = probe.finding.unwrap_or_else(|| {
        panic!(
            "expected a type_bound violation; exhausted={exhausted} \
             states_explored={states_explored}"
        )
    });
    assert_eq!(violation.kind, "type_bound");
    assert!(
        !exhausted,
        "a found violation should short-circuit before exhaustion"
    );
    assert!(
        trace.len() >= 2,
        "expected a replayable trace, got {trace:?}"
    );
}
