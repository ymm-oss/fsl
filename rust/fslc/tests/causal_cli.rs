// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! CLI contract tests for the review-only `fslc causal` command family
//! (issue #321, `docs/DESIGN-causal.md`). The load-bearing invariants:
//! deterministic JSON, `formal_result: "not_run"` everywhere, a
//! `do_not_assume` array on every success payload, and no output path that
//! attaches `proved`/`verified` to a causal claim.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn run_process(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc")
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
    let output = run_process(arguments);
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

#[test]
fn extracted_boundary_preserves_argument_failures_and_raw_sibling_stdout() {
    for (arguments, message) in [
        (vec!["causal"], "usage: fslc causal"),
        (vec!["causal", "check"], "fslc causal check requires a file"),
        (
            vec!["causal", "check", RETENTION, "--unknown"],
            "unknown causal check option '--unknown'",
        ),
        (
            vec!["causal", "analyze", RETENTION, "--projection"],
            "--projection requires a value",
        ),
    ] {
        let output = run_process(&arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stderr.is_empty(), "{arguments:?}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("usage JSON stdout");
        assert_eq!(value["result"], "error", "{arguments:?}");
        assert_eq!(value["kind"], "usage", "{arguments:?}");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|actual| actual.contains(message)),
            "{arguments:?}: {value}"
        );
    }

    let formatted = run_process(&["fmt", "specs/cart_v1.fsl"]);
    assert_eq!(formatted.status.code(), Some(0));
    assert!(formatted.stderr.is_empty());
    assert!(formatted.stdout.starts_with(b"spec ShoppingCart"));
    assert!(
        serde_json::from_slice::<Value>(&formatted.stdout).is_err(),
        "raw formatter output must not cross the JSON serialization branch"
    );
}

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
    assert_eq!(output["loc"], serde_json::json!({"line": 36, "column": 18}));
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
    assert_eq!(
        feedback["claims"].as_array().expect("witness claims").len(),
        4
    );
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

// ── observe-expectations (issue #360) ──────────────────────────────

const OBS_LOG: &str = "examples/causal/evidence/incident-observation-log.jsonl";
const OBS_MAPPING: &str = "examples/causal/evidence/incident-log-mapping.fsl";
const OBS_SCOPE: &str = "examples/causal/evidence/incident-replay-scope.json";

fn run_observe(extra: &[&str]) -> (Value, i32) {
    let mut args = vec![
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        OBS_MAPPING,
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ];
    args.extend_from_slice(extra);
    run_cli(&args)
}

fn generated_evidence_paths(directory: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut evidence = Vec::new();
    let mut lifecycle = Vec::new();
    for entry in std::fs::read_dir(directory).expect("read generated artifacts") {
        let path = entry.expect("artifact entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&source) else {
            continue;
        };
        match value["schema_version"].as_str() {
            Some("fsl-causal-evidence.v0") => evidence.push(path),
            Some("fsl-causal-evidence-lifecycle.v0") => lifecycle.push(path),
            _ => {}
        }
    }
    evidence.sort();
    lifecycle.sort();
    (evidence, lifecycle)
}

fn analyze_with_generated_evidence(
    model: &Path,
    evidence: &[PathBuf],
    lifecycle: &[PathBuf],
) -> (Value, i32) {
    let mut args = vec![
        "causal".to_owned(),
        "analyze".to_owned(),
        model.to_str().expect("utf-8 model path").to_owned(),
        "--projection".to_owned(),
        "causal_evidence_graph".to_owned(),
    ];
    for path in evidence {
        args.push("--evidence".to_owned());
        args.push(path.to_str().expect("utf-8 evidence path").to_owned());
    }
    for path in lifecycle {
        args.push("--lifecycle".to_owned());
        args.push(path.to_str().expect("utf-8 lifecycle path").to_owned());
    }
    let arguments: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli(&arguments)
}

