// SPDX-License-Identifier: Apache-2.0

#[path = "../../fslc/tests/support/ledger_attribution.rs"]
mod cases;
#[path = "../../fslc/tests/support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

fn assert_owner(mut value: Value, id: &str, name: &str) {
    let source = r#"spec Attribution {
        state { x: Int }
        init { x = 0 }
        @requirement("REQ-ACTION", "action obligation")
        action step() { x = x + 1 }
        @requirement("REQ-INV", "invariant obligation")
        invariant Safe { x >= 0 }
        @requirement("REQ-TRANS", "transition obligation")
        trans Stable { x >= old(x) }
        @requirement("REQ-LIVE", "progress obligation")
        leadsTo Done { x < 5 ~> x == 5 }
    }"#;
    let model = build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap();
    value["checked_to_depth"] = json!(3);
    value["completeness"] = json!("bounded");
    let body = fsl_tools::render_ledger("synthetic.fsl", &model, &value, &json!({}), None, &[]);
    let detail = findings::assert_issue(&body, id, &[name]);
    if value["result"] == "unknown_cti" {
        findings::assert_uncertain(detail);
    }
    for other in ["REQ-ACTION", "REQ-INV", "REQ-TRANS", "REQ-LIVE"] {
        if other != id {
            assert!(findings::risk_row(&body, other).contains("🟢"), "{body}");
        }
    }
    assert!(
        !findings::section(&body, "リスク一覧（要件ID別）").contains("| （仕様全体） |"),
        "{body}"
    );
}

fn action_cti(kind: &str, name: &str) {
    assert_owner(cases::cti(kind, name, Some("step")), "REQ-ACTION", name);
}

#[test]
fn partial_op_cti_uses_action_requirement() {
    action_cti("partial_op", "_partial_step");
}
#[test]
fn key_domain_cti_uses_action_requirement() {
    action_cti("key_domain", "_key_domain_step");
}
#[test]
fn ensures_cti_uses_action_requirement() {
    action_cti("ensures", "step");
}

fn action_violation(kind: &str, name: &str) {
    assert_owner(
        json!({"result":"violated", "violation_kind":kind, "invariant":name,
        "last_action":{"name":"step"}, "trace_type":kind, "trace":[], "violated_at_step":1}),
        "REQ-ACTION",
        name,
    );
}

#[test]
fn partial_op_violation_uses_action_requirement() {
    action_violation("partial_op", "_partial_step");
}
#[test]
fn ensures_violation_uses_action_requirement() {
    action_violation("ensures", "step");
}
#[test]
fn boundary_type_bound_uses_action_requirement() {
    action_violation("type_bound", "_bounds_x");
}

#[test]
fn bmc_transition_with_direct_metadata_keeps_its_requirement() {
    assert_owner(
        json!({"result":"violated", "violation_kind":"trans", "invariant":"Stable", "trans":"Stable",
        "requirement":{"id":"REQ-TRANS", "text":"transition obligation"},
        "last_action":{"name":"step"}, "trace_type":"trans", "trace":[]}),
        "REQ-TRANS",
        "Stable",
    );
}

#[test]
fn leadsto_violation_uses_leadsto_requirement() {
    assert_owner(
        json!({"result":"violated", "violation_kind":"leadsTo", "invariant":"Done",
        "trace_type":"leadsTo", "trace":[]}),
        "REQ-LIVE",
        "Done",
    );
}

#[test]
fn property_partial_cti_uses_property_requirement() {
    assert_owner(
        cases::cti("partial_op", "_partial_property_Safe", None),
        "REQ-INV",
        "_partial_property_Safe",
    );
}

#[test]
fn property_key_domain_cti_uses_property_requirement() {
    assert_owner(
        cases::cti("key_domain", "_key_domain_property_Safe", None),
        "REQ-INV",
        "_key_domain_property_Safe",
    );
}
