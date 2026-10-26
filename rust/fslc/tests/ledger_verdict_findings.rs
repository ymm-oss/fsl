// SPDX-License-Identifier: Apache-2.0

//! G1-G4: ledger findings must retain failed or unestablished verification evidence.

#[path = "support/report_cli.rs"]
mod cli;
#[path = "support/ledger_findings.rs"]
mod findings;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const GOV_FAIL: &str = "rust/fslc/tests/fixtures/issue_1303/governance.fsl";
const GOV_PASS: &str = "rust/fslc/tests/fixtures/issue_1303/governance_refines.fsl";
const SEAM_FAIL: &str = "tests/fixtures/chain/requirements_broken_implements.fsl";
const SEAM_PASS: &str = "tests/fixtures/chain/requirements.fsl";
const MISSING: &str = "rust/fslc/tests/fixtures/governance_missing_dependency.fsl";
const MALFORMED: &str = "examples/gallery/errors/governance_malformed_dependency.fsl";

fn report(command: &str, fixture: &str, extra: &[&str]) -> (String, i32) {
    let mut args = vec!["--deadlock", "ignore"];
    args.extend_from_slice(extra);
    cli::run(command, fixture, 3, &args)
}

fn ledger(fixture: &str, extra: &[&str]) -> (String, i32) {
    report("ledger", fixture, extra)
}

#[test]
fn failed_preservation_is_a_gov_finding() {
    let (body, status) = ledger(GOV_FAIL, &[]);
    assert_eq!(status, 1, "{body}");
    findings::assert_preservation_row(&body, "P", "refinement_failed");
    let detail = findings::assert_issue(
        &body,
        "GOV",
        &["P", "refinement_failed", "A", "→", "I", "CTRL-1"],
    );
    assert!(!detail.contains("coverage"), "{detail}");
    let (verified, _) = cli::run_json("verify", GOV_FAIL, 3, &["--deadlock", "ignore"]);
    findings::assert_raw_contains(&body, &verified["governance"]["preservations"][0]);
}

#[test]
fn passing_preservation_keeps_gov_green() {
    let (body, status) = ledger(GOV_PASS, &[]);
    assert_eq!(status, 0, "{body}");
    findings::assert_preservation_row(&body, "P", "refines");
    assert!(findings::risk_row(&body, "GOV").contains("🟢"), "{body}");
    findings::assert_no_findings(&body);
}

#[test]
fn failed_inline_seam_is_spec_level_not_a_fabricated_requirement() {
    let (body, status) = ledger(SEAM_FAIL, &[]);
    assert_eq!(status, 1, "{body}");
    findings::assert_inline_row(&body, "ChainBusiness", "refinement_failed");
    findings::assert_issue(
        &body,
        "（仕様全体）",
        &["ChainBusiness", "refinement_failed"],
    );
    assert!(findings::risk_row(&body, "REQ-1").contains("🟢"), "{body}");
}

#[test]
fn passing_inline_seam_does_not_add_findings() {
    let (body, status) = ledger(SEAM_PASS, &[]);
    assert_eq!(status, 0, "{body}");
    findings::assert_inline_row(&body, "ChainBusiness", "refines");
    assert!(findings::risk_row(&body, "REQ-1").contains("🟢"), "{body}");
    findings::assert_no_findings(&body);
}

fn assert_cti(fixture: &str, property: &str, ranked: bool) {
    let (body, status) = ledger(fixture, &["--engine", "induction"]);
    assert_eq!(status, 1, "{body}");
    let detail = findings::assert_issue(&body, "（仕様全体）", &[property]);
    findings::assert_uncertain(detail);
    // Evidence may be translated in the detail or retained verbatim in the appendix.
    for key in ["cti", "hint"] {
        assert!(body.contains(key), "missing {key}: {body}");
    }
    if ranked {
        assert!(body.contains("non_decreasing_action"), "{body}");
    }
    assert!(!findings::raw_findings(&body).as_array().unwrap().is_empty());
}

#[test]
fn induction_cti_is_unestablished_not_a_counterexample() {
    assert_cti(
        "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        "Sync",
        false,
    );
}

#[test]
fn ranked_leadsto_failure_is_unestablished_not_a_counterexample() {
    assert_cti(
        "tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl",
        "ReachFive",
        true,
    );
}

