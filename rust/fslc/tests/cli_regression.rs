// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

fn run_cli(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

#[test]
fn native_cli_checks_a_repository_spec_without_python() {
    let (value, status) = run_cli(&["check", "specs/cart_v1.fsl"]);
    assert_eq!(status, 0);
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "ok");
    assert_eq!(value["spec"], "ShoppingCart");
    assert_eq!(value["versions"]["verifier"]["name"], "fslc-rust");
    assert_eq!(
        value["versions"]["verifier"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(value["versions"]["core"]["name"], "fsl-core");
    assert_eq!(
        value["versions"]["core"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(value["versions"]["solver"]["name"], "z3");
    assert_eq!(value["versions"]["solver"]["backend"], "native-z3");
    assert!(
        value["versions"]["solver"]["version"]
            .as_str()
            .is_some_and(|version| version.starts_with("Z3 4.16.0"))
    );
}

#[test]
fn native_verify_preserves_the_agent_document_boundary_error() {
    let (value, status) = run_cli(&[
        "verify",
        "examples/ai/recursive_support_agent.fsl",
        "--no-cache",
    ]);

    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "parse");
    assert_eq!(
        value["message"],
        "agent documents cannot be verified as Kernel specs"
    );
}

#[test]
fn native_check_and_verify_share_the_core_duplicate_write_gate() {
    let fixture = "rust/fslc/tests/fixtures/duplicate_write.fsl";

    for args in [
        vec!["check", fixture],
        vec!["verify", fixture, "--no-cache"],
    ] {
        let command = args[0];
        let (value, status) = run_cli(&args);
        assert_eq!(status, 2, "{command}");
        assert_eq!(value["result"], "error", "{command}");
        assert_eq!(value["kind"], "semantics", "{command}");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| message.contains("same state location")),
            "{command}: {value}"
        );
    }
}

#[test]
fn native_check_rejects_an_incomplete_governance_contract() {
    let (value, status) = run_cli(&[
        "check",
        "examples/gallery/errors/governance_missing_before.fsl",
    ]);

    assert_eq!(status, 2);
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "type");
    assert_eq!(value["loc"], serde_json::json!({"line": 4, "column": 3}));
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("governance preservation missing before")),
        "{value}"
    );
}

#[test]
fn native_check_rejects_a_missing_governance_dependency() {
    let (value, status) = run_cli(&[
        "check",
        "rust/fslc/tests/fixtures/governance_missing_dependency.fsl",
    ]);

    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "type");
    assert_eq!(value["loc"], serde_json::json!({"line": 6, "column": 5}));
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing-before.fsl")),
        "{value}"
    );
}

#[test]
fn native_check_reports_a_governance_counterexample_without_misclassifying_it() {
    let (value, status) = run_cli(&[
        "check",
        "examples/refinement_liveness/governance_detects_safety_loss.fsl",
    ]);

    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");
    assert_eq!(
        value["governance"]["preservations"][0]["result"],
        "refinement_failed"
    );
}

#[test]
fn native_check_locates_a_malformed_dependency_at_the_governance_reference() {
    let (value, status) = run_cli(&[
        "check",
        "examples/gallery/errors/governance_malformed_dependency.fsl",
    ]);

    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "type");
    assert_eq!(value["loc"], serde_json::json!({"line": 5, "column": 3}));
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("governance_malformed_business.fsl")),
        "{value}"
    );
}

#[test]
fn native_check_locates_a_semantic_dependency_error_at_the_preservation() {
    let (value, status) = run_cli(&[
        "check",
        "examples/gallery/adversarial/governance_semantic_dependency.fsl",
    ]);

    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "type");
    assert_eq!(value["loc"], serde_json::json!({"line": 6, "column": 3}));
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown type 'Missing'")),
        "{value}"
    );
}

