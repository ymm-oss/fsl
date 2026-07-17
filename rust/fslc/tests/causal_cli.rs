// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! CLI contract tests for the review-only `fslc causal` command family
//! (issue #321, `docs/DESIGN-causal.md`). The load-bearing invariants:
//! deterministic JSON, `formal_result: "not_run"` everywhere, a
//! `do_not_assume` array on every success payload, and no output path that
//! attaches `proved`/`verified` to a causal claim.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn run_cli(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

const RETENTION: &str = "examples/causal/subscription_retention.fsl";
const INCIDENT: &str = "examples/causal/incident_response.fsl";
const FUNNEL: &str = "examples/causal/marketing_funnel.fsl";

fn assert_review_only(output: &Value) {
    assert_eq!(output["formal_result"], "not_run");
    assert!(
        output["do_not_assume"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty()),
        "success payloads must carry do_not_assume"
    );
    let rendered = output.to_string();
    // No output path attaches formal verdicts to causal content.
    assert!(!rendered.contains("\"proved\""));
    assert!(!rendered.contains("\"verified\""));
}

#[test]
fn causal_check_reports_counts_and_stays_review_only() {
    let (output, status) = run_cli(&["causal", "check", RETENTION]);
    assert_eq!(status, 0);
    assert_eq!(output["result"], "causal_model_checked");
    assert_eq!(output["schema_version"], "causal-check.v0");
    assert_eq!(output["model"], "SubscriptionRetention");
    assert_eq!(output["variables_checked"], 5);
    assert_eq!(output["claims_checked"], 4);
    assert_eq!(output["feedbacks_checked"], 1);
    assert_review_only(&output);
}

#[test]
fn plain_check_routes_a_causal_model_to_the_causal_checker() {
    let (output, status) = run_cli(&["check", RETENTION]);
    assert_eq!(status, 0);
    assert_eq!(output["result"], "causal_model_checked");
}

#[test]
fn causal_check_warns_on_latent_variable_without_proxy() {
    let (output, status) = run_cli(&["causal", "check", FUNNEL]);
    assert_eq!(status, 0);
    let kinds: Vec<&str> = output["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|warning| warning["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(kinds, ["causal_latent_without_proxy"]);
    let warning = &output["warnings"][0];
    assert!(warning["loc"]["line"].as_u64().expect("line") > 1);
}

#[test]
fn unknown_reference_fails_closed_with_location() {
    let source = std::fs::read_to_string(repository_root().join(RETENTION))
        .expect("read model")
        .replace("biz.enable_onboarding_support", "biz.enable_missing");
    let scratch = tempfile_dir();
    std::fs::write(scratch.join("model.fsl"), source).expect("write model");
    std::fs::copy(
        repository_root().join("examples/causal/subscription_business.fsl"),
        scratch.join("subscription_business.fsl"),
    )
    .expect("copy companion");
    let model = scratch.join("model.fsl");
    let (output, status) = run_cli(&["causal", "check", model.to_str().expect("utf-8")]);
    assert_eq!(status, 2);
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "semantics");
    assert_eq!(output["diagnostic"], "causal_unknown_reference");
    assert!(output["loc"]["line"].as_u64().expect("line") > 1);
}

fn tempfile_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-causal-cli-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch dir");
    directory
}

#[test]
fn causal_review_findings_are_never_violations() {
    for (model, expected) in [
        (RETENTION, "feedback_without_damping_story"),
        (INCIDENT, "deadline_before_earliest_effect"),
        (FUNNEL, "opposing_path_polarity"),
    ] {
        let (output, status) = run_cli(&["causal", "analyze", model, "--profile", "causal-review"]);
        assert_eq!(status, 0, "{model}");
        assert_eq!(output["schema_version"], "causal-findings.v0");
        assert_review_only(&output);
        let findings = output["findings"].as_array().expect("findings");
        assert!(
            findings
                .iter()
                .any(|finding| finding["finding_type"] == expected),
            "{model} must produce {expected}; got {findings:?}"
        );
        for finding in findings {
            assert_eq!(finding["formal_status"], "not_a_violation");
            assert!(
                !finding["do_not_assume"]
                    .as_array()
                    .expect("list")
                    .is_empty()
            );
        }
    }
}

