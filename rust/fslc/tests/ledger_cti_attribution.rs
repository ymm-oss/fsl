// SPDX-License-Identifier: Apache-2.0

#[path = "support/ledger_attribution.rs"]
mod cases;
#[path = "support/lane_b_cli.rs"]
mod cli;
#[path = "support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;
#[path = "support/ledger_r3.rs"]
mod support;

#[test]
fn synthetic_action_cti_shape_is_a_real_producer_projection() {
    let path = support::directory("producer-shape").join("spec.fsl");
    std::fs::write(
        &path,
        r#"spec EnsuresHole {
        state { x: 0..10 } init { x = 0 }
        @requirement("REQ-ACTION", "action obligation")
        action step() { requires x < 10 x = x + 1 ensures x != 6 }
        invariant Range { 0 <= x and x <= 10 }
    }"#,
    )
    .unwrap();
    let (verified, status) = cli::run_json(
        "verify",
        path.to_str().unwrap(),
        3,
        &["--engine", "induction", "--deadlock", "ignore"],
    );
    assert_eq!(status, 1, "{verified}");
    let synthetic = cases::cti("ensures", "step", Some("step"));
    // All attribution-bearing synthetic keys must exist in the actual producer.
    for key in [
        "result",
        "violation_kind",
        "invariant",
        "trans",
        "leadsTo",
        "last_action",
        "requirement",
    ] {
        if let Some(expected) = synthetic.get(key) {
            let actual = verified
                .get(key)
                .unwrap_or_else(|| panic!("synthetic key {key} absent from producer: {verified}"));
            if key == "last_action" {
                assert_eq!(actual["name"], expected["name"], "{verified}");
            } else {
                assert_eq!(actual, expected, "producer {key}: {verified}");
            }
        } else {
            assert!(
                verified.get(key).is_none(),
                "unexpected attribution key {key}: {verified}"
            );
        }
    }
}

fn assert_attributed(fixture: &str, declaration: &str, name: &str, annotated: bool) {
    let source = if annotated {
        support::annotated_fixture(fixture, declaration)
    } else {
        std::fs::read_to_string(cli::repository_root().join(fixture)).unwrap()
    };
    let path = support::directory("attribution").join("spec.fsl");
    std::fs::write(&path, source).unwrap();
    let path = path.to_str().unwrap();
    let extra = ["--engine", "induction", "--deadlock", "ignore"];
    let (verified, status) = cli::run_json("verify", path, 3, &extra);
    assert_eq!(status, 1, "{verified}");
    assert_eq!(verified["result"], "unknown_cti", "{verified}");
    assert_eq!(verified["invariant"], name, "{verified}");
    let (body, status) = cli::run("ledger", path, 3, &extra);
    assert_eq!(status, 1, "{body}");
    let id = if annotated {
        "REQ-R3"
    } else {
        "（仕様全体）"
    };
    let detail = findings::assert_issue(&body, id, &[name]);
    findings::assert_uncertain(detail);
    if annotated {
        assert!(
            !findings::section(&body, "リスク一覧（要件ID別）").contains("| （仕様全体） |"),
            "{body}"
        );
    }
}

#[test]
fn ranked_leadsto_cti_belongs_to_its_requirement() {
    assert_attributed(
        "tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl",
        "leadsTo ReachFive",
        "ReachFive",
        true,
    );
}

#[test]
fn invariant_cti_keeps_its_requirement() {
    assert_attributed(
        "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        "invariant Sync",
        "Sync",
        true,
    );
}

#[test]
fn unannotated_cti_stays_spec_level() {
    assert_attributed(
        "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        "invariant Sync",
        "Sync",
        false,
    );
}

#[test]
fn transition_cti_belongs_to_its_requirement() {
    assert_attributed(
        "rust/fslc/tests/fixtures/induction_suggestion_trans.fsl",
        "trans NeverIncrease",
        "NeverIncrease",
        true,
    );
}