#[test]
fn observe_expectations_pass_and_violated_golden() {
    let (output, status) = run_observe(&[]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "causal_expectations_observed");
    assert_eq!(output["schema_version"], "causal-observation.v0");
    assert_review_only(&output);
    let expectations = output["expectations"].as_array().expect("expectations");
    let verdict = |id: &str| {
        expectations
            .iter()
            .find(|entry| entry["id"] == format!("expectation:{id}"))
            .unwrap_or_else(|| panic!("missing {id}"))["verdict"]
            .clone()
    };
    assert_eq!(verdict("E_GuardrailsVisible"), "pass");
    assert_eq!(verdict("E_AlertQualityImproves"), "violated");
    // Claims must never move from not_run/untested (AC 2).
    for claim in output["claims"].as_array().expect("claims") {
        assert_eq!(claim["formal_assurance"], "not_run");
        assert_eq!(claim["causal_support"], "untested");
    }
    // Every expectation carries replay-observed assurance and do_not_assume (AC 3).
    for entry in expectations {
        assert_eq!(entry["assurance"], "replay-observed");
        assert!(
            entry["do_not_assume"]
                .as_array()
                .expect("list")
                .iter()
                .any(|line| line == "Temporal co-occurrence establishes causality"),
            "must include temporal co-occurrence caveat"
        );
    }
}

#[test]
fn observe_expectations_generates_valid_evidence_artifacts() {
    let scratch = tempfile_dir();
    let out = scratch.join("evidence.causal.json");
    let lifecycle_out = scratch.join("evidence.lifecycle.json");
    let (output, status) = run_observe(&[
        "--out",
        out.to_str().expect("utf-8"),
        "--lifecycle-out",
        lifecycle_out.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 0, "{output}");
    assert!(output["artifacts_generated"].as_u64().unwrap_or(0) >= 2);

    // Read the generated evidence artifact (first of two files).
    let evidence_files: Vec<_> = std::fs::read_dir(&scratch)
        .expect("read dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.file_name()?.to_str()?.contains("evidence")
                && path.extension()?.to_str()? == "json"
                && !path.file_name()?.to_str()?.contains("lifecycle")
            {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    assert!(
        !evidence_files.is_empty(),
        "at least one evidence file must be generated"
    );

    for evidence_path in &evidence_files {
        let artifact: Value =
            serde_json::from_str(&std::fs::read_to_string(evidence_path).expect("read evidence"))
                .expect("parse evidence JSON");
        assert_eq!(artifact["schema_version"], "fsl-causal-evidence.v0");
        assert_eq!(artifact["design"], "observational", "AC 3");
        assert_eq!(artifact["support"], "inconclusive", "AC 3/4");
        assert_eq!(artifact["formal_result"], "not_run");
        let observation = &artifact["observation"];
        assert_eq!(observation["kind"], "expectation_replay");
        assert_eq!(observation["assurance"], "replay-observed", "AC 1");
    }

    // Lifecycle files exist and parse.
    let lifecycle_files: Vec<_> = std::fs::read_dir(&scratch)
        .expect("read dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.file_name()?.to_str()?.contains("lifecycle") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    assert!(
        !lifecycle_files.is_empty(),
        "at least one lifecycle file must be generated"
    );
    for lifecycle_path in &lifecycle_files {
        let chain: Value =
            serde_json::from_str(&std::fs::read_to_string(lifecycle_path).expect("read lifecycle"))
                .expect("parse lifecycle JSON");
        assert_eq!(chain["schema_version"], "fsl-causal-evidence-lifecycle.v0");
        let records = chain["records"].as_array().expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["sequence"], 1);
        assert_eq!(records[0]["status"], "active");
    }
}

#[test]
fn observe_expectations_fails_without_scope() {
    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        OBS_MAPPING,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ]);
    assert_eq!(status, 2, "{output}");
    assert!(
        output["message"].as_str().unwrap_or("").contains("--scope"),
        "error must name the missing --scope flag"
    );
}