#[test]
fn incident_model_reports_cadence_against_shortest_persistence() {
    let (output, _) = run_cli(&["causal", "analyze", INCIDENT, "--profile", "causal-review"]);
    let finding = output["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["finding_type"] == "measurement_cadence_too_coarse")
        .expect("cadence finding")
        .clone();
    assert_eq!(finding["witness"]["cadence"], 30);
    assert_eq!(finding["witness"]["persists"]["min"], 14);
}

#[test]
fn causal_graph_projection_carries_loop_class_and_truncation() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_graph",
    ]);
    assert_eq!(status, 0);
    assert_eq!(output["schema_version"], "causal-graph.v0");
    assert_review_only(&output);
    let feedback = &output["feedbacks"][0];
    assert_eq!(feedback["loop_class"], "reinforcing");
    assert_eq!(feedback["recurrent"], true);
    assert!(feedback["claims"].as_array().expect("witness claims").len() == 4);
    assert_eq!(output["truncation"]["paths_truncated"], 0);
}

#[test]
fn causal_timeline_reports_first_pass_window_and_feedback_flag() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_timeline",
    ]);
    assert_eq!(status, 0);
    let timeline = &output["timelines"][0];
    // Minkowski sum of minimum lags: 0 + 14 + 60.
    assert_eq!(timeline["first_pass"]["min"], 74);
    assert_eq!(timeline["via_feedback"], true);
}

#[test]
fn traceability_projection_bridges_requirements_and_kpis() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_traceability_graph",
    ]);
    assert_eq!(status, 0);
    let nodes: Vec<&str> = output["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|node| node["id"].as_str().expect("id"))
        .collect();
    assert!(nodes.contains(&"requirement:REQ-ONBOARDING"));
    assert!(nodes.contains(&"action:biz.enable_onboarding_support"));
    assert!(nodes.contains(&"kpi:biz.retention_90d"));
    assert!(nodes.contains(&"evidence:EXP_ONBOARDING_2026"));
}

#[test]
fn mermaid_and_dot_exports_are_deterministic() {
    let (first, status) = run_cli(&[
        "causal",
        "analyze",
        FUNNEL,
        "--projection",
        "causal_graph",
        "--format",
        "mermaid",
    ]);
    assert_eq!(status, 0);
    let (second, _) = run_cli(&[
        "causal",
        "analyze",
        FUNNEL,
        "--projection",
        "causal_graph",
        "--format",
        "mermaid",
    ]);
    assert_eq!(first["content"], second["content"]);
    assert!(
        first["content"]
            .as_str()
            .expect("text")
            .starts_with("graph LR")
    );
    let (dot, status) = run_cli(&[
        "causal",
        "analyze",
        FUNNEL,
        "--projection",
        "causal_graph",
        "--format",
        "dot",
    ]);
    assert_eq!(status, 0);
    assert!(
        dot["content"]
            .as_str()
            .expect("text")
            .starts_with("digraph causal {")
    );
}

#[test]
fn causal_diff_pins_identity_to_ids_and_support_to_not_available() {
    let scratch = tempfile_dir();
    std::fs::copy(
        repository_root().join("examples/causal/subscription_business.fsl"),
        scratch.join("subscription_business.fsl"),
    )
    .expect("copy companion");
    let before = std::fs::read_to_string(repository_root().join(RETENTION)).expect("read");
    let after = before
        .replace("lag 60..180", "lag 90..180")
        .replace(
            "claim C_Habit_Retention habit_formation -> retention_90d {\n    version 1",
            "claim C_Habit_Retention habit_formation -> retention_90d {\n    version 2",
        )
        .replace("status active\n    polarity positive\n    lag 1..30", "status retired\n    polarity positive\n    lag 1..30")
        .replace(
            "  feedback F_RetentionLoop {\n    claims C_Retention_Onboarding,\n           C_Onboarding_FirstSuccess,\n           C_FirstSuccess_Habit,\n           C_Habit_Retention\n  }\n\n",
            "",
        );
    std::fs::write(scratch.join("before.fsl"), before).expect("write before");
    std::fs::write(scratch.join("after.fsl"), after).expect("write after");
    let (output, status) = run_cli(&[
        "causal",
        "diff",
        scratch.join("before.fsl").to_str().expect("utf-8"),
        scratch.join("after.fsl").to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["schema_version"], "causal-diff.v0");
    assert_review_only(&output);
    let changes = output["changes"].as_array().expect("changes");
    let kinds: Vec<&str> = changes
        .iter()
        .map(|change| change["kind"].as_str().expect("kind"))
        .collect();
    assert!(kinds.contains(&"claim_content_changed"));
    assert!(kinds.contains(&"claim_lifecycle_changed"));
    assert!(kinds.contains(&"feedback_removed"));
    for change in changes {
        assert_eq!(change["support_transition"], "not_available");
    }
    let content = changes
        .iter()
        .find(|change| change["kind"] == "claim_content_changed")
        .expect("content change");
    assert_eq!(content["before_version"], 1);
    assert_eq!(content["after_version"], 2);
}

#[test]
fn there_is_no_causal_verify_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["causal", "verify", RETENTION])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    assert_ne!(output.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("deliberately no 'causal verify'"),
        "output: {combined}"
    );
}

