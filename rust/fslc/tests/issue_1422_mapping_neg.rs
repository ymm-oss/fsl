// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

const MODEL: &str = "// SPDX-License-Identifier: Apache-2.0\nspec K {\n  type Bit = 0..1\n  state { x: Bit }\n  init { x = 0 }\n  action put(v: Bit) { x = v }\n}\n";
const CAUSAL: &str = "// SPDX-License-Identifier: Apache-2.0\ncausal C {\n  uses k from \"model.fsl\"\n  timebase day\n  horizon 2\n  clock c { kernel k 1 tick = 1 day }\n  variable cause { role intervention binds action k.put }\n  variable effect { role outcome observes state k.x cadence 1 }\n  claim Link cause -> effect {\n    version 1\n    status active\n    polarity positive\n    lag 0..1\n    persists 1..1\n    basis hypothesis\n  }\n  expectation E {\n    trigger action k.put\n    response predicate k { x == 0 }\n    within 1\n    clock c\n    derived_from_claim Link\n  }\n}\n";
const NOTE: &str = "leadsTo properties are not checked by replay (finite logs only)";
const DONT: [&str; 5] = [
    "The causal claim is proved",
    "Temporal co-occurrence establishes causality",
    "No unmodeled common cause exists",
    "Expectation violation refutes the causal claim",
    "Unobserved behavior did not occur",
];
// Full baseline success envelopes and artifacts, including every provenance digest.
const POSITIVES: [&str; 2] = [
    r#"{"packet":{"fsl":"1.0","result":"causal_expectations_observed","schema_version":"causal-observation.v0","formal_result":"not_run","model":"C","claims":[{"id":"claim:Link","formal_assurance":"not_run","causal_support":"untested"}],"expectations":[{"id":"expectation:E","verdict":"pass","assurance":"replay-observed","within_ticks":1,"clock":"c","trigger_kind":"action","derived_from_claim":"claim:Link","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]}],"events_observed":1,"artifacts_generated":1,"artifact_digests":["sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505"],"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]},"artifacts":{"artifact.json":{"schema_version":"fsl-causal-evidence.v0","evidence_id":"OBS_E_C","claims":[{"id":"claim:Link","version":1}],"design":"observational","source_study_id":null,"derived_from":[],"support":"inconclusive","scope":{"population":["all"]},"period":{"start":"2026-01-01","end":"2026-03-31","valid_until":"2026-03-31"},"observation":{"kind":"expectation_replay","expectation_id":"expectation:E","expectation_digest":"sha256:5b520455d99fb142a470ad64203b1e2dfa5ccdd0b7c963e29052697a9d7a45f4","verdict":"pass","assurance":"replay-observed","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"digests":{"model":"sha256:8528e11094bec6b5c4ead23846333aa8ca58353f65a19d3bbac72eced7e64804","log":"sha256:ae8d990abe3df06a022820e444977eb0cd8991aaf20e101b20a503e5e806755f","mapping":"sha256:2613cda6ee0d84dc506381533a7d08388a6d082dade3b0038d995762f4fe27e4","study_protocol":null}},"formal_result":"not_run","artifact_digest":"sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505"},"life.json":{"schema_version":"fsl-causal-evidence-lifecycle.v0","evidence_id":"OBS_E_C","artifact_digest":"sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505","records":[{"sequence":1,"status":"active","superseded_by":null,"recorded_at":"2026-03-31T00:00:00Z","previous_record_digest":null,"record_digest":"sha256:0480aa8d48f20dd4852f9533cc844161331ba72874dae195d8e55b60f7adee43"}]}}}"#,
    r#"{"packet":{"fsl":"1.0","result":"causal_expectations_observed","schema_version":"causal-observation.v0","formal_result":"not_run","model":"C","claims":[{"id":"claim:Link","formal_assurance":"not_run","causal_support":"untested"}],"expectations":[{"id":"expectation:E","verdict":"pass","assurance":"replay-observed","within_ticks":1,"clock":"c","trigger_kind":"action","derived_from_claim":"claim:Link","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]}],"events_observed":1,"artifacts_generated":1,"artifact_digests":["sha256:157c98f387cb292b4aa81e3a36e0c9750d44265ef9764c58d5ba333a74712fb6"],"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]},"artifacts":{"artifact.json":{"schema_version":"fsl-causal-evidence.v0","evidence_id":"OBS_E_C","claims":[{"id":"claim:Link","version":1}],"design":"observational","source_study_id":null,"derived_from":[],"support":"inconclusive","scope":{"population":["all"]},"period":{"start":"2026-01-01","end":"2026-03-31","valid_until":"2026-03-31"},"observation":{"kind":"expectation_replay","expectation_id":"expectation:E","expectation_digest":"sha256:5b520455d99fb142a470ad64203b1e2dfa5ccdd0b7c963e29052697a9d7a45f4","verdict":"pass","assurance":"replay-observed","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"digests":{"model":"sha256:8528e11094bec6b5c4ead23846333aa8ca58353f65a19d3bbac72eced7e64804","log":"sha256:ae8d990abe3df06a022820e444977eb0cd8991aaf20e101b20a503e5e806755f","mapping":"sha256:2ca562c295d0a8eb6953193777c9ebfb4773392a51b151a3da37f869ee5cb8e9","study_protocol":null}},"formal_result":"not_run","artifact_digest":"sha256:157c98f387cb292b4aa81e3a36e0c9750d44265ef9764c58d5ba333a74712fb6"},"life.json":{"schema_version":"fsl-causal-evidence-lifecycle.v0","evidence_id":"OBS_E_C","artifact_digest":"sha256:157c98f387cb292b4aa81e3a36e0c9750d44265ef9764c58d5ba333a74712fb6","records":[{"sequence":1,"status":"active","superseded_by":null,"recorded_at":"2026-03-31T00:00:00Z","previous_record_digest":null,"record_digest":"sha256:de1bf406966399a3fe8b420b85527ae8fafbab631f3e8111ac5b12094411145e"}]}}}"#,
];

