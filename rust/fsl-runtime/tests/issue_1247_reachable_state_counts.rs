// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1247 makes the concrete searches cheaper per state (by-reference
//! place reads, fewer `State` copies per step, enumerated-candidate steps, a
//! deduplicated walk queue) without changing what they explore. A cheaper
//! search that silently explores *fewer* states would still answer
//! `refines`, so the reachable-state counts are pinned here on the issue's
//! own repro, `examples/e2e` (`3_design` refining `2_requirements`, as
//! committed: `instances Claim = 3`, `values Amount = 0..3`), at depth 6.
//!
//! 1,818 is the implementation's reachable set within depth 6, measured
//! before the change by `fslc verify --engine explicit` (and by the #1247
//! investigation's instrumented pre-pass). The refinement pre-pass is pinned
//! through its budget: a cutoff at exactly 1,818 states trips in the pre-pass,
//! one state more completes (pre-pass and walk alike). The walk shares the
//! budget, so this pins it only from above; the negative controls and the
//! refine corpus outputs cover it otherwise.

use std::path::PathBuf;

use fsl_core::{FsResolver, KernelModel, Refinement, build_model, parse_kernel_source};
use fsl_runtime::RefinementVerdict;

const DEPTH: usize = 6;
const IMPL_REACHABLE: usize = 1_818;

fn e2e(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/e2e")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn model(name: &str) -> KernelModel {
    build_model(parse_kernel_source(&e2e(name), &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

fn refinement() -> (KernelModel, KernelModel, Refinement) {
    let implementation = model("3_design.fsl");
    let abstraction = model("2_requirements.fsl");
    let mapping =
        fsl_core::parse_refinement(&e2e("3_refines_2.fsl"), &implementation, &abstraction)
            .expect("parse mapping");
    (implementation, abstraction, mapping)
}

#[test]
fn bfs_and_explicit_reach_the_same_number_of_states() {
    let implementation = model("3_design.fsl");
    let bfs = fsl_runtime::bfs(implementation.clone(), DEPTH).expect("bfs runs");
    assert_eq!(bfs.violation, None);
    assert_eq!(bfs.states_explored, IMPL_REACHABLE);
    let explicit =
        fsl_runtime::verify_explicit(implementation, DEPTH, 1_000_000).expect("explicit runs");
    assert!(explicit.violation.is_none() && !explicit.budget_exceeded);
    assert_eq!(explicit.states_explored, IMPL_REACHABLE);
}

#[test]
fn the_refinement_pre_pass_visits_exactly_the_reachable_set() {
    let (implementation, abstraction, mapping) = refinement();
    let run = |budget| {
        fsl_runtime::check_refinement_with_budget(
            &implementation,
            &abstraction,
            &mapping,
            DEPTH,
            budget,
        )
        .expect("check_refinement_with_budget runs")
    };
    let cut = run(IMPL_REACHABLE);
    assert_eq!(
        cut.verdict(),
        RefinementVerdict::BudgetExhausted {
            states_explored: IMPL_REACHABLE
        }
    );
    // The pre-pass's cutoff, not the walk's: the pre-pass returns before
    // the walk builds the action map. A pre-pass that reached fewer states
    // would complete and leave the cutoff to the walk.
    assert!(cut.action_map().is_empty(), "{:?}", cut.action_map());
    assert_eq!(
        run(IMPL_REACHABLE + 1).verdict(),
        RefinementVerdict::Refines
    );
}
