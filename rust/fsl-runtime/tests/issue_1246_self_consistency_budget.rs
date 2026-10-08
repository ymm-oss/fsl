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

/// A self-violation the pre-pass reaches before the budget is still decided:
/// the cutoff applies only to what the pre-pass did not visit. `n = 4` is
/// the fifth state, so a budget of 6 leaves room to reach it.
#[test]
fn a_self_violation_inside_the_budget_is_still_reported() {
    let (implementation, abstraction, mapping) = late_self_violation_fixture();

    let checked =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 6)
            .expect("check_refinement_with_budget runs");

    assert!(
        checked.budget_exhausted.is_none(),
        "{:?}",
        checked.budget_exhausted
    );
    assert!(matches!(
        checked.verdict(),
        RefinementVerdict::ImplViolated { violation, .. } if violation.kind == "type_bound"
    ));
}