#[test]
fn observe_expectations_rejects_enum_conversion_without_typed_impl_model() {
    let scratch = tempfile_dir();
    let mapping = scratch.join("enum-conversion-mapping.fsl");
    std::fs::write(
        &mapping,
        r"refinement ExternalRefinesIncident {
  impl External
  abs IncidentResponse

  enum conversion status ExternalStatus -> IncidentStatus {
    Open -> Open
  }
}
",
    )
    .expect("write mapping");

    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        mapping.to_str().expect("utf-8 mapping path"),
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "type");
    assert_eq!(output["loc"], serde_json::json!({"line": 5, "column": 3}));
    assert!(
        output["message"]
            .as_str()
            .unwrap_or_default()
            .contains("typed impl model")
    );
}

#[test]
fn observe_expectations_fails_without_period() {
    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        OBS_MAPPING,
        "--scope",
        OBS_SCOPE,
    ]);
    assert_eq!(status, 2, "{output}");
    assert!(
        output["message"]
            .as_str()
            .unwrap_or("")
            .contains("--period-start"),
        "error must name the missing --period-start flag"
    );
}

#[test]
fn observe_expectations_rejects_nonconformant_log() {
    let scratch = tempfile_dir();
    let bad_log = scratch.join("bad.jsonl");
    // State says guardrails=0 after deploy_guardrails — nonconformant.
    std::fs::write(
        &bad_log,
        r#"{"action":"deploy_guardrails","params":{},"state":{"guardrails":0,"alert_precision":3,"mttr_hours":24}}"#,
    )
    .expect("write bad log");
    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        bad_log.to_str().expect("utf-8"),
        "--mapping",
        OBS_MAPPING,
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ]);
    assert_eq!(status, 2, "{output}");
    assert!(
        output["kind"]
            .as_str()
            .unwrap_or("")
            .contains("nonconformant"),
        "nonconformant log must be fail-closed"
    );
}

#[test]
fn observe_expectations_rejects_tampered_mapping() {
    let scratch = tempfile_dir();
    std::fs::copy(
        repository_root().join("examples/causal/incident_system.fsl"),
        scratch.join("incident_system.fsl"),
    )
    .expect("copy companion");
    let bad_mapping = scratch.join("bad_mapping.fsl");
    // Map deploy_guardrails to a nonexistent action.
    std::fs::write(
        &bad_mapping,
        "refinement BadMapping {\n  impl Bad\n  abs IncidentSystem\n  action deploy_guardrails() -> nonexistent_action()\n  maps auto\n}\n",
    )
    .expect("write bad mapping");
    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        bad_mapping.to_str().expect("utf-8"),
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ]);
    assert_eq!(status, 2, "{output}");
}

// ── observe-expectations negative controls (issue #360 AC6/AC7) ────

#[test]
fn observe_expectations_rejects_short_observation_window() {
    let scratch = tempfile_dir();
    let out = scratch.join("short.causal.json");
    let lifecycle_out = scratch.join("short.lifecycle.json");
    let (output, status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        OBS_MAPPING,
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-01-02",
        "--out",
        out.to_str().expect("utf-8"),
        "--lifecycle-out",
        lifecycle_out.to_str().expect("utf-8"),
    ]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "causal_expectations_observed");

    let (evidence, lifecycle) = generated_evidence_paths(&scratch);
    assert!(!evidence.is_empty(), "observation must generate evidence");
    let (analyzed, status) =
        analyze_with_generated_evidence(&repository_root().join(INCIDENT), &evidence, &lifecycle);
    assert_eq!(status, 0, "{analyzed}");
    let finding = analyzed["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["finding_type"] == "evidence_window_shorter_than_lag")
        .expect("short observation window must be excluded at consumption");
    assert_eq!(finding["witness"]["window"], 1);
}

