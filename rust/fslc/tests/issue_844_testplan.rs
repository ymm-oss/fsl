// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Controls for #844 slice 1: `fslc testplan` selects bounded conformance vectors
//! into a closed `test-plan.v1` document without reimplementing oracle semantics.

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{KERNEL_SCHEMA_VERSION, TEST_PLAN_V1_SCHEMA_ID, TEST_PLAN_V1_SCHEMA_VERSION};
use fsl_tools::validate_test_plan_v1;
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/{name}")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn compiled_schema() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("schemas/fslc/kernel/test-plan.v1.schema.json"))
            .expect("read test-plan schema"),
    )
    .expect("test-plan schema JSON");
    jsonschema::validator_for(&schema).expect("schema compiles")
}

fn case_by_name<'a>(plan: &'a Value, name: &str) -> &'a Value {
    plan["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["name"] == name)
        .unwrap_or_else(|| panic!("missing case '{name}'"))
}

/// REJECTING CONTROL. Unknown subcommands must not silently succeed.
#[test]
fn unknown_testplan_subcommand_fails_closed() {
    let (value, status) = run(&["testplan", "export", &fixture("testplan_limit.fsl")]);
    assert_eq!(status, 2, "{value:#}");
}

/// ACCEPTING CONTROL. Limit spec at depth 0 must emit boundary accept/reject cases.
#[test]
fn limit_spec_emits_boundary_accept_and_reject_cases() {
    let spec = fixture("testplan_limit.fsl");
    let (plan, status) = run(&["testplan", &spec, "--depth", "0"]);
    assert_eq!(status, 0, "{plan:#}");
    assert_eq!(plan["result"], "testplan");
    assert_eq!(plan["spec"], "Limit");
    assert_eq!(plan["formal_result"], "not_run");
    assert_eq!(plan["assurance_effect"], "none");
    assert_eq!(plan["oracle"]["evaluator_reimplemented"], false);
    compiled_schema()
        .validate(&plan)
        .expect("plan must match test-plan.v1 schema");

    let accept = case_by_name(&plan, "boundary_accept_q");
    assert_eq!(accept["target"]["action"], "submit");
    assert_eq!(accept["target"]["params"]["q"], 1);
    assert_eq!(accept["expected"]["outcome_kind"], "ok");
    assert_eq!(accept["expected"]["state"]["accepted"], true);

    let reject = case_by_name(&plan, "boundary_reject_q");
    assert_eq!(reject["target"]["params"]["q"], 2);
    assert_eq!(reject["expected"]["outcome_kind"], "requires_failed");
    assert_eq!(reject["expected"]["state"]["accepted"], false);
    assert_eq!(reject["expected"]["state_unchanged"], true);
}

/// REJECTING CONTROL. Selection coverage is descriptive only and must not hide
/// unselected vectors.
#[test]
fn selection_coverage_counts_available_and_selected_vectors() {
    let spec = fixture("testplan_limit.fsl");
    let (plan, status) = run(&["testplan", &spec, "--depth", "0"]);
    assert_eq!(status, 0, "{plan:#}");
    let coverage = &plan["selection_coverage"];
    assert_eq!(coverage["vectors_available"], 3);
    assert!(coverage["vectors_selected"].as_u64().unwrap() >= 2);
    let uncovered = coverage["uncovered"]
        .as_array()
        .expect("uncovered")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        uncovered.contains(&"v0"),
        "q=0 ok vector remains uncovered: {uncovered:?}"
    );
}

/// REJECTING CONTROL. Compose specs cannot produce a truthful Public Kernel pair.
#[test]
fn compose_spec_is_rejected_before_planning() {
    let (value, status) = run(&["testplan", "specs/bank_system.fsl", "--depth", "0"]);
    assert_eq!(status, 2, "{value:#}");
    let message = value["message"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .unwrap_or("");
    assert!(
        message.contains("compose unsupported"),
        "unexpected message: {message}"
    );
}

/// REJECTING CONTROL. Fail-closed decoder rejects an unknown outcome kind.
#[test]
fn validate_test_plan_rejects_unknown_outcome_kind() {
    let mut plan = valid_limit_plan();
    plan["cases"][0]["expected"]["outcome_kind"] = json!("guard_failed");
    let error = validate_test_plan_v1(&plan).expect_err("guard_failed must be rejected");
    assert!(error.contains("guard_failed"), "{error}");
}

/// REJECTING CONTROL. Fail-closed decoder rejects assurance escalation.
#[test]
fn validate_test_plan_rejects_assurance_escalation() {
    let mut plan = valid_limit_plan();
    plan["formal_result"] = json!("proved");
    let error = validate_test_plan_v1(&plan).expect_err("proved must be rejected");
    assert!(error.contains("formal_result"), "{error}");
}

/// REJECTING CONTROL. Fail-closed decoder rejects unknown top-level fields.
#[test]
fn validate_test_plan_rejects_unknown_fields() {
    let mut plan = valid_limit_plan();
    let object = plan.as_object_mut().expect("object");
    object.insert("verified".to_owned(), json!(true));
    let error = validate_test_plan_v1(&plan).expect_err("unknown field");
    assert!(error.contains("verified"), "{error}");
}

/// REJECTING CONTROL. Layer-selection guidance must be present in CLI help.
#[test]
fn cli_help_documents_same_layer_requirement() {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["testplan", "--help"])
        .current_dir(root())
        .output()
        .expect("run testplan --help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("same FSL layer granularity"),
        "help must document layer selection: {help}"
    );
}

