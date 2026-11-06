// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
use fsl_tools::{artifact_digest, lifecycle_record_digest};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
#[path = "support/scratch_identity.rs"]
mod scratch_identity;
const ROUTES: [&[&str]; 3] = [
    &["analyze", "--projection", "causal_evidence_graph"],
    &["analyze", "--profile", "causal-review"],
    &["ledger"],
];
const MODEL: &str = "examples/causal/subscription_retention.fsl";
const DATE_MESSAGE: &str = "period.valid_until must be a valid Gregorian date in YYYY-MM-DD format";
fn artifact(value: Option<Value>) -> Value {
    let mut artifact: Value = serde_json::from_str(include_str!(
        "../../../examples/causal/evidence/onboarding-2026.causal.json"
    ))
    .unwrap();
    artifact["evidence_id"] = json!("S1");
    artifact["period"]
        .as_object_mut()
        .unwrap()
        .remove("valid_until");
    if let Some(value) = value {
        artifact["period"]["valid_until"] = value;
    }
    artifact["artifact_digest"] = json!(artifact_digest(&artifact));
    artifact
}
fn run(artifact: &Value, route: &[&str], lifecycle: bool, as_of: bool) -> (Value, i32, Vec<u8>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = scratch_identity::unique(
        std::env::temp_dir().join(format!("fsl-1422-{}", std::process::id())),
    );
    std::fs::create_dir(&scratch).unwrap();
    let evidence = scratch.join("S1.json");
    std::fs::write(&evidence, artifact.to_string()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.current_dir(root).args(["causal", route[0], MODEL]);
    command.args(&route[1..]).arg("--evidence").arg(&evidence);
    if as_of {
        command.args(["--as-of", "2026-04-01"]);
    }
    if lifecycle {
        let digest = artifact["artifact_digest"].as_str().unwrap();
        let mut chain: Value = serde_json::from_str(include_str!(
            "../../../examples/causal/evidence/onboarding-2026.lifecycle.json"
        ))
        .unwrap();
        chain["evidence_id"] = json!("S1");
        chain["artifact_digest"] = json!(digest);
        chain["records"][0]["record_digest"] =
            json!(lifecycle_record_digest("S1", digest, &chain["records"][0]));
        let path = scratch.join("S1.lifecycle.json");
        std::fs::write(&path, chain.to_string()).unwrap();
        command.arg("--lifecycle").arg(path);
    }
    let output = command.output().unwrap();
    eprintln!(
        "ISSUE_1422_OBSERVATION {}",
        json!({"route": route, "artifact": artifact,
        "lifecycle": lifecycle, "as_of": as_of, "stdout": output.stdout,
        "stderr": output.stderr, "exit": output.status.code()})
    );
    std::fs::remove_dir_all(scratch).unwrap();
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.status.code().unwrap(),
        output.stderr,
    )
}
fn refusal(diagnostic: &str, message: &str) -> (Value, i32, Vec<u8>) {
    (
        json!({"fsl": "1.0", "result": "error", "kind": "semantics",
        "message": format!("evidence 'S1': {message}"), "diagnostic": diagnostic}),
        2,
        vec![],
    )
}
#[test]
fn invalid_valid_until_refuses_all_consumers_with_full_exact_packets() {
    let mut cases: Vec<(Value, &str)> = [json!(null), json!(false), json!(1), json!([]), json!({})]
        .into_iter()
        .map(|value| (value, "period.valid_until must be a string"))
        .collect();
    cases.extend(
        ["garbage", "+024-02-29", "2023-02-29"]
            .into_iter()
            .map(|date| (json!(date), DATE_MESSAGE)),
    );
    for (value, message) in cases {
        let artifact = artifact(Some(value.clone()));
        for route in ROUTES {
            for as_of in [false, true] {
                assert_eq!(
                    run(&artifact, route, false, as_of),
                    refusal("causal_evidence_schema_mismatch", message),
                    "{value} {route:?} {as_of}"
                );
            }
        }
    }
}
#[test]
fn bad_digest_keeps_priority_over_malformed_valid_until() {
    for value in [json!(null), json!("2023-02-29")] {
        let mut artifact = artifact(Some(value));
        artifact["artifact_digest"] = json!("sha256:incorrect");
        for route in ROUTES {
            assert_eq!(
                run(&artifact, route, false, false),
                refusal(
                    "causal_evidence_digest_mismatch",
                    "declared artifact_digest does not match the canonical payload"
                )
            );
        }
    }
}
#[test]
fn full_packet_comparator_rejects_wrong_kind_field_success_exit_and_stderr() {
    let expected = refusal("causal_evidence_schema_mismatch", DATE_MESSAGE);
    for (key, value) in [
        ("kind", json!("parse")),
        ("diagnostic", json!("causal_evidence_digest_mismatch")),
        ("message", json!("period.start must be a string")),
        ("result", json!("causal_evidence_graph")),
        ("fsl", json!("2.0")),
        ("loc", json!({"line": 1})),
    ] {
        let mut wrong = expected.clone();
        wrong.0[key] = value;
        assert_ne!(wrong, expected, "{key}");
    }
    let mut wrong = expected.clone();
    wrong.0.as_object_mut().unwrap().remove("diagnostic");
    assert_ne!(wrong, expected);
    assert_ne!((expected.0.clone(), 0, vec![]), expected);
    assert_ne!((expected.0.clone(), 2, b"warning".to_vec()), expected);
}
#[test]
fn valid_missing_and_lifecycle_controls_preserve_support() {
    for (date, lifecycle, exclusion) in [
        (Some("0000-02-29"), true, None),
        (Some("2000-02-29"), true, None),
        (Some("2024-02-29"), true, None),
        (Some("9999-12-31"), true, None),
        (None, true, Some("unknown_freshness")),
        (Some("2027-03-31"), false, Some("unknown_lifecycle")),
    ] {
        let artifact = artifact(date.map(|date| json!(date)));
        let supported = exclusion.is_none();
        let expected = ["unsupported_by_current_evidence", "supported"][usize::from(supported)];
        for (index, route) in ROUTES.into_iter().enumerate() {
            let (output, code, stderr) = run(&artifact, route, lifecycle, false);
            assert_eq!(code, 0, "{output}");
            assert!(stderr.is_empty());
            let support = if index == 1 {
                &output["causal_support"]["C_Onboarding_FirstSuccess"]
            } else {
                &output["claims"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|claim| claim["id"] == "claim:C_Onboarding_FirstSuccess")
                    .unwrap()["causal_support"]
            };
            assert_eq!(support, expected);
            if index == 0 {
                assert_eq!(output["edges"][0]["applicable"], supported);
                assert_eq!(
                    output["edges"][0]["exclusions"],
                    exclusion.map_or_else(|| json!([]), |code| json!([code]))
                );
            }
        }
    }
}
#[test]
fn start_end_null_garbage_and_signed_remain_named_soft_exclusions() {
    for (field, value, code) in [
        ("start", json!(null), "period_start_missing"),
        ("end", json!(null), "period_end_missing"),
        ("start", json!("garbage"), "period_date_unparsable"),
        ("end", json!("garbage"), "period_date_unparsable"),
        // A sign is not a digit: `+026` is not read as year 26.
        ("start", json!("+026-01-01"), "period_date_unparsable"),
        ("end", json!("+026-03-31"), "period_date_unparsable"),
        ("start", json!("2026-+1-01"), "period_date_unparsable"),
    ] {
        let mut artifact = artifact(Some(json!("2027-03-31")));
        artifact["period"][field] = value;
        artifact["artifact_digest"] = json!(artifact_digest(&artifact));
        let (output, status, stderr) = run(&artifact, ROUTES[0], true, false);
        assert_eq!(status, 0, "{output}");
        assert!(stderr.is_empty());
        let edge = &output["edges"][0];
        assert_eq!(edge["applicable"], false);
        assert_eq!(edge["exclusions"], json!(["evidence_timing_not_evaluable"]));
        assert_eq!(edge["not_evaluable"][0]["code"], code);
    }
}