#[test]
fn observe_expectations_claim_version_change_does_not_auto_update_support() {
    let scratch = tempfile_dir();
    let out = scratch.join("version-one.causal.json");
    let lifecycle_out = scratch.join("version-one.lifecycle.json");
    let (observed, observed_status) = run_observe(&[
        "--out",
        out.to_str().expect("utf-8"),
        "--lifecycle-out",
        lifecycle_out.to_str().expect("utf-8"),
    ]);
    assert_eq!(observed_status, 0, "{observed}");

    std::fs::copy(
        repository_root().join("examples/causal/incident_system.fsl"),
        scratch.join("incident_system.fsl"),
    )
    .expect("copy companion");
    let source = std::fs::read_to_string(repository_root().join(INCIDENT))
        .expect("read model")
        .replace(
            "claim C_Guardrails_AlertQuality guardrails -> alert_quality {\n    version 1",
            "claim C_Guardrails_AlertQuality guardrails -> alert_quality {\n    version 2",
        );
    let model_path = scratch.join("model.fsl");
    std::fs::write(&model_path, source).expect("write model");

    let (evidence, lifecycle) = generated_evidence_paths(&scratch);
    assert!(!evidence.is_empty(), "observation must generate evidence");
    let (output, status) = analyze_with_generated_evidence(&model_path, &evidence, &lifecycle);
    assert_eq!(status, 0, "{output}");
    let mismatch = output["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| {
            finding["finding_type"] == "evidence_claim_version_mismatch"
                && finding["witness"]["current_version"] == 2
        })
        .expect("version-one evidence must not apply to version two claim");
    assert_eq!(mismatch["witness"]["pinned_version"], 1);
    let claim = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|claim| claim["id"] == "claim:C_Guardrails_AlertQuality")
        .expect("changed claim");
    assert_eq!(claim["formal_assurance"], "not_run");
    assert_eq!(claim["causal_support"], "unsupported_by_current_evidence");
}

// ── ledger (issue #364) ────────────────────────────────────────────

const PLAN: &str = "examples/causal/evidence/onboarding-validation-plan.json";
const PLAN_LIFECYCLE: &str = "examples/causal/evidence/onboarding-validation-plan.lifecycle.json";

fn run_ledger(extra: &[&str]) -> (Value, i32) {
    let mut args = vec!["causal", "ledger", RETENTION];
    args.extend_from_slice(extra);
    run_cli(&args)
}

fn claim_attention(output: &Value, claim_id: &str) -> Vec<String> {
    output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|entry| entry["id"] == format!("claim:{claim_id}"))
        .unwrap_or_else(|| panic!("missing {claim_id}"))["attention_reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason["reason"].as_str().expect("reason string").to_owned())
        .collect()
}

#[test]
fn ledger_all_claims_appear_even_without_plans_or_evidence() {
    let (output, status) = run_ledger(&[]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "causal_ledger");
    assert_eq!(output["schema_version"], "causal-ledger.v0");
    assert_review_only(&output);
    let claims = output["claims"].as_array().expect("claims");
    assert_eq!(claims.len(), 4, "all 4 retention claims must appear");
    for entry in claims {
        assert_eq!(entry["formal_assurance"], "not_run");
        assert_eq!(entry["status"], "active");
    }
}

#[test]
fn ledger_plan_missing_attention_fires_without_plans() {
    let (output, status) = run_ledger(&[]);
    assert_eq!(status, 0, "{output}");
    let reasons = claim_attention(&output, "C_Onboarding_FirstSuccess");
    assert!(
        reasons.contains(&"validation_plan_missing".to_owned()),
        "plan_missing must fire: {reasons:?}"
    );
    assert!(
        reasons.contains(&"current_evidence_missing".to_owned()),
        "evidence_missing must fire: {reasons:?}"
    );
}

