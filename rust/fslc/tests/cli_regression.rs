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
