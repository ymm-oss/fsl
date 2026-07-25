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
    let warnings =
        fsl_runtime::verification_warnings(&unreachable, 3, false, None, None, &BTreeMap::new());
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
    let warnings =
        fsl_runtime::verification_warnings(&reachable, 3, false, None, None, &BTreeMap::new());
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.get("kind").and_then(|k| k.as_str()) == Some("vacuous_leadsto")),
        "reachable trigger must not be flagged: {warnings:#?}"
    );
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
