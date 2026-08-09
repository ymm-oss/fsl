// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #728 keeps bounded `action_coverage` and the selectable
//! `never_enabled_action` warning as two projections. Neither warning kind
//! nor its presence may change the ledger assurance class.

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::json;

const SPEC: &str = "spec LedgerNeverEnabled { state { x: Bool } init { x = false } \
     @requirement(\"REQ-LEDGER-BLOCKED\", \"blocked action is attributable\") \
     action blocked() { requires x x = false } invariant Trivial { true } }";

fn model() -> fsl_core::KernelModel {
    build_model(parse_kernel_source(SPEC, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

fn render(with_warning: bool) -> String {
    let warnings = with_warning.then(|| {
        json!([{
            "kind": "never_enabled_action",
            "name": "blocked",
            "loc": {"line": 1, "column": 1},
            "requirement": {"id": "REQ-LEDGER-BLOCKED", "text": "blocked action is attributable"},
            "requirements": [{"id": "REQ-LEDGER-BLOCKED", "text": "blocked action is attributable"}],
            "message": "action 'blocked' is never enabled within depth 2",
            "hint": "fixture hint",
        }])
    });
    let verification = json!({
        "result": "verified",
        "completeness": "bounded",
        "checked_to_depth": 2,
        "action_coverage": {
            "blocked": {
                "covered": false,
                "hint": "coverage hint",
                "requirement": {"id": "REQ-LEDGER-BLOCKED", "text": "blocked action is attributable"},
                "requirements": [{"id": "REQ-LEDGER-BLOCKED", "text": "blocked action is attributable"}],
            },
        },
        "warnings": warnings.unwrap_or_else(|| json!([])),
    });
    fsl_tools::render_ledger(
        "ledger_never_enabled.fsl",
        &model(),
        &verification,
        &json!({}),
        None,
        &[],
    )
}

#[test]
fn never_enabled_warning_and_action_coverage_remain_distinct_ledger_projections() {
    let rendered = render(true);
    assert!(
        rendered.contains("深さ内で一度も実行可能にならない（死アクション）"),
        "{rendered}"
    );
    assert!(
        rendered.contains("空虚性の疑い（never_enabled_action）"),
        "typed warning projection missing: {rendered}"
    );
    assert!(
        rendered.contains("REQ-LEDGER-BLOCKED"),
        "the warning must stay attributed to the blocked action's requirement: {rendered}"
    );
    assert!(
        rendered.contains("\"name\": \"blocked\"") && rendered.contains("\"loc\":"),
        "the ledger must preserve the warning's action name and location in raw evidence: {rendered}"
    );
}

#[test]
fn never_enabled_warning_does_not_promote_assurance() {
    let with_warning = render(true);
    let without_warning = render(false);
    let assurance_cell = |rendered: &str| {
        rendered
            .lines()
            .find(|line| line.contains("| REQ-LEDGER-BLOCKED |"))
            .unwrap_or_else(|| panic!("expected blocked-action row: {rendered}"))
            .split('|')
            .nth(4)
            .unwrap_or_else(|| panic!("expected assurance cell: {rendered}"))
            .trim()
            .to_owned()
    };
    assert_eq!(
        assurance_cell(&with_warning),
        assurance_cell(&without_warning),
        "the warning may add a vacuity trace type, but must not promote assurance"
    );
}