fn fixture(state: bool, operation: &str) -> PathBuf {
    let root = scratch_identity::unique(std::env::temp_dir().join("fsl-1422-mapping"));
    std::fs::create_dir(&root).unwrap();
    for (name, text) in [
        ("model.fsl", MODEL),
        ("causal.fsl", CAUSAL),
        ("scope.json", "{\"population\":[\"all\"]}\n"),
    ] {
        std::fs::write(root.join(name), text).unwrap();
    }
    let expression = match (state, operation) {
        (false, "div0") => "v / 0",
        (true, "div0") => "raw / 0",
        (false, _) => "-v",
        (true, _) => "-raw",
    };
    let (mapped_state, parameter) = if state {
        (expression, "v")
    } else {
        ("x", expression)
    };
    std::fs::write(root.join("mapping.fsl"), format!("refinement M {{ impl Raw abs K map x = {mapped_state} action incoming(v) -> put({parameter}) }}\n")).unwrap();
    let (v, raw) = if operation == "min" {
        if state { (0, i64::MIN) } else { (i64::MIN, 0) }
    } else {
        (0, 0)
    };
    std::fs::write(root.join("log.jsonl"), format!("{{\"action\":\"incoming\",\"params\":{{\"v\":{v}}},\"state\":{{\"x\":0,\"raw\":{raw}}}}}\n")).unwrap();
    root
}

