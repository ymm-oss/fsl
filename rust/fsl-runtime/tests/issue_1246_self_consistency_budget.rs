// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1246: `check_refinement_with_budget` runs a self-consistency
//! pre-pass (`first_self_violation`) over the implementation's whole
//! reachable set before its correspondence walk. #1060 (#1041) budgeted only
//! the walk, so the pre-pass still held every reachable state: #1041's
//! original reproducer exceeds 6 GB inside it. The pre-pass now stops at the
//! same budget and reports `budget_exhausted`, the walk's own vocabulary.
//!
//! The fixture's only self-violation (a type bound) is at step 4, after four
//! in-bound states. A budget of 2 stops the pre-pass before it gets there; an
//! unbudgeted pre-pass finds the violation instead, which is what the red
//! case observes. `rust/fslc/tests/issue_1246_self_consistency_budget.rs`
//! covers the production constant through the CLI.

use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};
use fsl_runtime::RefinementVerdict;

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

/// `bump` has no guard, so the fifth reachable value (`n = 4`) leaves
/// `IQty`: a `type_bound` self-violation first reached at step 4.
fn late_self_violation_fixture() -> (
    fsl_core::KernelModel,
    fsl_core::KernelModel,
    fsl_core::Refinement,
) {
    let implementation = model(
        "spec LateBroken { type IQty = 0..3 state { n: IQty } init { n = 0 } \
         action bump() { n = n + 1 } }",
    );
    let abstraction = model(
        "spec LateAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } \
         action bump() { requires n < 3  n = n + 1 } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl LateBroken abs LateAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    (implementation, abstraction, mapping)
}

/// Calibration: a budget covering the reachable set lets the pre-pass reach
/// step 4 and report the impl's own violation.
#[test]
fn a_budget_covering_the_reachable_set_finds_the_late_self_violation() {
    let (implementation, abstraction, mapping) = late_self_violation_fixture();

    let checked = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        6,
        1_000,
    )
    .expect("check_refinement_with_budget runs");

    match checked.verdict() {
        RefinementVerdict::ImplViolated { violation, .. } => {
            assert_eq!(violation.kind, "type_bound");
            assert_eq!(violation.step, 4);
        }
        other => panic!("expected the step-4 self-violation, got {other:?}"),
    }
}

/// A budget of 2 stops the pre-pass at its second state, before step 4. The
/// violation is in the part it never reached, so the verdict is the cutoff,
/// reported as the walk reports one: `states_explored` is the pre-pass's
/// `visited.len()` at the cutoff, which equals the budget.
#[test]
fn a_pre_pass_cut_off_before_the_self_violation_is_a_budget_verdict() {
    let (implementation, abstraction, mapping) = late_self_violation_fixture();

    let checked =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 2)
            .expect("check_refinement_with_budget runs");

    assert!(
        checked.impl_violation.is_none(),
        "the pre-pass must stop at the budget, not search on to the violation: {:?}",
        checked.impl_violation
    );
    assert!(checked.failure.is_none(), "{:?}", checked.failure);
    assert_eq!(
        checked.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 2 }
    );
}

/// A self-violation the pre-pass reaches before the budget is still decided,
/// at the exact boundary: `n = 4` is generated from the fourth visited state
/// (`n = 3`), so a budget of 5 leaves room to reach it and a budget of 4
/// stops right after inserting `n = 3`.
#[test]
fn the_budget_boundary_separates_a_decided_violation_from_a_cutoff() {
    let (implementation, abstraction, mapping) = late_self_violation_fixture();

    let decided =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 5)
            .expect("check_refinement_with_budget runs");
    assert!(matches!(
        decided.verdict(),
        RefinementVerdict::ImplViolated { violation, .. } if violation.kind == "type_bound"
    ));

    let cut =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 4)
            .expect("check_refinement_with_budget runs");
    assert_eq!(
        cut.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 4 }
    );
}

/// A budget of 1 is reached by the initial state alone: the pre-pass stops
/// at its root-level check with exactly one state explored, before expanding
/// the root. Without that check it would insert a child first and report 2.
#[test]
fn a_budget_reached_by_the_initial_states_stops_before_expanding_them() {
    let (implementation, abstraction, mapping) = late_self_violation_fixture();

    let checked =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 1)
            .expect("check_refinement_with_budget runs");

    assert_eq!(
        checked.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 1 }
    );
}

/// A self-consistent impl (four in-bound states) whose very first step is
/// rejected by the abstraction: `LateAbs`'s `bump` requires `n >= 1`, and
/// both start at 0, so the correspondence walk fails at step 1.
fn early_mismatch_fixture() -> (
    fsl_core::KernelModel,
    fsl_core::KernelModel,
    fsl_core::Refinement,
) {
    let implementation = model(
        "spec EarlyImpl { type IQty = 0..3 state { n: IQty } init { n = 0 } \
         action bump() { requires n < 3  n = n + 1 } }",
    );
    let abstraction = model(
        "spec EarlyAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } \
         action bump() { requires n >= 1  requires n < 3  n = n + 1 } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl EarlyImpl abs EarlyAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    (implementation, abstraction, mapping)
}

/// The order this issue fixes: a pre-pass cutoff decides the run, and the
/// correspondence walk does not run after it -- even when the walk would
/// have found a mismatch at step 1, inside its own budget. Calibrated first:
/// with a budget covering the reachable set the step-1 mismatch is found.
/// Running the walk after a pre-pass cutoff is a separate, deferred option;
/// changing to it must change this test on purpose.
#[test]
fn a_pre_pass_cutoff_is_reported_even_when_the_walk_would_fail_at_step_one() {
    let (implementation, abstraction, mapping) = early_mismatch_fixture();

    let full = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        6,
        1_000,
    )
    .expect("check_refinement_with_budget runs");
    match full.verdict() {
        RefinementVerdict::Failed(failure) => {
            assert_eq!(failure.kind, "abs_requires_failed");
            assert_eq!(failure.step, 1);
        }
        other => panic!("expected the step-1 mismatch, got {other:?}"),
    }

    let cut =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 2)
            .expect("check_refinement_with_budget runs");
    assert_eq!(
        cut.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 2 }
    );
}