#[test]
fn check_and_review_outputs_are_deterministic() {
    let (first, _) = run_cli(&["causal", "analyze", RETENTION, "--profile", "causal-review"]);
    let (second, _) = run_cli(&["causal", "analyze", RETENTION, "--profile", "causal-review"]);
    assert_eq!(first, second);
}

const EVIDENCE: &str = "examples/causal/evidence/onboarding-2026.causal.json";
const LIFECYCLE: &str = "examples/causal/evidence/onboarding-2026.lifecycle.json";

#[test]
fn evidence_graph_overlays_support_without_touching_formal_assurance() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_evidence_graph",
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
    ]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["schema_version"], "causal-evidence-graph.v0");
    assert_review_only(&output);
    let claims = output["claims"].as_array().expect("claims");
    for claim in claims {
        // Evidence never changes the formal axis (issue #322 invariant).
        assert_eq!(claim["formal_assurance"], "not_run");
    }
    let supported = claims
        .iter()
        .find(|claim| claim["id"] == "claim:C_Onboarding_FirstSuccess")
        .expect("target claim");
    assert_eq!(supported["causal_support"], "supported");
    let untested = claims
        .iter()
        .find(|claim| claim["id"] == "claim:C_Habit_Retention")
        .expect("untested claim");
    assert_eq!(untested["causal_support"], "untested");
    let edge = &output["edges"][0];
    assert_eq!(edge["applicable"], true);
    assert_eq!(edge["scope_relation"], "subsumes");
}

#[test]
fn evidence_without_lifecycle_chain_is_excluded_from_support() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_evidence_graph",
        "--evidence",
        EVIDENCE,
    ]);
    assert_eq!(status, 0);
    let claim = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|claim| claim["id"] == "claim:C_Onboarding_FirstSuccess")
        .expect("claim")
        .clone();
    assert_eq!(claim["causal_support"], "unsupported_by_current_evidence");
    assert!(
        output["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["finding_type"] == "unknown_lifecycle")
    );
}

#[test]
fn stale_evidence_requires_explicit_as_of() {
    let (output, _) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_evidence_graph",
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
        "--as-of",
        "2028-01-01",
    ]);
    let claim = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|claim| claim["id"] == "claim:C_Onboarding_FirstSuccess")
        .expect("claim")
        .clone();
    assert_eq!(claim["causal_support"], "unsupported_by_current_evidence");
    assert!(
        output["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["finding_type"] == "stale_evidence")
    );
}

#[test]
fn tampered_artifact_digest_fails_closed() {
    let scratch = tempfile_dir();
    let mut artifact: Value = serde_json::from_str(
        &std::fs::read_to_string(repository_root().join(EVIDENCE)).expect("read artifact"),
    )
    .expect("artifact JSON");
    artifact["support"] = serde_json::json!("challenges");
    let path = scratch.join("tampered.causal.json");
    std::fs::write(&path, artifact.to_string()).expect("write artifact");
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--projection",
        "causal_evidence_graph",
        "--evidence",
        path.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 2);
    assert_eq!(output["result"], "error");
    assert_eq!(output["diagnostic"], "causal_evidence_digest_mismatch");
}

#[test]
fn review_profile_with_evidence_appends_support_map() {
    let (output, status) = run_cli(&[
        "causal",
        "analyze",
        RETENTION,
        "--profile",
        "causal-review",
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
    ]);
    assert_eq!(status, 0);
    assert_eq!(
        output["causal_support"]["C_Onboarding_FirstSuccess"],
        "supported"
    );
    // The review profile still never claims formal verdicts.
    assert_review_only(&output);
}

