// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn replay(trace: &str) -> Output {
    replay_path(&fixture(trace))
}

fn replay_path(trace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "replay",
            fixture("replay_trace.fsl").to_str().expect("spec path"),
            "--trace",
            trace.to_str().expect("trace path"),
        ])
        .output()
        .expect("run replay")
}

fn replay_value(value: &Value) -> Output {
    let path = std::env::temp_dir().join(format!(
        "fsl-replay-trace-{}-{}.json",
        std::process::id(),
        TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::write(&path, serde_json::to_vec(value).expect("serialize trace"))
        .expect("write trace");
    let output = replay_path(&path);
    std::fs::remove_file(path).expect("remove trace");
    output
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

#[test]
fn public_v1_replays_complete_tick_ordered_observations() {
    let output = replay("replay_trace.valid.v1.json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = json(&output);
    assert_eq!(value["result"], "conformant");
    assert_eq!(value["steps_checked"], 2);
    assert_eq!(value["trace_schema_version"], "1.0.0");
    assert_eq!(value["kernel_schema_version"], "1.0.0");
    assert_eq!(value["final_state"]["phase"], "Done");
}

#[test]
fn well_typed_observed_state_divergence_is_nonconformant_with_leaf_evidence() {
    let output = replay("replay_trace.state-mismatch.v1.json");
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_eq!(value["result"], "nonconformant");
    assert_eq!(value["failed_at_event"], 0);
    assert_eq!(value["violation"]["kind"], "state_mismatch");
    assert_eq!(value["violation"]["tick"], 1);
    assert_eq!(value["violation"]["mismatches"][0]["path"], "count.0");
    assert_eq!(value["state_before"]["selected"], Value::Null);
}

#[test]
fn malformed_tick_is_an_input_error_not_a_partial_replay() {
    let output = replay("replay_trace.bad-tick.v1.json");
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "io");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("expected 1"))
    );
}

#[test]
fn versioned_parameter_type_errors_are_input_errors_but_rejected_calls_are_nonconformant() {
    let valid: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("replay_trace.valid.v1.json")).expect("fixture"),
    )
    .expect("trace JSON");

    let mut ill_typed = valid.clone();
    ill_typed["events"][0]["params"]["i"] = json!("0");
    let output = replay_value(&ill_typed);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["kind"], "io");

    let mut out_of_domain = valid.clone();
    out_of_domain["events"][0]["params"]["i"] = json!(2);
    let output = replay_value(&out_of_domain);
    assert_eq!(output.status.code(), Some(2));

    let mut rejected = valid.clone();
    rejected["events"]
        .as_array_mut()
        .expect("events")
        .truncate(1);
    rejected["events"][0]["action"] = json!("finish");
    rejected["events"][0]["params"] = json!({});
    rejected["events"][0]["state"] = rejected["initial"].clone();
    let output = replay_value(&rejected);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["violation"]["kind"], "requires_failed");

    let mut malformed_after_rejection = valid;
    malformed_after_rejection["events"][0]["action"] = json!("finish");
    malformed_after_rejection["events"][0]["params"] = json!({});
    malformed_after_rejection["events"][1]["params"] = json!({"unexpected":true});
    let output = replay_value(&malformed_after_rejection);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn typed_initial_divergence_is_nonconformant_but_incomplete_state_is_an_input_error() {
    let mut initial_mismatch: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("replay_trace.valid.v1.json")).expect("fixture"),
    )
    .expect("trace JSON");
    initial_mismatch["initial"]["phase"] = json!("Done");
    let output = replay_value(&initial_mismatch);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_eq!(value["failed_at_event"], Value::Null);
    assert_eq!(value["violation"]["kind"], "initial_state_mismatch");
    assert_eq!(value["violation"]["tick"], 0);

    let mut incomplete = initial_mismatch;
    incomplete["initial"]
        .as_object_mut()
        .expect("initial")
        .remove("selected");
    let output = replay_value(&incomplete);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json(&output)["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing state variable 'selected'"))
    );
}

#[test]
fn legacy_action_only_arrays_keep_the_pre_v1_result_shape() {
    let output = replay_value(&json!([
        {"action":"select","params":{"i":0}},
        {"action":"finish","params":{}}
    ]));
    assert!(output.status.success());
    let value = json(&output);
    assert_eq!(value["result"], "conformant");
    assert_eq!(value["steps_checked"], 2);
    assert!(value.get("trace_schema_version").is_none());
    assert!(value.get("kernel_schema_version").is_none());
}

#[test]
fn release_bundles_include_the_schema_spec_and_positive_and_negative_goldens() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let workflow = std::fs::read_to_string(workspace.join(".github/workflows/release.yml"))
        .expect("release workflow");
    for artifact in [
        "replay-trace.v1.schema.json",
        "DESIGN-replay-trace.md",
        "replay_trace.fsl",
        "replay_trace.valid.v1.json",
        "replay_trace.state-mismatch.v1.json",
        "replay_trace.bad-tick.v1.json",
    ] {
        assert_eq!(workflow.matches(artifact).count(), 2, "{artifact}");
    }
}
