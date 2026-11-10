// SPDX-License-Identifier: Apache-2.0

#[path = "../../fslc/tests/support/ledger_name_cases.rs"]
mod cases;
#[path = "../../fslc/tests/support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;

use fsl_core::{CoreError, FileResolver, FsResolver, build_model, parse_kernel_source};
use serde_json::json;

struct Component(String);
impl FileResolver for Component {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        assert_eq!(path, "component.fsl");
        Ok(self.0.clone())
    }
}

fn check_renderer_cell(case: &cases::Case, composed: bool, annotated: bool) {
    let annotation = case.annotation(annotated);
    // Domain always emits a legacy requirement (domain_lowering.rs::lower_properties).
    // An unlinked domain property cannot be authored through that CLI; use an
    // explicit internal-name model for the renderer's unlinked control.
    let source = if case.implicit_requirement {
        format!(
            "spec Subject {{ state {{ x: Int }} init {{ x = 0 }} action step() {{ x = x + 1 }} {annotation} invariant Counter_Safe {{ x >= 0 }} }}"
        )
    } else {
        case.source.replace("OWNER", annotation)
    };
    let (body, name) = render_cell(case, composed, annotated, source);
    cases::assert_ledger(&body, case, &name, annotated);
}

fn render_cell(
    case: &cases::Case,
    composed: bool,
    annotated: bool,
    source: String,
) -> (String, String) {
    let kernel = if composed {
        parse_kernel_source(
            "compose Composed { use Subject as ab from \"component.fsl\" }",
            &Component(source),
        )
        .unwrap()
    } else {
        parse_kernel_source(&source, &FsResolver::new(".")).unwrap()
    };
    let model = build_model(kernel).unwrap();
    let name = cases::public_name(case.name, composed);
    // Attribution-key projections, not full fabricated verify envelopes:
    // verification.rs::render_induction_cti has no property last_action/direct requirement;
    // origin-aware names come from origin_aware_property_name. Direct requirement
    // is retained for linked violations (verification_output.rs::render_violation).
    let mut value = json!({"result":case.result, "invariant":name,
        "checked_to_depth":3, "completeness":"bounded"});
    if case.implicit_requirement {
        let lowered = build_model(
            parse_kernel_source(&case.source.replace("OWNER", ""), &FsResolver::new(".")).unwrap(),
        )
        .unwrap();
        value["origin"] = fsl_core::internal_origin_json(
            lowered
                .property_origin("invariant", "Counter_Safe")
                .unwrap(),
        );
    }
    if let Some(kind) = case.kind {
        value["violation_kind"] = json!(kind);
    }
    if let Some(generated) = case.generated {
        value["generated_name"] = json!(generated);
    }
    if case.result == "unknown_cti" {
        value["trace_type"] = json!("induction_cti");
        value["cti"] = json!({"states":[],"violated_at":1});
        value["hint"] = json!("strengthen the obligation");
        if case.kind == Some("ensures") {
            // verification.rs::insert_action_cti_last_action uses the public action name.
            value["last_action"] = json!({"name":name});
        }
    } else {
        value["trace"] = json!([]);
        value["trace_type"] = json!("invariant");
        value["last_action"] = json!({"name":"Step", "generated_name":"counter_step"});
        if annotated {
            value["requirement"] = json!({"id":"DOMAIN-INVARIANT","text":"Counter.Safe"});
        }
    }
    let body = fsl_tools::render_ledger("synthetic.fsl", &model, &value, &json!({}), None, &[]);
    (body, name)
}

#[test]
fn synthetic_property_group_and_generated_identity_matrix() {
    let mut red = Vec::new();
    let mut green = Vec::new();
    for case in cases::cases()
        .into_iter()
        .filter(|case| {
            case.result == "unknown_cti"
                && matches!(
                    case.label,
                    "property_partial_T"
                        | "property_partial_L"
                        | "property_key_domain_T"
                        | "property_key_domain_L"
                )
        })
        .chain(
            cases::lowered_cases()
                .into_iter()
                .filter(|case| case.implicit_requirement),
        )
    {
        for &composed in if case.implicit_requirement {
            &[false][..]
        } else {
            &[false, true][..]
        } {
            for annotated in [false, true] {
                let key = case.key(
                    if case.implicit_requirement {
                        "lowered-projection"
                    } else if composed {
                        "compose"
                    } else {
                        "plain"
                    },
                    annotated,
                );
                cases::collect_cell(&mut green, &mut red, key, || {
                    check_renderer_cell(&case, composed, annotated);
                });
            }
        }
    }
    cases::finish_matrix(&green, &red);
}

#[test]
fn competing_candidate_attribution_matrix() {
    let mut red = Vec::new();
    let mut green = Vec::new();
    for cell in cases::competing_cases() {
        cases::collect_cell(&mut green, &mut red, cell.key(), || {
            let (body, name) = render_cell(
                &cell.case,
                cell.composed,
                cell.target_id.is_some(),
                cell.source(),
            );
            cases::assert_attribution(&body, &cell.case, &name, cell.target_id);
            if let Some(id) = cell.competitor_id {
                assert!(
                    findings::risk_row(&body, id).contains("🟢"),
                    "competitor requirement must stay green: {body}"
                );
            }
        });
    }
    cases::finish_matrix(&green, &red);
}
