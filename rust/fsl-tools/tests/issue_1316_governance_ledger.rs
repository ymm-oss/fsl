// SPDX-License-Identifier: Apache-2.0

//! Issue #1316: since #1303 a failed governance `preservation` folds into the
//! verification verdict, so `fslc ledger` exits 1, but the Markdown body
//! showed neither that verdict nor any preservation result. A reader of the
//! ledger alone saw no reason for the failure. The body now renders the
//! top-level verdict and every preservation with its result, failing ones
//! marked, whenever the verification envelope carries a `governance` section,
//! and renders nothing new when it does not.

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

const SPEC: &str = "spec G { state { ok: Bool } init { ok = true } invariant Ok { ok } }";

fn model() -> fsl_core::KernelModel {
    build_model(parse_kernel_source(SPEC, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

fn render(verification: &Value) -> String {
    fsl_tools::render_ledger(
        "governance.fsl",
        &model(),
        verification,
        &json!({}),
        None,
        &[],
    )
}

fn preservation(name: &str, after: &str, result: &str) -> Value {
    json!({
        "name": name,
        "before": "A",
        "after": after,
        "preserve": ["CTRL-1", "CTRL-2"],
        "result": result,
    })
}

fn verification(result: &str, preservations: &[Value]) -> Value {
    json!({
        "result": result,
        "completeness": "bounded",
        "checked_to_depth": 8,
        "governance": {
            "name": "G",
            "controls": ["CTRL-1", "CTRL-2"],
            "delegates": [],
            "preservations": preservations,
        },
    })
}

#[test]
fn a_failed_preservation_and_the_folded_verdict_are_in_the_body() {
    let rendered = render(&verification(
        "refinement_failed",
        &[
            preservation("Kept", "I1", "refines"),
            preservation("Lost", "I2", "refinement_failed"),
        ],
    ));
    assert!(
        rendered.contains("- 検証判定: `refinement_failed`\n"),
        "{rendered}"
    );
    // Every preservation is listed, not only the failing one, in declaration
    // order, and only the failing one is marked as failing.
    let kept = rendered
        .find("| `Kept` | `A` → `I1` | `CTRL-1`, `CTRL-2` | 🟢 `refines` |\n")
        .unwrap_or_else(|| panic!("passing preservation row missing:\n{rendered}"));
    let lost = rendered
        .find("| `Lost` | `A` → `I2` | `CTRL-1`, `CTRL-2` | 🔴 `refinement_failed` |\n")
        .unwrap_or_else(|| panic!("failing preservation row missing:\n{rendered}"));
    assert!(kept < lost, "{rendered}");
    assert_eq!(rendered.matches("| 🔴 `").count(), 1, "{rendered}");
}

#[test]
fn every_non_refines_preservation_result_is_marked_failing() {
    for result in ["impl_violated", "unknown_budget", "not_a_verdict"] {
        let rendered = render(&verification(
            "impl_violated",
            &[preservation("P", "I", result)],
        ));
        assert!(
            rendered.contains(&format!("| 🔴 `{result}` |\n")),
            "{result}: {rendered}"
        );
    }
}

#[test]
fn a_passing_governance_ledger_shows_its_verdict_and_preservations_too() {
    let rendered = render(&verification(
        "verified",
        &[preservation("Kept", "I1", "refines")],
    ));
    assert!(rendered.contains("- 検証判定: `verified`\n"), "{rendered}");
    assert!(
        rendered.contains("| `Kept` | `A` → `I1` | `CTRL-1`, `CTRL-2` | 🟢 `refines` |\n"),
        "{rendered}"
    );
    assert!(!rendered.contains("🔴 `"), "{rendered}");
}

/// A ledger whose verification carries no `governance` section (every
/// non-governance document) gains nothing: its bytes stay the ones the
/// approval digests and the frozen Python ledger were taken over.
#[test]
fn a_ledger_without_a_governance_section_is_unchanged() {
    let mut with_section = verification("verified", &[preservation("Kept", "I1", "refines")]);
    let rendered_with = render(&with_section);
    with_section
        .as_object_mut()
        .expect("verification object")
        .remove("governance");
    let rendered_without = render(&with_section);
    assert!(!rendered_without.contains("検証判定"), "{rendered_without}");
    assert!(
        !rendered_without.contains("preservation"),
        "{rendered_without}"
    );
    // The section is purely additive: removing it from the governance ledger
    // gives back the ledger without one.
    let start = rendered_with
        .find("\n## governance preservation")
        .expect("governance section");
    let end = rendered_with[start + 1..]
        .find("\n## ")
        .map_or(rendered_with.len(), |offset| start + 1 + offset);
    let mut stripped = rendered_with.clone();
    stripped.replace_range(start..end, "");
    assert_eq!(stripped, rendered_without);
}
