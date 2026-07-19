// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

fn verify(fixture: &str, lemmas: &[&str]) -> (serde_json::Value, i32) {
    verify_property(fixture, lemmas, None)
}

fn verify_property(
    fixture: &str,
    lemmas: &[&str],
    property: Option<&str>,
) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let mut arguments = vec![
        "verify",
        fixture,
        "--engine",
        "induction",
        "--depth",
        "8",
        "--deadlock",
        "ignore",
        "--no-cache",
    ];
    for lemma in lemmas {
        arguments.extend(["--lemma", lemma]);
    }
    if let Some(property) = property {
        arguments.extend(["--property", property]);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
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
fn rejects_false_and_non_inductive_candidates_independently() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_sync.fsl";
    let (output, status) = verify(fixture, &["x <= 0", "x != -1"]);

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["lemmas"][0]["status"], "rejected");
    assert_eq!(output["lemmas"][0]["proof"]["result"], "violated");
    assert!(output["lemmas"][0]["proof"]["trace"].is_array());
    assert_eq!(output["lemmas"][1]["status"], "rejected");
    assert_eq!(output["lemmas"][1]["proof"]["result"], "unknown_cti");
    assert!(output["lemmas"][1]["proof"]["cti"]["states"].is_array());
    assert_eq!(output["lemma_cti_exclusions"], serde_json::json!([]));
}

#[test]
fn rejects_mutually_reinforcing_candidates() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_mutual.fsl";
    let (output, status) = verify(fixture, &["not x or y", "not y or x"]);

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["lemmas"][0]["status"], "rejected");
    assert_eq!(output["lemmas"][1]["status"], "rejected");
    assert_eq!(output["lemmas"][0]["proof"]["result"], "unknown_cti");
    assert_eq!(output["lemmas"][1]["proof"]["result"], "unknown_cti");
    assert_eq!(output["lemmas"][0]["used"], false);
    assert_eq!(output["lemmas"][1]["used"], false);
}

#[test]
fn uses_only_the_first_relevant_candidate_in_cli_order() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_sync.fsl";
    let (output, status) = verify(fixture, &["x <= 4", "x == y", "x - y == 0"]);

    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "proved");
    assert!(
        output["lemmas"]
            .as_array()
            .unwrap()
            .iter()
            .all(|lemma| lemma["status"] == "proved")
    );
    assert_eq!(
        output["lemmas"]
            .as_array()
            .expect("lemma reports")
            .iter()
            .map(|lemma| lemma["used"].as_bool().expect("used flag"))
            .collect::<Vec<_>>(),
        vec![false, true, false]
    );
    assert_eq!(output["lemma_cti_exclusions"].as_array().unwrap().len(), 1);
    assert_eq!(output["lemma_cti_exclusions"][0]["lemma"], "x == y");
    assert_eq!(
        output["auxiliary_invariant_recommendation"]["declarations"],
        serde_json::json!(["invariant AuxiliaryLemma2 { x == y }"])
    );
}

#[test]
fn synthetic_names_do_not_collide_with_unselected_source_properties() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_name_collision.fsl";
    let (output, status) = verify_property(fixture, &["x == y"], Some("Target"));

    assert_eq!(status, 0, "{output}");
    assert_eq!(output["lemmas"][0]["name"], "AuxiliaryLemma1Candidate");
    assert_eq!(
        output["auxiliary_invariant_recommendation"]["declarations"],
        serde_json::json!(["invariant AuxiliaryLemma1Candidate { x == y }"])
    );
}

#[test]
fn selected_trans_properties_accept_lemma_candidates() {
    let (output, status) = verify_property(
        "rust/fslc/tests/fixtures/induction_lemma_trans.fsl",
        &["x >= 0"],
        Some("Monotone"),
    );

    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "proved");
    assert_eq!(output["lemmas"][0]["status"], "proved");
}

#[test]
fn iterates_until_distinct_candidates_exclude_successive_ctis() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_multi_cti.fsl";
    let (output, status) = verify(fixture, &["a == b", "c == d"]);

    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "proved");
    assert_eq!(output["lemmas"][0]["used"], true);
    assert_eq!(output["lemmas"][1]["used"], true);
    assert_eq!(output["lemma_cti_exclusions"].as_array().unwrap().len(), 2);
    assert_ne!(
        output["lemma_cti_exclusions"][0]["cti"],
        output["lemma_cti_exclusions"][1]["cti"]
    );
    assert_eq!(
        output["auxiliary_invariant_recommendation"]["declarations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn does_not_recommend_a_used_candidate_until_the_target_is_proved() {
    let fixture = "rust/fslc/tests/fixtures/induction_lemma_multi_cti.fsl";
    let (output, status) = verify(fixture, &["c == d"]);

    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "unknown_cti");
    assert_eq!(output["lemmas"][0]["status"], "proved");
    assert_eq!(output["lemmas"][0]["used"], true);
    assert_eq!(output["lemma_cti_exclusions"].as_array().unwrap().len(), 1);
    assert!(output.get("auxiliary_invariant_recommendation").is_none());
}
