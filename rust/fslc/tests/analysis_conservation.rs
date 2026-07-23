// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native analyze")
}

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse native output: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn ai_review_keeps_the_conservation_envelope_order_and_id() {
    let output = run(&[
        "analyze",
        "tests/fixtures/rust_port/ai_review_conservation.fsl",
        "--profile",
        "ai-review",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = output_json(&output);
    assert_eq!(value["result"], "analyzed");
    assert_eq!(value["analysis"], "structure");
    assert_eq!(value["profile"], "ai-review");
    assert_eq!(value["schema_version"], "analysis-findings.v0");
    let findings = value["findings"].as_array().expect("findings");
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding["finding_id"].as_str().expect("finding ID"))
            .collect::<Vec<_>>(),
        [
            "STRUCT-CONSERVATION-CANDIDATE-0001",
            "STRUCT-UNCONSTRAINED-EFFECT-0001",
            "STRUCT-UNCONSTRAINED-EFFECT-0002",
            "STRUCT-UNCONSTRAINED-EFFECT-0003",
            "STRUCT-UNGUARDED-ACTION-0001",
        ]
    );
    let candidate = findings
        .iter()
        .find(|finding| finding["finding_type"] == "conservation_candidate")
        .expect("conservation candidate");
    assert_eq!(
        candidate["finding_id"],
        "STRUCT-CONSERVATION-CANDIDATE-0001"
    );
    assert_eq!(candidate["formal_status"], "not_a_violation");
    assert_eq!(candidate["witness"]["expression"], "2*reserved + stock");
    assert_eq!(
        candidate["witness"]["weights"],
        json!({"reserved":2,"stock":1})
    );
    assert_eq!(
        candidate["witness"]["action_net_effects"],
        json!([
            {"action":"action:release","deltas":{"reserved":-1,"stock":2},"weighted_sum_delta":0},
            {"action":"action:reserve","deltas":{"reserved":1,"stock":-2},"weighted_sum_delta":0}
        ])
    );
}

#[test]
fn ai_review_invalid_input_and_unsupported_modes_still_exit_two() {
    let unknown_profile = run(&[
        "analyze",
        "tests/fixtures/rust_port/ai_review_conservation.fsl",
        "--profile",
        "unknown-review",
    ]);
    assert_eq!(unknown_profile.status.code(), Some(2));
    let unknown_profile = output_json(&unknown_profile);
    assert_eq!(unknown_profile["result"], "error");
    assert_eq!(unknown_profile["kind"], "semantics");

    let unsupported = run(&[
        "analyze",
        "tests/fixtures/rust_port/ai_review_conservation.fsl",
        "--profile",
        "ai-review",
        "--format",
        "dot",
    ]);
    assert_eq!(unsupported.status.code(), Some(2));
    let unsupported = output_json(&unsupported);
    assert_eq!(unsupported["result"], "error");
    assert_eq!(unsupported["kind"], "semantics");

    let directory = root().join("rust/target/analysis-conservation-contract");
    std::fs::create_dir_all(&directory).expect("create invalid fixture directory");
    let invalid = directory.join("invalid.fsl");
    std::fs::write(&invalid, "spec Broken { state { value: } }")
        .expect("write invalid analysis fixture");
    let invalid = run(&[
        "analyze",
        invalid.to_str().expect("UTF-8 fixture path"),
        "--profile",
        "ai-review",
    ]);
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(output_json(&invalid)["result"], "error");
    std::fs::remove_dir_all(&directory).expect("clear invalid fixture directory");
}

#[test]
fn cli_source_has_no_conservation_classifier_copy() {
    let source = include_str!("../src/main.rs");
    for symbol in [
        "fn counter_delta(",
        "fn scan_counter_statements(",
        "fn integer_gcd(",
        "fn weighted_sum_text(",
        "fn ai_conservation_findings(",
    ] {
        assert!(!source.contains(symbol), "CLI retained {symbol}");
    }
    assert!(source.contains("fsl_tools::conservation_review_findings(model)"));
}