#[test]
fn ledger_with_plan_and_evidence_clears_attention() {
    let (output, status) = run_ledger(&[
        "--plans",
        PLAN,
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
        "--lifecycle",
        PLAN_LIFECYCLE,
        "--as-of",
        "2027-01-15",
    ]);
    assert_eq!(status, 0, "{output}");
    let reasons = claim_attention(&output, "C_Onboarding_FirstSuccess");
    assert!(
        !reasons.contains(&"validation_plan_missing".to_owned()),
        "plan should be applicable: {reasons:?}"
    );
    let claim = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|entry| entry["id"] == "claim:C_Onboarding_FirstSuccess")
        .expect("claim");
    assert_eq!(claim["causal_support"], "supported");
    assert!(!claim["external_refs"].as_array().expect("refs").is_empty());
}

#[test]
fn ledger_deterministic_output() {
    let args = &[
        "--plans",
        PLAN,
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
        "--lifecycle",
        PLAN_LIFECYCLE,
        "--as-of",
        "2027-01-15",
    ];
    let (output_a, _) = run_ledger(args);
    let (output_b, _) = run_ledger(args);
    assert_eq!(
        output_a.to_string(),
        output_b.to_string(),
        "ledger must be byte-stable"
    );
}

#[test]
fn ledger_claims_without_plan_have_missing_attention() {
    let (output, status) = run_ledger(&[
        "--plans",
        PLAN,
        "--evidence",
        EVIDENCE,
        "--lifecycle",
        LIFECYCLE,
        "--lifecycle",
        PLAN_LIFECYCLE,
        "--as-of",
        "2027-01-15",
    ]);
    assert_eq!(status, 0, "{output}");
    // Claims other than C_Onboarding_FirstSuccess should still have
    // validation_plan_missing because the plan only pins that one claim.
    let reasons = claim_attention(&output, "C_Habit_Retention");
    assert!(
        reasons.contains(&"validation_plan_missing".to_owned()),
        "claims without plans must have plan_missing: {reasons:?}"
    );
}

#[test]
fn ledger_retired_claim_has_no_attention_but_appears() {
    // Build a model with one retired claim.
    let scratch = tempfile_dir();
    std::fs::copy(
        repository_root().join("examples/causal/subscription_business.fsl"),
        scratch.join("subscription_business.fsl"),
    )
    .expect("copy companion");
    let source = std::fs::read_to_string(repository_root().join(RETENTION))
        .expect("read model")
        .replace(
            "claim C_Retention_Onboarding retention_90d -> onboarding_support {\n    version 1\n    status active",
            "claim C_Retention_Onboarding retention_90d -> onboarding_support {\n    version 2\n    status retired",
        );
    let model_path = scratch.join("model.fsl");
    std::fs::write(&model_path, source).expect("write model");
    let (output, status) = run_cli(&["causal", "ledger", model_path.to_str().expect("utf-8")]);
    assert_eq!(status, 0, "{output}");
    // Retired claim appears.
    let retired = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|entry| entry["id"] == "claim:C_Retention_Onboarding")
        .expect("retired claim must appear (AC 10)");
    assert_eq!(retired["status"], "retired");
    // Retired claim has no attention reasons.
    assert_eq!(
        retired["attention_reasons"]
            .as_array()
            .expect("reasons")
            .len(),
        0,
        "retired claims must have no attention reasons (AC 10)"
    );
}

