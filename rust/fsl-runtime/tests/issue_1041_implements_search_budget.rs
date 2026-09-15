// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1041: `check_refinement`'s correspondence
//! walk had no state-count budget (`visited: BTreeSet<State>` grew without a
//! cap, unlike `find_boundary_violation`'s own `budget` parameter). P1
//! (`SIGMA-P1.md`) measured this reaching ~3.8 GB RSS (`visited.len() ==
//! 132,302`) on one corpus-scale inline `implements` domain, with a flat
//! (~7.8 MB) control run against the same domain with the `implements`
//! block removed -- confirming the growth is specific to the correspondence
//! walk, not general parsing/type-checking cost.
//!
//! This file calibrates the cutoff mechanism itself
//! (`check_refinement_with_budget`) against a tiny, four-state fixture with
//! an injected budget -- "inject a small budget, fall; remove it, pass"
//! (`PLAN-1041.md` §"test-first") -- so the calibration is fast and does not
//! need a corpus-scale domain. `rust/fslc/tests/issue_1041_implements_budget.rs`
//! separately proves the fixed production constant
//! (`fsl_runtime::IMPLEMENTS_SEARCH_BUDGET`) is actually wired through both
//! CLI entry points (`check` and `verify`).

use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

// Four reachable states (n = 0, 1, 2, 3), all within `depth = 3` of the
// deterministic `init { n = 0 }` root. `maps auto` on an identically-shaped
// abstraction is a correct refinement, so a generous budget must report
// `refines` -- the fixture's own passing verdict is not the thing under
// test, only a stable baseline to detect against.
const IMPL: &str = "spec MinCounter { type Qty = 0..3 state { n: Qty } init { n = 0 } \
     action bump() { requires n < 3  n = n + 1 } }";
const ABS: &str = "spec MinCounterAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } \
     action bump() { requires n < 3  n = n + 1 } }";

fn fixture() -> (
    fsl_core::KernelModel,
    fsl_core::KernelModel,
    fsl_core::Refinement,
) {
    let implementation = model(IMPL);
    let abstraction = model(ABS);
    let mapping = parse_refinement(
        "refinement M { impl MinCounter abs MinCounterAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    (implementation, abstraction, mapping)
}

/// Red half of the calibration: a budget smaller than the fixture's four
/// reachable states must cut the walk off before it decides `refines` or
/// `refinement_failed` -- reporting either at this point would be a false
/// result, since the search never actually explored the unvisited rest of
/// the state space.
#[test]
fn a_budget_smaller_than_the_reachable_set_reports_exhaustion_not_a_verdict() {
    let (implementation, abstraction, mapping) = fixture();

    let checked =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 3, 2)
            .expect("check_refinement_with_budget runs");

    let explored = checked
        .budget_exhausted
        .expect("a budget of 2 must cut off a 4-state reachable set");
    assert!(
        explored >= 2,
        "cutoff must not fire before visited.len() reaches the budget: {explored}"
    );
    assert!(
        checked.failure.is_none(),
        "an exhausted walk must not also report a decided refinement failure: {:?}",
        checked.failure
    );
    assert!(
        checked.impl_violation.is_none(),
        "an exhausted walk must not also report a decided impl violation: {:?}",
        checked.impl_violation
    );
}

/// Green half of the calibration: the same fixture, same depth, with the
/// budget removed (made generous) must complete normally and report the
/// fixture's real verdict (`refines`) -- proving the cutoff in the red case
/// above was the budget, not some other defect in the fixture or depth.
#[test]
fn removing_the_budget_lets_the_same_fixture_reach_its_real_verdict() {
    let (implementation, abstraction, mapping) = fixture();

    let checked = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        3,
        1_000,
    )
    .expect("check_refinement_with_budget runs");

    assert!(
        checked.budget_exhausted.is_none(),
        "a budget of 1,000 must not cut off a 4-state reachable set: {:?}",
        checked.budget_exhausted
    );
    assert!(checked.failure.is_none(), "{:?}", checked.failure);
    assert!(
        checked.impl_violation.is_none(),
        "{:?}",
        checked.impl_violation
    );
}

/// `check_refinement` (the production entry point, no budget parameter) must
/// still complete this small fixture normally under the real fixed
/// constant -- a preservation control proving the new budget path does not
/// change behavior for domains far below it.
#[test]
fn the_production_entry_point_is_unaffected_by_the_budget_for_a_small_domain() {
    let (implementation, abstraction, mapping) = fixture();

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("check_refinement runs");

    assert!(
        checked.budget_exhausted.is_none(),
        "{:?}",
        checked.budget_exhausted
    );
    assert!(checked.failure.is_none(), "{:?}", checked.failure);
    assert!(
        checked.impl_violation.is_none(),
        "{:?}",
        checked.impl_violation
    );
}
