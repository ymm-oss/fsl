// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::BTreeMap;

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::json;

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

/// Negative control for #465: native `verification_warnings` emitted only
/// `vacuous_implication`, never `vacuous_leadsto`, so a `leadsTo` whose
/// trigger never becomes true within depth K was reported `verified` with no
/// warning at all (`--vacuity error` could not fail closed on it, because it
/// had nothing to select). If this regresses, the trigger-unreachable case
/// stops appearing in `warnings`.
#[test]
fn vacuous_leadsto_is_reported_when_the_trigger_is_unreachable() {
    let unreachable = model(
        "spec VacuousLeadsto { state { pending: Bool, done: Bool } \
         init { pending = false done = false } \
         action finish() { requires pending pending = false done = true } \
         leadsTo Served { pending ~> done } }",
    );
    let warnings = fsl_runtime::verification_warnings(
        &unreachable,
        3,
        false,
        None,
        None,
        &BTreeMap::new(),
        &[],
        false,
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.get("kind").and_then(|k| k.as_str()) == Some("vacuous_leadsto")),
        "expected a vacuous_leadsto warning: {warnings:#?}"
    );

    // Regression control: a leadsTo whose trigger *is* reachable must not be
    // flagged, so the lane does not over-trigger on ordinary leadsTo use.
    let reachable = model(
        "spec ReachableLeadsto { state { pending: Bool, done: Bool } \
         init { pending = false done = false } \
         action request() { requires not pending pending = true } \
         action finish() { requires pending pending = false done = true } \
         leadsTo Served { pending ~> done } }",
    );
    let warnings = fsl_runtime::verification_warnings(
        &reachable,
        3,
        false,
        None,
        None,
        &BTreeMap::new(),
        &[],
        false,
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.get("kind").and_then(|k| k.as_str()) == Some("vacuous_leadsto")),
        "reachable trigger must not be flagged: {warnings:#?}"
    );
}

/// Digit-growth model (same trick as
/// `rust/fsl-runtime/tests/boundary_probe_budget.rs`): `count = count * 10 +
/// x` is injective per level, so the distinct reachable state count grows
/// exactly `10^level` and is fully predictable without the test doing
/// meaningful work past a cutoff. `ReachesThree`'s antecedent becomes true
/// at level 1 (`count == 3`, one of the ten `x in 0..9` successors of the
/// root); `NeverNegative`'s antecedent (`count < 0`) can never become true,
/// because `step` only ever appends a non-negative digit.
fn digit_growth_vacuity_model() -> fsl_core::KernelModel {
    build_model(
        parse_kernel_source(
            "spec DigitGrowthVacuity { state { count: Int } init { count = 0 } \
             action step(x in 0..9) { count = count * 10 + x } \
             invariant ReachesThree { count == 3 => count == 3 } \
             invariant NeverNegative { count < 0 => count == 3 } }",
            &fsl_core::FsResolver::new("."),
        )
        .expect("parse model"),
    )
    .expect("build model")
}

/// Positive and negative control for issue #729's tri-state contract, in one
/// shared BFS call (`docs/DESIGN-vacuity.md`, `expression_reachability`):
/// - Negative control: a genuinely unreachable antecedent
///   (`NeverNegative`'s `count < 0`) must resolve `Exhausted`, not
///   `Unreachable`, once the shared budget is hit -- folding it into
///   `Unreachable` would be the exact fail-open #729 exists to prevent.
/// - Positive control: a step-1-reachable antecedent (`ReachesThree`'s
///   `count == 3`) must resolve `Reachable` under the very same budget --
///   the budget must not suppress an early, cheap finding.
#[test]
fn a_genuinely_unreachable_candidate_exhausts_the_shared_budget_while_a_step_one_candidate_resolves_reachable()
 {
    let model = digit_growth_vacuity_model();
    let candidates = fsl_runtime::vacuous_implication_candidates(&model);
    assert_eq!(
        candidates.len(),
        2,
        "both invariants are `forall*P => Q`-shaped: {candidates:?}"
    );
    let expressions: Vec<_> = candidates.iter().map(|(_, expr)| expr.clone()).collect();

    // Level 1 alone already reaches 10 distinct states (`count` in `0..=9`);
    // level 2 reaches 100 more -- far past a 50-state budget -- so at
    // `depth: 8` the search for `NeverNegative`'s antecedent must exhaust
    // long before it could ever exhaustively enumerate the (here: infinite,
    // since `count` is an unbounded `Int`) space.
    let results = fsl_runtime::expression_reachability(&model, &expressions, 8, 50)
        .expect("shared probe must not error");
    assert_eq!(
        results,
        vec![
            fsl_runtime::Reachability::Reachable,
            fsl_runtime::Reachability::Exhausted,
        ],
        "ReachesThree must resolve Reachable and NeverNegative must resolve Exhausted, \
         not Unreachable: {results:?}"
    );
}

/// Companion to the control above: the very same genuinely-unreachable
/// antecedent resolves `Unreachable` (not `Exhausted`) when the BFS is
/// allowed to actually complete -- `depth: 2` bounds the reachable set to
/// exactly 100 distinct `count` values (`0..=99`), comfortably inside a
/// 1,000-state budget that is never hit. This is the pre-#729 verdict,
/// unchanged: budget exhaustion, not depth-bounded closure, is the only new
/// way to reach `Exhausted`.
#[test]
fn the_same_candidate_resolves_unreachable_once_the_search_actually_completes() {
    let model = digit_growth_vacuity_model();
    let candidates = fsl_runtime::vacuous_implication_candidates(&model);
    let never_negative = candidates
        .iter()
        .find(|(index, _)| model.invariants[*index].name.contains("NeverNegative"))
        .map(|(_, expr)| expr.clone())
        .expect("NeverNegative contributes a candidate");

    let results = fsl_runtime::expression_reachability(&model, &[never_negative], 2, 1_000)
        .expect("shared probe must not error");
    assert_eq!(results, vec![fsl_runtime::Reachability::Unreachable]);
}

#[test]
fn induction_drops_only_typed_deadlock_warnings() {
    let warnings = vec![
        json!({"kind": "vacuous_implication", "message": "mentions deadlock intentionally"}),
        json!({"kind": "deadlock", "message": "deadlock reachable at step 0"}),
        json!({"message": "action is never enabled"}),
    ];

    assert_eq!(
        fsl_runtime::induction_warnings(&warnings),
        vec![warnings[0].clone(), warnings[2].clone()]
    );
}
