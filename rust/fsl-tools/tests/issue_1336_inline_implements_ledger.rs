// SPDX-License-Identifier: Apache-2.0

//! #1336 renderer contract, including envelopes with both refinement axes.

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

fn render(verification: &Value) -> String {
    let kernel = parse_kernel_source(
        r#"spec LedgerOrder {
            state { ready: Bool }
            init { ready = false }
            @undecided("review rollback")
            action publish() { ready = true }
        }"#,
        &FsResolver::new("."),
    )
    .expect("parse renderer fixture");
    let model = build_model(kernel).expect("build renderer fixture");
    fsl_tools::render_ledger("order.fsl", &model, verification, &json!({}), None, &[])
}

fn envelope() -> Value {
    json!({"result":"verified", "checked_to_depth":3, "completeness":"bounded"})
}

fn governance_envelope() -> Value {
    let mut verification = envelope();
    verification["governance"] = json!({
        "name":"G", "preservations":[{
            "name":"Kept", "before":"A", "after":"I", "preserve":["CTRL-1"], "result":"refines"
        }]
    });
    verification
}

fn assert_inline_section(verdict: &str, seam_result: &str) {
    let mut verification = governance_envelope();
    verification["result"] = json!(verdict);
    verification["implements"] = json!({"abs":"UpperContract", "result":seam_result});
    let body = render(&verification);
    let inline = body
        .find("\n## inline implements\n")
        .unwrap_or_else(|| panic!("missing inline implements section: {body}"));
    let governance = body
        .find("\n## governance preservation\n")
        .expect("existing governance section");
    let undecided = body.find("\n## 未決定一覧\n").expect("undecided section");
    assert!(inline < governance && governance < undecided, "{body}");
    let section = &body[inline..governance];
    assert!(
        section.contains(&format!("- 検証判定: `{verdict}`\n")),
        "{section}"
    );
    assert!(section.contains("`UpperContract`"), "{section}");
    assert!(section.contains(&format!("`{seam_result}`")), "{section}");
    assert!(
        body.contains("| `Kept` | `A` → `I` | `CTRL-1` | 🟢 `refines` |\n"),
        "{body}"
    );
    // G2, representative decision 2026-10-26: failed seams also add findings outside this section.
    if seam_result == "refines" {
        let mut without_inline = body.clone();
        without_inline.replace_range(inline..governance, "");
        verification
            .as_object_mut()
            .expect("envelope")
            .remove("implements");
        assert_eq!(without_inline, render(&verification));
    }
}

#[test]
fn failing_inline_section_displays_verdict_abs_and_result_before_governance_and_undecided() {
    assert_inline_section("refinement_failed", "refinement_failed");
}

#[test]
fn passing_inline_section_displays_verdict_abs_and_result_before_governance_and_undecided() {
    assert_inline_section("verified", "refines");
}

#[test]
fn ledger_without_implements_keeps_governance_and_undecided_sections() {
    let verification = governance_envelope();
    let body = render(&verification);
    assert!(!body.contains("## inline implements"), "{body}");
    assert!(body.contains("- 検証判定: `verified`\n"), "{body}");
    assert!(
        body.contains("| `Kept` | `A` → `I` | `CTRL-1` | 🟢 `refines` |\n"),
        "{body}"
    );
    assert!(body.contains("## 未決定一覧\n"), "{body}");
    assert!(body.contains("review rollback"), "{body}");
    // Byte-for-byte noninterference is the section-subtraction relation in
    // the detector above, not a comparison of an input with its own clone.
}
