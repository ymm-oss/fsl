// SPDX-License-Identifier: Apache-2.0

//! G1-G3/G5: small synthetic evidence exercises reporting, not search budgets.

#[path = "../../fslc/tests/support/ledger_findings.rs"]
mod findings;

use fsl_core::{FsResolver, KernelModel, build_model, parse_kernel_source};
use fsl_syntax::{SourcePos, Span};
use serde_json::{Value, json};

fn model(governance: bool) -> KernelModel {
    let source = if governance {
        r#"governance RenamedCatalog { control CTRL-1 "control one" control CTRL-2 "control two" }"#
    } else {
        r#"spec Evidence {
            state { ready: Bool }
            init { ready = true }
            @requirement("REQ-KEEP", "readiness")
            invariant Ready { ready }
        }"#
    };
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse fixture"))
        .expect("build fixture")
}

fn render(verification: &Value, governance: bool) -> String {
    fsl_tools::render_ledger(
        "synthetic.fsl",
        &model(governance),
        verification,
        &json!({}),
        None,
        &[],
    )
}

fn envelope(result: &str) -> Value {
    json!({"result":result,"checked_to_depth":3,"completeness":"bounded"})
}

fn preservation(name: &str, result: &str) -> Value {
    json!({"name":name,"before":"Before","after":"After","preserve":["CTRL-1","CTRL-2"],"result":result})
}

fn governance(result: &str, items: &[Value]) -> Value {
    let mut value = envelope(result);
    value["governance"] = json!({"name":"RenamedCatalog","preservations":items});
    value
}

fn assert_preservation(result: &str) {
    let failed = preservation("ChangedControl", result);
    let body = render(&governance(result, std::slice::from_ref(&failed)), true);
    let detail = findings::assert_issue(
        &body,
        "GOV",
        &[
            "ChangedControl",
            result,
            "Before",
            "→",
            "After",
            "CTRL-1",
            "CTRL-2",
        ],
    );
    assert!(!detail.contains("coverage"), "{detail}");
    findings::assert_raw_contains(&body, &failed);
    assert_eq!(findings::raw_findings(&body).as_array().unwrap().len(), 1);
}

#[test]
fn preservation_unknown_budget_is_a_gov_finding() {
    assert_preservation("unknown_budget");
}

#[test]
fn preservation_impl_violated_is_a_gov_finding() {
    assert_preservation("impl_violated");
}

#[test]
fn only_failed_preservations_become_findings() {
    let items = [
        preservation("KeptFirst", "refines"),
        preservation("FailedOne", "refinement_failed"),
        preservation("KeptLast", "refines"),
        preservation("FailedTwo", "unknown_budget"),
    ];
    let body = render(&governance("refinement_failed", &items), true);
    let detail = findings::assert_issue(
        &body,
        "GOV",
        &[
            "FailedOne",
            "FailedTwo",
            "refinement_failed",
            "unknown_budget",
        ],
    );
    for name in ["KeptFirst", "KeptLast"] {
        assert!(
            !detail.contains(name),
            "passing preservation is not a finding: {detail}"
        );
        assert!(
            !findings::raw_findings(&body).to_string().contains(name),
            "{body}"
        );
        findings::assert_preservation_row(&body, name, "refines");
    }
    let raw = findings::raw_findings(&body);
    assert_eq!(
        raw.as_array().unwrap().len(),
        2,
        "one raw per failed declaration: {raw}"
    );
    for item in [&items[1], &items[3]] {
        findings::assert_raw_contains(&body, item);
    }
}

#[test]
fn all_refining_preservations_add_no_findings() {
    let body = render(
        &governance("verified", &[preservation("Kept", "refines")]),
        true,
    );
    assert!(findings::risk_row(&body, "GOV").contains("🟢"), "{body}");
    findings::assert_no_findings(&body);
}

#[test]
fn zero_preservations_add_no_findings() {
    let body = render(&governance("verified", &[]), true);
    assert!(findings::risk_row(&body, "GOV").contains("🟢"), "{body}");
    findings::assert_no_findings(&body);
}

