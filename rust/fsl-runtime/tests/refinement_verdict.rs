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
use fsl_runtime::RefinementVerdict;

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

    // `BudgetExhausted` is read first and `verdict()` debug-asserts that no
    // other outcome field is set alongside it.
    assert_eq!(
        checked.verdict(),
        RefinementVerdict::BudgetExhausted { states_explored: 2 }
    );
}

// The precedence `verdict()` fixes when more than one outcome field is set
// can only be built inside the crate now that the fields are private (#1262):
// see `refinement_verdict_precedence` in `src/lib.rs`.