#[test]
fn monitor_boundary_self_spec_is_proved_and_mutation_sensitive() {
    let fixture = "examples/self/monitor_action_boundary.fsl";
    let (checked, check_status) = run_cli(&["check", fixture, "--strict-tags"]);
    assert_eq!(check_status, 0, "{checked}");
    assert_eq!(checked["result"], "ok");

    let (verified, verify_status) = run_cli(&["verify", fixture, "--depth", "8", "--no-cache"]);
    assert_eq!(verify_status, 0, "{verified}");
    assert_eq!(verified["result"], "verified");

    let (proved, induction_status) = run_cli(&[
        "verify",
        fixture,
        "--engine",
        "induction",
        "--depth",
        "8",
        "--no-cache",
    ]);
    assert_eq!(induction_status, 0, "{proved}");
    assert_eq!(proved["result"], "proved");

    let (mutated, mutation_status) =
        run_cli(&["mutate", fixture, "--depth", "8", "--by-requirement"]);
    assert_eq!(mutation_status, 0, "{mutated}");
    assert_eq!(mutated["result"], "mutated");
    assert!(
        mutated["summary"]["kill_rate"]
            .as_f64()
            .is_some_and(|rate| rate >= 0.70),
        "{mutated}"
    );
    assert!(
        mutated["by_requirement"]["REQ-MONITOR-001"]["kills"]
            .as_u64()
            .is_some_and(|kills| kills > 0),
        "{mutated}"
    );
}

#[test]
fn mutate_attributes_trace_oracle_kills_to_attached_requirements() {
    let (mutated, status) = run_cli(&[
        "mutate",
        "rust/fslc/tests/fixtures/mutate_trace_requirement_attribution.fsl",
        "--depth",
        "8",
        "--by-requirement",
        "--max-mutants",
        "0",
        "--from",
        "rust/fslc/tests/fixtures/mutate_trace_requirement_attribution.jsonl",
    ]);

    assert_eq!(status, 0, "{mutated}");
    let mutants = mutated["mutants"].as_array().expect("mutants");
    assert_eq!(mutants.len(), 3, "{mutated}");
    assert_eq!(mutants[0]["status"], "killed", "{mutated}");
    assert_eq!(mutants[0]["killed_by"], "acceptance", "{mutated}");
    assert_eq!(mutants[1]["status"], "killed", "{mutated}");
    assert_eq!(mutants[1]["killed_by"], "forbidden", "{mutated}");
    assert_eq!(mutants[2]["status"], "killed", "{mutated}");
    assert_eq!(mutants[2]["killed_by"], "forbidden", "{mutated}");
    let requirement = &mutated["by_requirement"]["REQ-TEST-001"];
    assert_eq!(requirement["kills"], 3, "{mutated}");
    assert!(requirement.get("warning").is_none(), "{mutated}");
    assert_eq!(
        mutated["by_requirement"]["AC-TEST-001"]["kills"], 1,
        "{mutated}"
    );
    for case_id in ["FB-TEST-001", "FB-SETUP-001"] {
        assert!(
            mutated["by_requirement"].get(case_id).is_none(),
            "{mutated}"
        );
    }

    let (builtin, builtin_status) = run_cli(&[
        "mutate",
        "rust/fslc/tests/fixtures/mutate_trace_requirement_attribution.fsl",
        "--depth",
        "8",
        "--by-requirement",
        "--max-mutants",
        "100",
    ]);
    assert_eq!(builtin_status, 0, "{builtin}");
    let prepare_guard = builtin["mutants"]
        .as_array()
        .expect("builtin mutants")
        .iter()
        .find(|mutant| {
            mutant["op"] == "requires_negate"
                && mutant["target"]
                    .as_str()
                    .is_some_and(|target| target.starts_with("prepare requires"))
        })
        .expect("prepare guard mutant");
    assert_eq!(prepare_guard["status"], "killed", "{builtin}");
    assert_eq!(prepare_guard["killed_by"], "forbidden", "{builtin}");
    let builtin_requirement = &builtin["by_requirement"]["REQ-TEST-001"];
    assert!(
        builtin_requirement["kills"]
            .as_u64()
            .is_some_and(|kills| kills > 0),
        "{builtin}"
    );
    assert!(builtin_requirement.get("warning").is_none(), "{builtin}");
}

#[test]
fn native_check_and_verify_reject_duplicate_correspondence_origins() {
    let fixture = "rust/fslc/tests/fixtures/action_correspondence_duplicate.fsl";

    for args in [
        vec!["check", fixture],
        vec!["verify", fixture, "--no-cache"],
    ] {
        let command = args[0];
        let (value, status) = run_cli(&args);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(value["result"], "error", "{command}");
        assert_eq!(value["kind"], "type", "{command}");
        let message = value["message"].as_str().expect("diagnostic message");
        assert!(message.contains("implements_block"), "{command}: {message}");
        assert!(
            message.contains("inline_maps_clause"),
            "{command}: {message}"
        );
        assert_eq!(message.matches(" at ").count(), 2, "{command}: {message}");
    }
}