fn assert_implements(result: &str) {
    let mut value = envelope(result);
    let seam = json!({"abs":"UpperContract","result":result});
    value["implements"] = seam.clone();
    let body = render(&value, false);
    findings::assert_issue(&body, "（仕様全体）", &["UpperContract", result]);
    assert!(
        findings::risk_row(&body, "REQ-KEEP").contains("🟢"),
        "{body}"
    );
    findings::assert_raw_contains(&body, &seam);
}

#[test]
fn implements_unknown_budget_is_spec_level() {
    assert_implements("unknown_budget");
}

#[test]
fn implements_impl_violated_is_spec_level() {
    assert_implements("impl_violated");
}

#[test]
fn passing_implements_adds_no_findings() {
    let mut value = envelope("verified");
    value["implements"] = json!({"abs":"UpperContract","result":"refines"});
    let body = render(&value, false);
    assert!(
        findings::risk_row(&body, "REQ-KEEP").contains("🟢"),
        "{body}"
    );
    findings::assert_no_findings(&body);
}

#[test]
fn cti_uses_requirement_metadata_and_keeps_raw_evidence() {
    let mut value = envelope("unknown_cti");
    value["invariant"] = json!("Ready");
    value["trace_type"] = json!("induction_cti");
    value["cti"] = json!({"states":[{"ready":false}],"violated_at":1});
    value["hint"] = json!("add an auxiliary invariant");
    let body = render(&value, false);
    let detail = findings::assert_issue(&body, "REQ-KEEP", &["Ready"]);
    findings::assert_uncertain(detail);
    findings::assert_raw_contains(&body, &value);
    assert!(!findings::section(&body, "リスク一覧（要件ID別）").contains("| （仕様全体） |"));
}

fn assert_inline_binding(result: &str) {
    let mut value = envelope(if result == "refines" {
        "verified"
    } else {
        result
    });
    value["implements"] = json!({"abs":"RowBoundAbs","result":result});
    // A different table contains the same result, so a section-wide search is insufficient.
    value["governance"] = json!({"name":"Other","preservations":[preservation("Decoy", result)]});
    findings::assert_inline_row(&render(&value, false), "RowBoundAbs", result);
}

#[test]
fn inline_passing_row_binds_abs_result_and_mark() {
    assert_inline_binding("refines");
}

#[test]
fn inline_failing_row_binds_abs_result_and_mark() {
    assert_inline_binding("refinement_failed");
}

#[test]
fn inline_impl_violated_row_binds_abs_result_and_mark() {
    assert_inline_binding("impl_violated");
}

#[test]
fn inline_unknown_budget_row_binds_abs_result_and_mark() {
    assert_inline_binding("unknown_budget");
}

#[derive(Clone, Copy)]
enum SourceCase {
    WrittenSentinelName,
    GeneratedOtherName,
    AuthoredOriginWithZeroSpan,
}