#[test]
fn proved_induction_does_not_add_findings() {
    let fixture = "examples/gallery/valid/tiny_bounded_counter.fsl";
    let (verified, status) = cli::run_json(
        "verify",
        fixture,
        3,
        &["--deadlock", "ignore", "--engine", "induction"],
    );
    assert_eq!(status, 0, "{verified}");
    assert_eq!(verified["result"], "proved", "{verified}");
    let (body, status) = ledger(fixture, &["--engine", "induction"]);
    assert_eq!(status, 0, "{body}");
    findings::assert_no_findings(&body);
}

fn output_path() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent = cli::repository_root().join("rust/target/ledger-verdict-findings");
    std::fs::create_dir_all(&parent).expect("worktree-local artifact parent");
    parent.join(format!(
        "gov-row-{}-{timestamp}-{}.md",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn assert_error(command: &str, fixture: &str, to_file: bool) {
    let (mut verified, verify_status) =
        cli::run_json("verify", fixture, 3, &["--deadlock", "ignore"]);
    assert_eq!(verified["result"], "error", "{verified}");
    assert_eq!(verify_status, 2, "{verified}");
    let path = output_path();
    assert!(
        !path.exists(),
        "output must start absent: {}",
        path.display()
    );
    let extra = if to_file {
        vec!["-o", path.to_str().expect("UTF-8 output path")]
    } else {
        vec![]
    };
    let (stdout, status) = report(command, fixture, &extra);
    assert_eq!(status, verify_status, "{stdout}");
    assert!(!path.exists(), "error wrote artifact: {}", path.display());
    let actual: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected JSON error, not {command} artifact ({error}): {stdout}")
    });
    assert_eq!(actual["result"], "error", "{actual}");
    assert!(!stdout.contains("# 意図ずれ監査台帳"), "{stdout}");
    assert!(!stdout.contains("<!doctype html>"), "{stdout}");
    // Only verify's CLI dispatch adds versions; preserve every other error field.
    assert!(actual.get("versions").is_none(), "{actual}");
    let versions = verified
        .as_object_mut()
        .unwrap()
        .remove("versions")
        .unwrap();
    assert!(
        versions.is_object(),
        "live verify-only versions: {versions}"
    );
    assert_eq!(
        actual, verified,
        "complete error envelope except CLI-only versions"
    );
}

#[test]
fn missing_dependency_returns_json_error_on_stdout() {
    assert_error("ledger", MISSING, false);
}

#[test]
fn malformed_dependency_returns_json_error_on_stdout() {
    assert_error("ledger", MALFORMED, false);
}

#[test]
fn missing_dependency_does_not_write_an_artifact() {
    assert_error("ledger", MISSING, true);
}

#[test]
fn malformed_dependency_does_not_write_an_artifact() {
    assert_error("ledger", MALFORMED, true);
}

#[test]
fn html_missing_dependency_returns_json_error_on_stdout() {
    assert_error("html", MISSING, false);
}

#[test]
fn html_malformed_dependency_returns_json_error_on_stdout() {
    assert_error("html", MALFORMED, false);
}

#[test]
fn html_missing_dependency_does_not_write_an_artifact() {
    assert_error("html", MISSING, true);
}

#[test]
fn html_malformed_dependency_does_not_write_an_artifact() {
    assert_error("html", MALFORMED, true);
}

fn assert_html_artifact(fixture: &str, verdict: &str) {
    let (body, status) = report("html", fixture, &[]);
    assert_eq!(status, 1, "{body}");
    assert!(body.starts_with("<!doctype html>"), "{body}");
    let hero = body
        .split_once("<header class=\"hero\">")
        .expect("HTML hero")
        .1
        .split_once("</header>")
        .expect("hero end")
        .0;
    assert!(
        hero.contains(&format!("<span class=\"badge bad\">{verdict}</span>")),
        "{hero}"
    );
}

#[test]
fn html_primary_violation_still_generates_a_report() {
    assert_html_artifact(
        "rust/fslc/tests/fixtures/issue_502_reachable_empty.fsl",
        "violated",
    );
}

#[test]
fn html_failed_preservation_still_generates_a_report() {
    assert_html_artifact(GOV_FAIL, "refinement_failed");
}

#[test]
fn existing_invariant_violation_stays_red() {
    let (body, status) = ledger(
        "rust/fslc/tests/fixtures/issue_502_reachable_empty.fsl",
        &[],
    );
    assert_eq!(status, 1, "{body}");
    findings::assert_issue(&body, "（仕様全体）", &["NoTrivialSelfReach"]);
}
