// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_tools::{EvidenceError, artifact_digest, parse_artifact};
use serde_json::{Value, json};

fn artifact(value: Option<Value>) -> Value {
    let mut artifact: Value = serde_json::from_str(include_str!(
        "../../../examples/causal/evidence/onboarding-2026.causal.json"
    ))
    .expect("existing evidence fixture");
    artifact["evidence_id"] = json!("S1");
    artifact["period"]
        .as_object_mut()
        .unwrap()
        .remove("valid_until");
    if let Some(value) = value {
        artifact["period"]["valid_until"] = value;
    }
    restamp(&mut artifact);
    artifact
}

fn restamp(artifact: &mut Value) {
    artifact["artifact_digest"] = json!(artifact_digest(artifact));
}

fn schema_error(message: &str) -> EvidenceError {
    EvidenceError {
        kind: "causal_evidence_schema_mismatch",
        message: format!("evidence 'S1': period.valid_until {message}"),
    }
}

#[test]
fn present_nonstring_is_a_schema_error_including_null() {
    for value in [json!(null), json!(true), json!(42), json!([]), json!({})] {
        assert_eq!(
            parse_artifact(&artifact(Some(value.clone()))).unwrap_err(),
            schema_error("must be a string"),
            "valid_until={value}"
        );
    }
}

#[test]
fn date_requires_exact_ascii_shape_and_existing_gregorian_day() {
    for date in [
        "garbage",
        "",
        "2024-2-29",
        "024-02-29",
        "02024-02-29",
        "2024-02-029",
        "+024-02-29",
        "-024-02-29",
        "2024-+2-29",
        "2024-02-+9",
        "２０２４-02-29",
        "2024-02-29T00:00:00Z",
        "2024-02-29 ",
        "1900-02-29",
        "2023-02-29",
        "2024-02-30",
        "2026-04-31",
        "2024-00-01",
        "2024-13-01",
        "2024-01-00",
        "2024-01-32",
    ] {
        assert_eq!(
            parse_artifact(&artifact(Some(json!(date)))).unwrap_err(),
            schema_error("must be a valid Gregorian date in YYYY-MM-DD format"),
            "valid_until={date}"
        );
    }
}

#[test]
fn valid_leaps_boundaries_and_omission_remain_accepted() {
    for date in [
        "0000-01-01",
        "0000-02-29",
        "2000-02-29",
        "2024-02-29",
        "2024-04-30",
        "2024-12-31",
        "9999-12-31",
    ] {
        let parsed = parse_artifact(&artifact(Some(json!(date)))).expect(date);
        assert_eq!(parsed.valid_until.as_deref(), Some(date));
    }
    assert_eq!(parse_artifact(&artifact(None)).unwrap().valid_until, None);
}

#[test]
fn malformed_valid_until_does_not_preempt_existing_digest_error() {
    for value in [json!(null), json!("2023-02-29")] {
        let mut artifact = artifact(Some(value));
        artifact["artifact_digest"] = json!("sha256:incorrect");
        assert_eq!(
            parse_artifact(&artifact).unwrap_err(),
            EvidenceError {
                kind: "causal_evidence_digest_mismatch",
                message:
                    "evidence 'S1': declared artifact_digest does not match the canonical payload"
                        .to_owned(),
            }
        );
    }
}

#[test]
fn start_and_end_keep_soft_null_and_garbage_parsing() {
    for field in ["start", "end"] {
        for value in [json!(null), json!("garbage")] {
            let mut artifact = artifact(Some(json!("2027-03-31")));
            artifact["period"][field] = value.clone();
            restamp(&mut artifact);
            let parsed = parse_artifact(&artifact).expect("window conversion remains soft");
            let actual = if field == "start" {
                parsed.period_start
            } else {
                parsed.period_end
            };
            assert_eq!(actual.as_deref(), value.as_str());
        }
    }
}

#[test]
fn prior_required_field_and_observation_errors_keep_precedence() {
    for (field, value, message) in [
        (
            "formal_result",
            json!(null),
            "missing or non-string required field 'formal_result'",
        ),
        (
            "observation",
            json!(false),
            "observation must be an object or null",
        ),
    ] {
        let mut artifact = artifact(Some(json!("2023-02-29")));
        artifact[field] = value;
        restamp(&mut artifact);
        assert_eq!(
            parse_artifact(&artifact).unwrap_err(),
            EvidenceError {
                kind: "causal_evidence_schema_mismatch",
                message: format!("evidence 'S1': {message}"),
            }
        );
    }
}