#[test]
fn native_cli_preserves_bmc_outcomes() {
    let (verified, status) = run_cli(&[
        "verify",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(verified["result"], "verified");
    assert_eq!(verified["completeness"], "bounded");

    let (violated, status) = run_cli(&[
        "verify",
        "specs/cart_buggy.fsl",
        "--depth",
        "5",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["violation_kind"], "invariant");
    assert_eq!(violated["invariant"], "NoNegativeStock");
}

#[test]
fn general_conditionals_cross_cli_verification_and_replay_paths() {
    let spec = "examples/conditional_expressions.fsl";
    let (checked, status) = run_cli(&["check", spec]);
    assert_eq!(status, 0);
    assert_eq!(checked["result"], "ok");

    let (conformance, status) = run_cli(&["conformance", spec, "--depth", "1"]);
    assert_eq!(status, 0);
    assert_eq!(conformance["schema_version"], "1.0.0");
    assert_eq!(conformance["kernel_schema_version"], "1.0.0");
    assert_eq!(conformance["vectors"][0]["outcome"]["state"]["count"], 2);

    let (bounded, status) = run_cli(&[
        "verify",
        spec,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(bounded["result"], "verified");

    let (induction, status) = run_cli(&[
        "verify",
        spec,
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(induction["result"], "proved");

    let trace = std::env::temp_dir().join(format!(
        "fsl-conditional-replay-{}.json",
        std::process::id()
    ));
    std::fs::write(&trace, r#"[{"action":"advance","params":{}}]"#).expect("write replay trace");
    let trace_path = trace.to_str().expect("UTF-8 trace path");
    let (replayed, status) = run_cli(&["replay", spec, "--trace", trace_path]);
    std::fs::remove_file(trace).expect("remove replay trace");
    assert_eq!(status, 0);
    assert_eq!(replayed["result"], "conformant");
    assert_eq!(replayed["steps_checked"], 1);
}

#[test]
fn conditional_type_diagnostics_point_to_the_invalid_child_expression() {
    for (fixture, location) in [
        (
            "rust/fslc/tests/fixtures/conditional_type_error.fsl",
            "at 7:29",
        ),
        (
            "rust/fslc/tests/fixtures/conditional_const_type_error.fsl",
            "at 4:37",
        ),
    ] {
        let (value, status) = run_cli(&["check", fixture]);

        assert_eq!(status, 2, "{fixture}");
        assert_eq!(value["result"], "error", "{fixture}");
        assert_eq!(value["kind"], "semantics", "{fixture}");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| message.ends_with(location)),
            "{fixture}: {value}"
        );
    }
}

#[test]
fn formatter_parenthesizes_a_conditional_used_as_an_operator_operand() {
    let expression = fsl_syntax::parse_expr("(if c then a else b) + 1").expect("parse expression");

    assert_eq!(
        fslc_rust::expr_text(&expression),
        "(if c then a else b) + 1"
    );
}

#[test]
fn requirements_stage_is_readable_in_explain_and_violation_evidence() {
    let fixture = "rust/fslc/tests/fixtures/requirements_stage.fsl";
    let (explained, status) = run_cli(&["explain", fixture, "--depth", "1"]);

    assert_eq!(status, 0, "{explained}");
    assert_eq!(
        explained["skeleton"]["properties"][0]["body_text"],
        "forall c: Claim { stage(c) == Draft }"
    );
    assert_eq!(
        explained["skeleton"]["properties"][0]["origin"]["lowering_steps"][0]["detail"],
        "stage(c) -> claim_stage[entity]"
    );

    let (violated, status) = run_cli(&[
        "verify",
        fixture,
        "--depth",
        "1",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{violated}");
    assert_eq!(
        violated["blame"]["conjuncts"][0]["text"],
        "forall c: Claim { stage(c) == Draft }"
    );
}

#[test]
fn qualified_requirements_stage_keeps_its_process_path_in_evidence() {
    let fixture = "rust/fslc/tests/fixtures/requirements_stage_qualified.fsl";
    let expected =
        "forall c: Claim { claims.Claim.stage(c) == Draft and legacy.Claim.stage(c) == Imported }";
    let (explained, status) = run_cli(&["explain", fixture, "--depth", "1"]);

    assert_eq!(status, 0, "{explained}");
    assert_eq!(
        explained["skeleton"]["properties"][0]["body_text"],
        expected
    );

    let (violated, status) = run_cli(&[
        "verify",
        fixture,
        "--depth",
        "1",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{violated}");
    assert_eq!(violated["blame"]["conjuncts"][0]["text"], expected);
}

#[test]
fn typed_requirement_relations_reach_strict_tags_scenarios_and_diagnostics() {
    let spec = "rust/fslc/tests/fixtures/typed_annotation_outputs.fsl";
    let requirements = "rust/fslc/tests/fixtures/typed_annotation_requirements.txt";

    let (checked, status) = run_cli(&[
        "check",
        spec,
        "--strict-tags",
        "--requirements",
        requirements,
    ]);
    assert_eq!(status, 0);
    assert_eq!(checked["result"], "ok");
    assert!(
        checked["warnings"]
            .as_array()
            .is_none_or(std::vec::Vec::is_empty)
    );

    let (violation, status) = run_cli(&[
        "verify",
        "rust/fslc/tests/fixtures/typed_annotation_violation.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(violation["result"], "violated");
    assert_eq!(violation["requirement"]["id"], "REQ-OUTER");
    assert_eq!(
        violation["requirements"],
        serde_json::json!([
            {"id":"REQ-OUTER","text":"outer requirement"},
            {"id":"REQ-SAFETY","text":"safety requirement"}
        ])
    );
    let (scenarios, status) = run_cli(&["scenarios", spec, "--depth", "2", "--deadlock", "ignore"]);
    assert_eq!(status, 0);
    let publish = scenarios["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .find(|scenario| scenario["name"] == "cover_publish")
        .expect("publish coverage scenario");
    assert_eq!(publish["requirement"]["id"], "REQ-ACTION");
    assert_eq!(
        publish["requirements"],
        serde_json::json!([
            {"id":"REQ-ACTION","text":"action requirement"},
            {"id":"REQ-OUTER","text":"outer requirement"}
        ])
    );

    let (explained, status) = run_cli(&["explain", spec, "--depth", "2"]);
    assert_eq!(status, 0);
    let explained_publish = explained["witnesses"]
        .as_array()
        .expect("witnesses")
        .iter()
        .find(|witness| witness["name"] == "cover_publish")
        .expect("explained publish witness");
    assert_eq!(explained_publish["requirements"], publish["requirements"]);

    let (mutated, status) = run_cli(&[
        "mutate",
        spec,
        "--depth",
        "2",
        "--max-mutants",
        "100",
        "--by-requirement",
    ]);
    assert_eq!(status, 0);
    for id in ["REQ-ACTION", "REQ-OUTER", "REQ-REACH", "REQ-SAFETY"] {
        assert!(mutated["by_requirement"].get(id).is_some(), "missing {id}");
    }
    let publish_mutant = mutated["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["target"]
                .as_str()
                .is_some_and(|target| target.starts_with("publish "))
        })
        .expect("publish mutant");
    assert_eq!(
        publish_mutant["requirements"],
        serde_json::json!([
            {"id":"REQ-ACTION","text":"action requirement"},
            {"id":"REQ-OUTER","text":"outer requirement"}
        ])
    );
}

#[test]
fn native_cli_replays_witnesses_from_partially_initialized_state() {
    let (violated, status) = run_cli(&[
        "verify",
        "tests/fixtures/rust_port/partial_map_init.fsl",
        "--depth",
        "4",
        "--engine",
        "bmc",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(violated["result"], "violated");
    assert_eq!(violated["violation_kind"], "invariant");
    assert_eq!(violated["invariant"], "NotB");
    assert_eq!(violated["violated_at_step"], 0);
    assert_eq!(violated["trace"][0]["state"]["values"]["A"], false);
    assert_eq!(violated["trace"][0]["state"]["values"]["B"], true);
}

#[test]
fn native_cli_preserves_induction_outcomes() {
    let (proved, status) = run_cli(&[
        "verify",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0);
    assert_eq!(proved["result"], "proved");
    assert_eq!(proved["engine"], "induction");
    assert_eq!(proved["completeness"], "unbounded");
    assert!(proved["cost"]["solver"]["checks"].as_u64().unwrap_or(0) > 0);
    assert!(
        !proved["cost"]["properties"]
            .as_array()
            .expect("induction property cost")
            .is_empty()
    );

    let (cti, status) = run_cli(&[
        "verify",
        "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1);
    assert_eq!(cti["result"], "unknown_cti");
    assert_eq!(cti["invariant"], "Sync");
    assert_eq!(cti["completeness"], "bounded");
    assert!(cti["cost"]["solver"]["checks"].as_u64().unwrap_or(0) > 0);
}