#[test]
fn verify_expectations_checks_bounded_and_never_touches_claim_axes() {
    let (output, status) = run_cli(&["causal", "verify-expectations", INCIDENT, "--depth", "8"]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "causal_expectations_checked");
    assert_eq!(output["schema_version"], "causal-expectations.v0");
    assert_review_only(&output);
    let expectations = output["expectations"].as_array().expect("expectations");
    let verdict = |id: &str| {
        expectations
            .iter()
            .find(|entry| entry["id"] == format!("expectation:{id}"))
            .unwrap_or_else(|| panic!("missing {id}"))["verdict"]
            .clone()
    };
    // Pass and violated goldens: in BOTH cases every claim stays not_run /
    // untested (issue #323's central invariant).
    assert_eq!(verdict("E_GuardrailsVisible"), "pass");
    assert_eq!(verdict("E_AlertQualityImproves"), "violated");
    for claim in output["claims"].as_array().expect("claims") {
        assert_eq!(claim["formal_assurance"], "not_run");
        assert_eq!(claim["causal_support"], "untested");
    }
    for entry in expectations {
        assert!(
            entry["do_not_assume"]
                .as_array()
                .expect("list")
                .iter()
                .any(|line| line == "Expectation violation refutes the causal claim")
        );
        assert_eq!(entry["assurance"], "bounded");
        assert_eq!(
            entry["derived_from_claim"],
            "claim:C_Guardrails_AlertQuality"
        );
    }
}

fn incident_with(replacement: &str, target: &str) -> PathBuf {
    let scratch = tempfile_dir();
    std::fs::copy(
        repository_root().join("examples/causal/incident_system.fsl"),
        scratch.join("incident_system.fsl"),
    )
    .expect("copy companion");
    let source = std::fs::read_to_string(repository_root().join(INCIDENT))
        .expect("read model")
        .replace(target, replacement);
    let path = scratch.join("model.fsl");
    std::fs::write(&path, source).expect("write model");
    path
}

#[test]
fn expectation_clock_gates_fail_closed() {
    // Missing clock reference (only the expectation reference is renamed;
    // the clock declaration itself keeps its name).
    let model = incident_with(
        "clock missing_clock\n    derived_from_claim",
        "clock ops_clock\n    derived_from_claim",
    );
    let (output, status) = run_cli(&[
        "causal",
        "verify-expectations",
        model.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 2);
    assert_eq!(output["diagnostic"], "causal_expectation_invalid");
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("unknown clock")
    );
    // Fractional tick conversion: 3 ticks = 2 days makes within 5 fractional.
    let model = incident_with("3 tick = 2 day", "1 tick = 1 day");
    let (output, status) = run_cli(&[
        "causal",
        "verify-expectations",
        model.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 2);
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("exact integer number of kernel ticks")
    );
}

#[test]
fn expectation_rejects_legacy_supports_field_and_unresolved_targets() {
    let model = incident_with(
        "supports C_Guardrails_AlertQuality",
        "derived_from_claim C_Guardrails_AlertQuality",
    );
    let (output, status) = run_cli(&[
        "causal",
        "verify-expectations",
        model.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 2);
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("derived_from_claim")
    );
    // A response referencing a non-state name (e.g. a KPI-style metric that
    // does not exist in the target state space) fails closed at build time.
    let model = incident_with(
        "response predicate ops { average_mttr_delta >= 1 }",
        "response predicate ops { alert_precision >= 4 }",
    );
    let (output, status) = run_cli(&[
        "causal",
        "verify-expectations",
        model.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 2);
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("state space")
    );
}

#[test]
fn generated_leadsto_matches_a_hand_written_property() {
    // Hand-write the same pulse encoding + leadsTo in a kernel spec and
    // confirm the ordinary verifier agrees with verify-expectations.
    let scratch = tempfile_dir();
    let source = std::fs::read_to_string(
        repository_root().join("examples/causal/incident_system.fsl"),
    )
    .expect("read spec")
    .replace(
        "    mttr_hours: 0..100\n  }",
        "    mttr_hours: 0..100,\n    fired: Bool\n  }",
    )
    .replace(
        "    mttr_hours = 24\n  }",
        "    mttr_hours = 24\n    fired = false\n  }",
    )
    .replace(
        "    guardrails = guardrails + 1\n  }",
        "    guardrails = guardrails + 1\n    fired = true\n  }",
    )
    .replace(
        "    alert_precision = alert_precision + 1\n  }",
        "    alert_precision = alert_precision + 1\n    fired = false\n  }",
    )
    .replace(
        "    mttr_hours = mttr_hours - 1\n  }",
        "    mttr_hours = mttr_hours - 1\n    fired = false\n  }",
    )
    .replace(
        "  invariant PrecisionBounded",
        "  leadsTo Manual { fired ~> within 5 alert_precision >= 4 }\n  invariant PrecisionBounded",
    );
    let path = scratch.join("manual.fsl");
    std::fs::write(&path, source).expect("write spec");
    let (output, status) = run_cli(&["verify", path.to_str().expect("utf-8"), "--depth", "8"]);
    // The hand-written property is violated exactly like the generated one.
    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["invariant"], "Manual");
}