fn source_case(case: SourceCase) -> (KernelModel, Value, String, bool) {
    let (dialect, name, keep) = match case {
        SourceCase::WrittenSentinelName => ("spec", "_governance_noop", true),
        SourceCase::GeneratedOtherName => ("spec", "another_generated_action", false),
        SourceCase::AuthoredOriginWithZeroSpan => ("spec", "origin_backed_action", true),
    };
    let source = format!(
        "{dialect} SourceIdentity {{ state {{ x: Bool }} init {{ x = false }} \
         @requirement(\"REQ-SOURCE\", \"source identity\") \
         action {name}() {{ requires false x = true }} invariant Stable {{ true }} }}"
    );
    let mut model =
        build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
    if matches!(case, SourceCase::AuthoredOriginWithZeroSpan) {
        let annotations = model.actions[0].annotations.clone();
        let domain = r"domain SourceIdentity {
            implementation_profile functional_ddd
            aggregate Item {
                id ItemId
                state { ready: Bool = false; }
                command Blocked {}
                event Touched {}
                decide Blocked { requires ready emits Touched }
                evolve Touched { ready = true }
                invariant Stable { true }
            }
        }";
        model = build_model(parse_kernel_source(domain, &FsResolver::new(".")).unwrap()).unwrap();
        assert_eq!(model.actions.len(), 1, "one origin-backed action");
        model.actions[0].annotations = annotations;
    }
    let action = &model.actions[0];
    let origin = model
        .action_origin(&action.name)
        .and_then(|chain| chain.primary.as_ref());
    if matches!(case, SourceCase::AuthoredOriginWithZeroSpan) {
        let span = origin
            .and_then(|site| site.span)
            .expect("positive authored primary origin");
        assert!(span.start.line > 0 && span.start.column > 0);
    } else {
        assert!(
            origin.is_none(),
            "direct kernel has no authored primary origin"
        );
    }
    if matches!(case, SourceCase::WrittenSentinelName) {
        assert!(action.span.start.line > 0 && action.span.start.column > 0);
    }
    if !matches!(case, SourceCase::WrittenSentinelName) {
        let zero = SourcePos {
            offset: 0,
            line: 0,
            column: 0,
        };
        model.actions[0].span = Span {
            start: zero,
            end: zero,
        };
    }
    let display_name = model.action_display_name(&model.actions[0].name);
    if matches!(case, SourceCase::AuthoredOriginWithZeroSpan) {
        assert_ne!(display_name, "_governance_noop");
    } else {
        assert_eq!(display_name, name);
    }
    let mut verification = envelope("verified");
    verification["action_coverage"] = json!({
        &display_name: {"covered":false,"requirement":{"id":"REQ-SOURCE","text":"source identity"}}
    });
    (model, verification, display_name, keep)
}

fn assert_ledger_source_case(case: SourceCase) {
    let (model, verification, name, keep) = source_case(case);
    let body = fsl_tools::render_ledger("source.fsl", &model, &verification, &json!({}), None, &[]);
    if keep {
        findings::assert_issue(&body, "REQ-SOURCE", &[&name, "coverage"]);
        findings::assert_raw_contains(&body, &verification["action_coverage"][&name]);
    } else {
        assert!(
            findings::risk_row(&body, "REQ-SOURCE").contains("🟢"),
            "{body}"
        );
        findings::assert_no_findings(&body);
    }
}

fn assert_html_source_case(case: SourceCase) {
    let (model, verification, name, keep) = source_case(case);
    let explained = json!({"spec":"SourceIdentity","depth":3,"skeleton":{
        "actions":[{"name":name,"requires_text":["requires false"]}]
    }});
    let html =
        fsl_tools::render_html_report("source.fsl", "", &model, &explained, &verification, &[]);
    let actions = html
        .split_once("id=\"actions\"")
        .unwrap()
        .1
        .split_once("</section>")
        .unwrap()
        .0;
    let row = actions
        .split_once(&format!("<code>{name}</code>"))
        .unwrap()
        .1
        .split_once("</tr>")
        .unwrap()
        .0;
    assert_eq!(row.contains(">uncovered</span>"), keep, "{row}");
    let denominator = usize::from(keep);
    assert!(
        html.contains(&format!(
            "<span>Coverage</span><strong>0/{denominator}</strong>"
        )),
        "{html}"
    );
}

#[test]
fn ledger_keeps_authored_action_with_sentinel_name() {
    assert_ledger_source_case(SourceCase::WrittenSentinelName);
}

#[test]
fn ledger_excludes_zero_span_action_with_other_name() {
    assert_ledger_source_case(SourceCase::GeneratedOtherName);
}

#[test]
fn ledger_keeps_authored_origin_even_with_zero_action_span() {
    assert_ledger_source_case(SourceCase::AuthoredOriginWithZeroSpan);
}

#[test]
fn html_keeps_authored_action_with_sentinel_name() {
    assert_html_source_case(SourceCase::WrittenSentinelName);
}

#[test]
fn html_excludes_zero_span_action_with_other_name() {
    assert_html_source_case(SourceCase::GeneratedOtherName);
}

#[test]
fn html_keeps_authored_origin_even_with_zero_action_span() {
    assert_html_source_case(SourceCase::AuthoredOriginWithZeroSpan);
}
