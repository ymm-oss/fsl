// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Two obligations from issue #729's implementation brief, exercised
//! directly at the `render_ledger` level (no real budget-exhausting run
//! needed -- `collect_findings`/`assurance_token` operate on the JSON shape
//! alone):
//!
//! 1. The ledger's summary prefix for `kind == "vacuity_probe_truncated"`
//!    must read "空虚性未確立（到達性 probe が budget で打ち切り）"
//!    (vacuity *not established*), never "空虚性の疑い" (*suspected*
//!    hollow) -- the two are different claims: a suspected finding says
//!    something was proven vacuous; a truncated probe says nothing was
//!    proven either way.
//! 2. Negative control (c): `assurance_token` (and therefore the rendered
//!    保証クラス cell) must not move when a `vacuity_probe_truncated`
//!    finding is present versus an ordinary `vacuous_implication` finding,
//!    because it is derived from `completeness`/`result` alone and never
//!    reads `warnings`.

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::json;

const SPEC: &str = "spec LedgerTruncatedProbe { state { x: Bool } init { x = false } \
     action flip() { x = not x } invariant Trivial { x => x } }";

fn model() -> fsl_core::KernelModel {
    build_model(parse_kernel_source(SPEC, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

fn verification_with(warning_kind: &str) -> serde_json::Value {
    json!({
        "result": "verified",
        "completeness": "bounded",
        "checked_to_depth": 8,
        "warnings": [{
            "kind": warning_kind,
            "name": "Trivial",
            "message": format!("invariant 'Trivial' has an implication antecedent ({warning_kind} fixture)"),
            "hint": "fixture hint",
        }],
    })
}

fn render(verification: &serde_json::Value) -> String {
    fsl_tools::render_ledger(
        "ledger_truncated_probe.fsl",
        &model(),
        verification,
        &json!({}),
        None,
        &[],
    )
}

#[test]
fn vacuity_probe_truncated_uses_the_not_established_prefix_not_the_suspected_prefix() {
    let rendered = render(&verification_with("vacuity_probe_truncated"));
    assert!(
        rendered.contains("空虚性未確立（到達性 probe が budget で打ち切り）"),
        "expected the truncated-probe summary prefix: {rendered}"
    );
    assert!(
        !rendered.contains("空虚性の疑い（vacuity_probe_truncated）"),
        "a truncated (inconclusive) probe must not be worded as a suspected-hollow finding: \
         {rendered}"
    );
}

#[test]
fn an_ordinary_vacuous_implication_still_uses_the_suspected_prefix() {
    let rendered = render(&verification_with("vacuous_implication"));
    assert!(
        rendered.contains("空虚性の疑い（vacuous_implication）"),
        "regression control: the ordinary vacuity prefix must be unchanged: {rendered}"
    );
    assert!(
        !rendered.contains("空虚性未確立"),
        "an ordinary (non-truncated) finding must not use the not-established wording: {rendered}"
    );
}

/// Negative control (c) from the implementation brief: `assurance_token`
/// reads only `completeness`/`kernel.completeness`/`result`/`evidence.kind`/
/// `guarantee_kind`/`status` -- never `warnings` -- so the rendered
/// assurance-class row for the spec-level finding must be byte-identical
/// whether the finding is an ordinary `vacuous_implication` or a
/// `vacuity_probe_truncated` truncation. `collect_findings` maps every
/// `warnings` entry to `trace_type: "vacuity"` regardless of `kind`, so the
/// whole spec-level row -- not just the assurance cell -- is expected to
/// match exactly.
#[test]
fn assurance_class_is_unmoved_by_a_truncated_probe_finding() {
    let with_vacuous = render(&verification_with("vacuous_implication"));
    let with_truncated = render(&verification_with("vacuity_probe_truncated"));

    let spec_level_row = |rendered: &str| -> String {
        rendered
            .lines()
            .find(|line| line.contains("要件ID未付与の検出"))
            .unwrap_or_else(|| panic!("expected a spec-level row: {rendered}"))
            .to_owned()
    };

    assert_eq!(
        spec_level_row(&with_vacuous),
        spec_level_row(&with_truncated),
        "the spec-level assurance row must not depend on which vacuity `kind` produced the \
         finding: with_vacuous={with_vacuous}\n---\nwith_truncated={with_truncated}"
    );
}
