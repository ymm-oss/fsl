// SPDX-License-Identifier: Apache-2.0

//! #1338: filter public findings, not the raw verification evidence. The
//! original explain/model listing is deliberately outside this exclusion.

#[path = "support/lane_b_cli.rs"]
mod cli;

use serde_json::Value;

const FIXTURES: [(&str, i32); 2] = [("governance.fsl", 1), ("governance_refines.fsl", 0)];
const AUTHORED: &str = "rust/fslc/tests/fixtures/never_enabled_depth_boundary.fsl";

fn html_section<'a>(html: &'a str, id: &str) -> &'a str {
    html.split_once(&format!("id=\"{id}\""))
        .expect("HTML section id")
        .1
        .split_once("</section>")
        .expect("HTML section end")
        .0
}

fn raw_verification(html: &str) -> Value {
    let encoded = html_section(html, "raw-data")
        .split_once("<summary>verify JSON</summary>")
        .expect("raw verify JSON")
        .1
        .split_once("<pre>")
        .expect("JSON pre block")
        .1
        .split_once("</pre>")
        .expect("JSON pre end")
        .0;
    let decoded = encoded
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&decoded).expect("raw JSON remains readable evidence")
}

fn assert_ledger_has_no_sentinel_findings(file: &str, expected_status: i32) {
    let fixture = format!("rust/fslc/tests/fixtures/issue_1303/{file}");
    let (body, status) = cli::run("ledger", &fixture, 3, &[]);
    assert_eq!(status, expected_status, "{body}");
    assert!(!body.contains("`coverage` — _governance_noop"), "{body}");
    assert!(!body.contains("アクション『_governance_noop』"), "{body}");
    let risk_table = body
        .split_once("## リスク一覧（要件ID別）")
        .expect("risk table")
        .1
        .split("\n## ")
        .next()
        .expect("risk table body");
    assert!(!risk_table.contains("| coverage |"), "{risk_table}");
}

#[test]
fn failing_preservation_ledger_has_no_sentinel_coverage_findings() {
    assert_ledger_has_no_sentinel_findings("governance.fsl", 1);
}

#[test]
fn passing_preservation_ledger_has_no_sentinel_coverage_findings() {
    assert_ledger_has_no_sentinel_findings("governance_refines.fsl", 0);
}

fn assert_html_has_no_synthetic_badge(file: &str, expected_status: i32) {
    let fixture = format!("rust/fslc/tests/fixtures/issue_1303/{file}");
    let (html, status) = cli::run("html", &fixture, 3, &[]);
    assert_eq!(status, expected_status, "{html}");
    let actions = html_section(&html, "actions");
    assert!(!actions.contains(">uncovered</span>"), "{actions}");
}

#[test]
fn failing_preservation_html_has_no_synthetic_uncovered_badge() {
    assert_html_has_no_synthetic_badge("governance.fsl", 1);
}

#[test]
fn passing_preservation_html_has_no_synthetic_uncovered_badge() {
    assert_html_has_no_synthetic_badge("governance_refines.fsl", 0);
}

fn assert_html_denominator_excludes_synthetic_actions(file: &str, expected_status: i32) {
    let fixture = format!("rust/fslc/tests/fixtures/issue_1303/{file}");
    let (html, status) = cli::run("html", &fixture, 3, &[]);
    assert_eq!(status, expected_status, "{html}");
    let metric = html
        .split_once("<span>Coverage</span><strong>")
        .expect("coverage metric")
        .1
        .split_once("</strong>")
        .expect("metric end")
        .0;
    assert_eq!(metric, "0/0", "no authored actions: {metric}");
}

#[test]
fn failing_preservation_html_denominator_excludes_synthetic_actions() {
    assert_html_denominator_excludes_synthetic_actions("governance.fsl", 1);
}

#[test]
fn passing_preservation_html_denominator_excludes_synthetic_actions() {
    assert_html_denominator_excludes_synthetic_actions("governance_refines.fsl", 0);
}

#[test]
fn governance_raw_coverage_is_retained_in_verify_and_html() {
    for (file, expected_status) in FIXTURES {
        let fixture = format!("rust/fslc/tests/fixtures/issue_1303/{file}");
        let (verified, status) = cli::run_json("verify", &fixture, 3, &[]);
        assert_eq!(status, expected_status, "{verified:#}");
        let coverage = verified["action_coverage"]
            .as_object()
            .expect("raw coverage");
        assert_eq!(coverage.len(), 1, "{coverage:#?}");
        assert_eq!(
            coverage.values().next().expect("raw action")["covered"],
            false
        );
        assert_eq!(
            coverage.values().next().expect("raw action")["requirement"]["id"],
            "GOV"
        );
        let (html, html_status) = cli::run("html", &fixture, 3, &[]);
        assert_eq!(html_status, expected_status);
        // This explicitly compares the entire coverage projection; the other
        // raw fields include independently measured cost and CLI-only versions.
        assert_eq!(
            raw_verification(&html)["action_coverage"],
            verified["action_coverage"]
        );
    }
}

#[test]
fn authored_dead_action_keeps_its_ledger_and_html_findings() {
    let (ledger, status) = cli::run("ledger", AUTHORED, 2, &[]);
    assert_eq!(status, 0, "{ledger}");
    assert!(ledger.contains("`coverage` — late"), "{ledger}");
    assert!(ledger.contains("アクション『late』"), "{ledger}");
    let (html, status) = cli::run("html", AUTHORED, 2, &[]);
    assert_eq!(status, 0, "{html}");
    let action_row = html_section(&html, "actions")
        .split_once("<code>late</code>")
        .expect("authored action row")
        .1
        .split_once("</tr>")
        .expect("action row end")
        .0;
    assert!(
        action_row.contains("<span class=\"badge bad\">uncovered</span>"),
        "{action_row}"
    );
    assert!(
        html.contains("<span>Coverage</span><strong>1/2</strong>"),
        "{html}"
    );
}

#[test]
fn authored_dead_action_keeps_its_verify_and_scenarios_warnings() {
    for command in ["verify", "scenarios"] {
        let (output, status) = cli::run_json(command, AUTHORED, 2, &[]);
        assert_eq!(status, 0, "{output:#}");
        let warnings = output["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().any(|warning| {
                warning["kind"] == "never_enabled_action" && warning["name"] == "late"
            }),
            "{command}: {warnings:#?}"
        );
    }
}