fn wire(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

type Artifacts = Vec<(String, Vec<u8>)>;

fn expected(causal: bool, state: bool, success: bool) -> (Value, Artifacts, i32) {
    if success && causal {
        let literal: Value = serde_json::from_str(POSITIVES[usize::from(state)]).unwrap();
        let artifacts = ["artifact.json", "life.json"]
            .map(|name| (name.to_owned(), wire(&literal["artifacts"][name])))
            .to_vec();
        return (literal["packet"].clone(), artifacts, 0);
    }
    let packet = if success {
        json!({"fsl":"1.0","result":"conformant","spec":"K","mapping":"M",
            "source":"jsonl_mapping","steps_checked":1,"final_state":{"x":0},"note":NOTE})
    } else if causal {
        json!({"fsl":"1.0","result":"error","kind":"observation_replay_failed",
            "message":"log record 0: invalid mapping arithmetic","failed_at_record":0,"do_not_assume":DONT})
    } else {
        json!({"fsl":"1.0","result":"nonconformant","spec":"K","mapping":"M","source":"jsonl_mapping",
            "failed_at_event":0,"failed_at_record":0,"log_line":1,
            "violation":{"kind":"log_mapping","message":"invalid mapping arithmetic","loc":null},
            "state_before":{"x":0},"note":NOTE})
    };
    let code = if success { 0 } else { 1 + i32::from(causal) };
    (packet, vec![], code)
}

fn observe(root: &Path, causal: bool, state: bool, operation: &str) {
    let args = if causal {
        vec![
            "causal",
            "observe-expectations",
            "causal.fsl",
            "--from-log",
            "log.jsonl",
            "--mapping",
            "mapping.fsl",
            "--scope",
            "scope.json",
            "--period-start",
            "2026-01-01",
            "--period-end",
            "2026-03-31",
            "--out",
            "artifact.json",
            "--lifecycle-out",
            "life.json",
        ]
    } else {
        vec![
            "replay",
            "model.fsl",
            "--from-log",
            "log.jsonl",
            "--mapping",
            "mapping.fsl",
        ]
    };
    let (packet, expected_artifacts, code) = expected(causal, state, operation == "zero");
    let mut previous = None;
    for repeat in 0..2 {
        for name in ["artifact.json", "life.json"] {
            let path = root.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .current_dir(root)
            .args(&args)
            .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
            .env("FSLC_CACHE", "off")
            .env("RUST_BACKTRACE", "0")
            .output()
            .unwrap();
        let artifacts = ["artifact.json", "life.json"]
            .into_iter()
            .filter_map(|name| {
                std::fs::read(root.join(name))
                    .ok()
                    .map(|bytes| (name.to_owned(), bytes))
            })
            .collect::<Vec<_>>();
        let raw = (
            output.stdout,
            output.stderr,
            output.status.code(),
            artifacts,
        );
        eprintln!(
            "ISSUE_1422_MAPPING_OBSERVATION {}",
            json!({"causal":causal,"state":state,"operation":operation,
            "repeat":repeat,"argv":args,"stdout":raw.0,"stderr":raw.1,"exit":raw.2,"artifacts":raw.3})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&raw.0).ok(),
            Some(packet.clone())
        );
        assert_eq!(
            (&raw.0, raw.1.as_slice(), raw.2, &raw.3),
            (&wire(&packet), &[][..], Some(code), &expected_artifacts)
        );
        if let Some(previous) = &previous {
            assert_eq!(&raw, previous);
        }
        previous = Some(raw);
    }
}

fn case(causal: bool, state: bool, operation: &str) {
    let root = fixture(state, operation);
    observe(&root, causal, state, operation);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn replay_parameter_minimum_uses_the_arithmetic_failure_wrapper() {
    case(false, false, "min");
}
#[test]
fn replay_scalar_state_minimum_uses_the_arithmetic_failure_wrapper() {
    case(false, true, "min");
}
#[test]
fn causal_parameter_minimum_uses_the_arithmetic_failure_wrapper() {
    case(true, false, "min");
}
#[test]
fn causal_scalar_state_minimum_uses_the_arithmetic_failure_wrapper() {
    case(true, true, "min");
}
#[test]
fn existing_arithmetic_failures_and_success_artifacts_are_preserved() {
    for causal in [false, true] {
        for state in [false, true] {
            for operation in ["div0", "zero"] {
                case(causal, state, operation);
            }
        }
    }
}