#[test]
fn ledger_plan_version_mismatch_fires_attention() {
    // The plan pins version 1, but change the model claim to version 2.
    let scratch = tempfile_dir();
    std::fs::copy(
        repository_root().join("examples/causal/subscription_business.fsl"),
        scratch.join("subscription_business.fsl"),
    )
    .expect("copy companion");
    let source = std::fs::read_to_string(repository_root().join(RETENTION))
        .expect("read model")
        .replace(
            "claim C_Onboarding_FirstSuccess onboarding_support -> first_success {\n    version 1",
            "claim C_Onboarding_FirstSuccess onboarding_support -> first_success {\n    version 2",
        );
    let model_path = scratch.join("model.fsl");
    std::fs::write(&model_path, source).expect("write model");
    let (output, status) = run_cli(&[
        "causal",
        "ledger",
        model_path.to_str().expect("utf-8"),
        "--plans",
        PLAN,
        "--lifecycle",
        PLAN_LIFECYCLE,
    ]);
    assert_eq!(status, 0, "{output}");
    let reasons: Vec<String> = output["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|entry| entry["id"] == "claim:C_Onboarding_FirstSuccess")
        .expect("claim")["attention_reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|r| r["reason"].as_str().expect("string").to_owned())
        .collect();
    assert!(
        reasons.contains(&"validation_plan_version_mismatch".to_owned()),
        "version mismatch must fire: {reasons:?} (AC 3/13)"
    );
}

#[test]
fn ledger_observation_evidence_stays_inconclusive_and_never_directed() {
    // Generate observation artifacts and verify they are always inconclusive.
    // Observation evidence without valid_until is excluded by freshness from
    // the support overlay, so the ledger reports evidence_freshness or
    // current_evidence_missing rather than observation_not_directional_support.
    // This test verifies the stronger invariant: observation-generated
    // artifacts are always design:observational, support:inconclusive,
    // and claim axes never move (AC 8 core guarantee).
    let scratch = tempfile_dir();
    let out_path = scratch.join("obs.json");
    let lc_path = scratch.join("obs.lc.json");
    let (obs_output, obs_status) = run_cli(&[
        "causal",
        "observe-expectations",
        INCIDENT,
        "--from-log",
        OBS_LOG,
        "--mapping",
        OBS_MAPPING,
        "--scope",
        OBS_SCOPE,
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
        "--out",
        out_path.to_str().expect("utf-8"),
        "--lifecycle-out",
        lc_path.to_str().expect("utf-8"),
    ]);
    assert_eq!(obs_status, 0, "{obs_output}");
    // Every generated artifact must be observational + inconclusive.
    for entry in std::fs::read_dir(&scratch).expect("read dir") {
        let path = entry.expect("entry").path();
        let name = path.file_name().unwrap().to_str().unwrap_or("");
        if !name.starts_with("obs.OBS_") || name.contains("lc.") {
            continue;
        }
        let artifact: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(artifact["design"], "observational", "AC 3/8: {name}");
        assert_eq!(artifact["support"], "inconclusive", "AC 3/8: {name}");
        assert!(
            artifact["observation"].is_object(),
            "must have observation object: {name}"
        );
    }
    // Ledger with observation evidence: claims must not gain directed support.
    let evidence_files: Vec<_> = std::fs::read_dir(&scratch)
        .expect("read dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("obs.OBS_") && !name.contains("lc.")).then_some(path)
        })
        .collect();
    let lifecycle_files: Vec<_> = std::fs::read_dir(&scratch)
        .expect("read dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            name.starts_with("obs.lc.OBS_").then_some(path)
        })
        .collect();
    let mut ledger_args: Vec<String> = vec!["causal".into(), "ledger".into(), INCIDENT.into()];
    for path in &evidence_files {
        ledger_args.push("--evidence".into());
        ledger_args.push(path.to_str().expect("utf-8").into());
    }
    for path in &lifecycle_files {
        ledger_args.push("--lifecycle".into());
        ledger_args.push(path.to_str().expect("utf-8").into());
    }
    let args_ref: Vec<&str> = ledger_args.iter().map(String::as_str).collect();
    let (output, status) = run_cli(&args_ref);
    assert_eq!(status, 0, "{output}");
    // No claim should have causal_support = "supported" or "challenged".
    for claim in output["claims"].as_array().expect("claims") {
        let support = claim["causal_support"].as_str().unwrap_or("");
        assert_ne!(
            support, "supported",
            "observation-only evidence must never produce directed support: {} (AC 8)",
            claim["id"]
        );
        assert_ne!(
            support, "challenged",
            "observation-only evidence must never produce directed challenge: {} (AC 8)",
            claim["id"]
        );
    }
}
