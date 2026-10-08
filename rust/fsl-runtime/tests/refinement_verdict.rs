// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `RefinementCheck::verdict` is the one reading of a refinement check's
//! three outcome fields. Before it existed only inline `implements` read
//! `budget_exhausted`; `fslc refine`, `fslc chain` and governance read
//! `impl_violation`/`failure` alone, so a correspondence walk cut off by its
//! state budget came out as `refines` even when the cut-off part held a
//! mismatch. The CLI and Worker wiring is covered by
//! `rust/fslc/tests/refine_budget_unknown.rs` and the `fsl-wasm` unit tests.

use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};
use fsl_runtime::{RefinementCheck, RefinementVerdict};

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

/// `reset` is mapped to `stutter` but moves the abstract counter back to 0:
/// a `stutter_changed_abs` mismatch first reachable at step 4 (three bumps,
/// then reset). The miniature of the investigation's `fg8` variant.
fn stutter_mismatch_fixture() -> (
    fsl_core::KernelModel,
    fsl_core::KernelModel,
    fsl_core::Refinement,
) {
    let implementation = model(
        "spec ResetImpl { type Qty = 0..3 state { n: Qty } init { n = 0 } \
         action bump() { requires n < 3  n = n + 1 } \
         action reset() { requires n == 3  n = 0 } }",
    );
    let abstraction = model(
        "spec ResetAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } \
         action bump() { requires n < 3  n = n + 1 } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl ResetImpl abs ResetAbs map n = n \
         action bump() -> bump() action reset() -> stutter }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    (implementation, abstraction, mapping)
}

/// Calibration: with a budget that covers the reachable set, the mismatch is
/// found.
#[test]
fn a_budget_covering_the_reachable_set_finds_the_stutter_mismatch() {
    let (implementation, abstraction, mapping) = stutter_mismatch_fixture();

    let checked = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        6,
        1_000,
    )
    .expect("check_refinement_with_budget runs");

    match checked.verdict() {
        RefinementVerdict::Failed(failure) => assert_eq!(failure.kind, "stutter_changed_abs"),
        other => panic!("expected the stutter mismatch, got {other:?}"),
    }
}

/// The same fixture with a budget that cuts the walk off before step 4: the
/// mismatch is in the unvisited part, so the verdict is the cutoff -- never
/// `Refines`, which is what every consumer except inline `implements` used to
/// report here.
#[test]
fn a_walk_cut_off_before_the_mismatch_is_a_budget_verdict_not_refines() {
    let (implementation, abstraction, mapping) = stutter_mismatch_fixture();

    let checked =
        fsl_runtime::check_refinement_with_budget(&implementation, &abstraction, &mapping, 6, 2)
            .expect("check_refinement_with_budget runs");

    assert!(checked.failure.is_none(), "{:?}", checked.failure);
    assert!(checked.impl_violation.is_none());
    assert_eq!(
        checked.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 2 }
    );
}

fn bare_check() -> RefinementCheck {
    let (implementation, abstraction, mapping) = stutter_mismatch_fixture();
    let found = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        6,
        1_000,
    )
    .expect("check_refinement_with_budget runs");
    RefinementCheck {
        failure: None,
        impl_violation: None,
        budget_exhausted: None,
        ..found
    }
}

/// No outcome field set reads as `Refines`.
#[test]
fn no_outcome_field_reads_as_refines() {
    assert_eq!(bare_check().verdict(), RefinementVerdict::Refines);
}

/// The order the verdict fixes if more than one field were ever set: the
/// undecided cutoff first, so a decided verdict is never read off an
/// incomplete walk; then the impl's own violation; then the mismatch.
#[test]
fn the_cutoff_wins_over_every_decided_field() {
    let (implementation, abstraction, mapping) = stutter_mismatch_fixture();
    let failed = fsl_runtime::check_refinement_with_budget(
        &implementation,
        &abstraction,
        &mapping,
        6,
        1_000,
    )
    .expect("check_refinement_with_budget runs");
    let failure = failed.failure.clone().expect("calibrated mismatch");
    let self_violating = model(
        "spec SelfViolating { type IQty = 0..1 state { n: IQty } init { n = 1 } \
         action dec() { n = n - 1 } }",
    );
    let self_abs = model(
        "spec SelfAbs { type AQty = 0..1 state { n: AQty } init { n = 1 } \
         action dec() { requires n > 0  n = n - 1 } }",
    );
    let self_mapping = parse_refinement(
        "refinement S { impl SelfViolating abs SelfAbs maps auto }",
        &self_violating,
        &self_abs,
    )
    .expect("parse mapping");
    let impl_violation =
        fsl_runtime::check_refinement(&self_violating, &self_abs, &self_mapping, 4)
            .expect("check_refinement runs")
            .impl_violation
            .expect("calibrated self-violation");

    let all = RefinementCheck {
        failure: Some(failure.clone()),
        impl_violation: Some(impl_violation.clone()),
        budget_exhausted: Some(7),
        ..bare_check()
    };
    assert_eq!(
        all.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 7 }
    );

    let decided = RefinementCheck {
        budget_exhausted: None,
        ..all.clone()
    };
    assert!(matches!(
        decided.verdict(),
        RefinementVerdict::ImplViolated { violation, .. } if violation.kind == "type_bound"
    ));

    let mismatch_only = RefinementCheck {
        impl_violation: None,
        ..decided
    };
    assert_eq!(mismatch_only.verdict(), RefinementVerdict::Failed(&failure));
}