fn valid_limit_plan() -> Value {
    json!({
        "$schema": TEST_PLAN_V1_SCHEMA_ID,
        "schema_version": TEST_PLAN_V1_SCHEMA_VERSION,
        "kernel_schema_version": KERNEL_SCHEMA_VERSION,
        "result": "testplan",
        "spec": "Limit",
        "depth": 0,
        "initial": {"accepted": false},
        "formal_result": "not_run",
        "assurance_effect": "none",
        "oracle": {"producer": "fslc conformance", "evaluator_reimplemented": false},
        "selection_coverage": {"vectors_available": 3, "vectors_selected": 2, "uncovered": ["v0"]},
        "do_not_assume": [
            "not proof of implementation correctness",
            "not exhaustive beyond the declared depth and finite scope",
            "selection coverage is not completeness",
            "does not replace verify, induction, replay, or refinement"
        ],
        "layer_selection": {
            "requirement": "pass the spec at the same FSL layer granularity as the implementation you are checking; from an upper layer reuse forbidden (negative) scenarios only"
        },
        "cases": [{
            "name": "boundary_accept_q",
            "setup": [],
            "target": {"action": "submit", "params": {"q": 1}, "expected": {"accepted": true}},
            "expected": {"outcome_kind": "ok", "state": {"accepted": true}},
            "source_vector": {"state": "s0", "action": {"name": "submit", "params": {"q": 1}}}
        }]
    })
}

/// REJECTING CONTROL. A corrupted conformance outcome kind must abort planning.
#[test]
fn unknown_conformance_outcome_kind_aborts_plan_generation() {
    use fsl_core::{FsResolver, build_model, parse_kernel_source, public_kernel_contract};
    use fsl_tools::build_test_plan_v1;

    let source = std::fs::read_to_string(root().join(fixture("testplan_limit.fsl")))
        .expect("read limit fixture");
    let resolver = FsResolver::new(root().join("rust/fslc/tests/fixtures"));
    let kernel = parse_kernel_source(&source, &resolver).expect("parse");
    let model = build_model(kernel.clone()).expect("model");
    let kernel_json =
        public_kernel_contract(&kernel, &model, "testplan_limit.fsl", "kernel").expect("kernel");
    let mut conformance = fslc_rust::conformance_vectors(&model, 0).expect("conformance");
    let vectors = conformance["vectors"].as_array_mut().expect("vectors");
    let outcome = vectors[2]["outcome"].as_object_mut().expect("outcome");
    outcome.insert("kind".to_owned(), json!("guard_failed"));
    let error = build_test_plan_v1(&kernel_json, &conformance).expect_err("guard_failed");
    assert!(error.contains("guard_failed"), "{error}");
}

/// REJECTING CONTROL. Kernel/conformance spec mismatch must fail closed.
#[test]
fn mismatched_spec_names_fail_closed() {
    use fsl_core::{FsResolver, build_model, parse_kernel_source, public_kernel_contract};
    use fsl_tools::build_test_plan_v1;

    let source = std::fs::read_to_string(root().join(fixture("testplan_limit.fsl")))
        .expect("read limit fixture");
    let resolver = FsResolver::new(root().join("rust/fslc/tests/fixtures"));
    let kernel = parse_kernel_source(&source, &resolver).expect("parse");
    let model = build_model(kernel.clone()).expect("model");
    let mut kernel_json =
        public_kernel_contract(&kernel, &model, "testplan_limit.fsl", "kernel").expect("kernel");
    kernel_json["spec"]["name"] = json!("Other");
    let conformance = fslc_rust::conformance_vectors(&model, 0).expect("conformance");
    let error = build_test_plan_v1(&kernel_json, &conformance).expect_err("spec mismatch");
    assert!(error.contains("spec names must match"), "{error}");
}
