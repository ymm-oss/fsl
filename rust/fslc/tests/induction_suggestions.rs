// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

const CTI_HINT: &str = "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run";

fn verify(fixture: &str, k: usize) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture,
            "--engine",
            "induction",
            "--depth",
            "8",
            "--k",
            &k.to_string(),
            "--deadlock",
            "ignore",
            "--no-cache",
        ])
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
fn suggests_a_scalar_bound_without_changing_the_verdict() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_scalar.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["invariant"], "Sync");
    assert_eq!(
        output["suggested_invariants"],
        serde_json::json!(["audit >= 0"])
    );
    assert!(output["hint"].as_str().unwrap().starts_with(CTI_HINT));
    assert!(output["hint"].as_str().unwrap().contains("audit >= 0"));
    assert!(output.get("auxiliary_invariant_recommendation").is_none());
    assert!(
        output["cti"]["states"][0]["state"]["audit"]
            .as_i64()
            .unwrap()
            < 0
    );
}

#[test]
fn suggests_a_quantified_bound_for_a_uniform_map() {
    let (output, status) = verify("rust/fslc/tests/fixtures/induction_suggestion_map.fsl", 1);

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(
        output["suggested_invariants"],
        serde_json::json!(["forall k: Case { audit[k] >= 0 }"])
    );
    assert!(output["hint"].as_str().unwrap().contains("audit"));
}

#[test]
fn suggests_a_bound_for_a_domain_counter() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_domain.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(
        output["suggested_invariants"],
        serde_json::json!(["audit >= 0"])
    );
}

#[test]
fn suggests_an_upper_bound_for_a_decreasing_counter() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_decreasing.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(
        output["suggested_invariants"],
        serde_json::json!(["audit <= 0"])
    );
    assert!(
        output["cti"]["states"][0]["state"]["audit"]
            .as_i64()
            .unwrap()
            > 0
    );
}

#[test]
fn omits_suggestions_without_a_violated_initial_bound() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_bounded.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert!(output.get("suggested_invariants").is_none());
    assert_eq!(output["hint"], CTI_HINT);
}

#[test]
fn omits_suggestions_for_nondeterministic_initialization() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_nondeterministic_init.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert!(output.get("suggested_invariants").is_none());
    assert_eq!(output["hint"], CTI_HINT);
}

#[test]
fn omits_map_suggestions_for_nonuniform_initialization() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_nonuniform_map.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert!(output.get("suggested_invariants").is_none());
    assert_eq!(output["hint"], CTI_HINT);
}

#[test]
fn omits_map_suggestions_when_keys_move_in_opposite_directions() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_mixed_map.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert!(output.get("suggested_invariants").is_none());
    assert_eq!(output["hint"], CTI_HINT);
}

#[test]
fn omits_suggestions_for_a_nonmonotone_cti() {
    let (output, status) = verify(
        "rust/fslc/tests/fixtures/induction_suggestion_nonmonotone.fsl",
        2,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["k"], 2);
    assert!(output.get("suggested_invariants").is_none());
    assert_eq!(output["hint"], CTI_HINT);
}

#[test]
fn ranked_leadsto_failures_never_receive_suggestions() {
    let (output, status) = verify(
        "tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl",
        1,
    );

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["violation_kind"], "leadsTo_rank");
    assert!(output.get("suggested_invariants").is_none());
}

#[test]
fn trans_ctis_never_receive_invariant_suggestions() {
    let (output, status) = verify("rust/fslc/tests/fixtures/induction_suggestion_trans.fsl", 1);

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["trans"], "NeverIncrease");
    assert!(output.get("suggested_invariants").is_none());
}
