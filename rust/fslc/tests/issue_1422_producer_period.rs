// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

type Files = Vec<(String, Vec<u8>)>;
type Raw = (Vec<u8>, Vec<u8>, Option<i32>, Files);
const MODEL: &str = "// SPDX-License-Identifier: Apache-2.0\nspec K {\n  type Bit = 0..1\n  state { x: Bit }\n  init { x = 0 }\n  action put(v: Bit) { x = v }\n}\n";
const CAUSAL: &str = "// SPDX-License-Identifier: Apache-2.0\ncausal C {\n  uses k from \"model.fsl\"\n  timebase day\n  horizon 2\n  clock c { kernel k 1 tick = 1 day }\n  variable cause { role intervention binds action k.put }\n  variable effect { role outcome observes state k.x cadence 1 }\n  claim Link cause -> effect {\n    version 1\n    status active\n    polarity positive\n    lag 0..1\n    persists 1..1\n    basis hypothesis\n  }\n  expectation E {\n    trigger action k.put\n    response predicate k { x == 0 }\n    within 1\n    clock c\n    derived_from_claim Link\n  }\n}\n";
const MAPPING: &str = "refinement M { impl Raw abs K map x = x action incoming(v) -> put(-v) }\n";
const LOG: &str = "{\"action\":\"incoming\",\"params\":{\"v\":0},\"state\":{\"x\":0,\"raw\":0}}\n";
// Complete existing producer/ledger/file observations, including provenance digests.
const PRESERVED: [&str; 1] = [
    r#"{"producer":{"fsl":"1.0","result":"causal_expectations_observed","schema_version":"causal-observation.v0","formal_result":"not_run","model":"C","claims":[{"id":"claim:Link","formal_assurance":"not_run","causal_support":"untested"}],"expectations":[{"id":"expectation:E","verdict":"pass","assurance":"replay-observed","within_ticks":1,"clock":"c","trigger_kind":"action","derived_from_claim":"claim:Link","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]}],"events_observed":1,"artifacts_generated":1,"artifact_digests":["sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505"],"do_not_assume":["The causal claim is proved","Temporal co-occurrence establishes causality","No unmodeled common cause exists","Expectation violation refutes the causal claim","Unobserved behavior did not occur"]},"ledger":{"fsl":"1.0","result":"causal_ledger","schema_version":"causal-ledger.v0","formal_result":"not_run","model":"C","as_of":null,"claims":[{"id":"claim:Link","version":1,"status":"active","formal_assurance":"not_run","causal_support":"unsupported_by_current_evidence","plans":{"applicable":[],"excluded":[]},"evidence":{"applicable":[],"excluded":[{"evidence_id":"evidence:OBS_E_C","design":"observational","support":"inconclusive","lifecycle_status":"active","is_observation":true,"exclusions":["evidence_scope_unassessable"]}]},"external_refs":[],"attention_reasons":[{"reason":"validation_plan_missing","witness":{"claim_id":"claim:Link"}},{"reason":"current_evidence_missing","witness":{"claim_id":"claim:Link"}}]}],"do_not_assume":["The causal claim is proved or verified","A completed validation plan establishes causality","No unmodeled common cause exists","Absence of challenging evidence means the claim is true","Portfolio readiness is a formal assurance class"]},"files":{"artifact.json":{"schema_version":"fsl-causal-evidence.v0","evidence_id":"OBS_E_C","claims":[{"id":"claim:Link","version":1}],"design":"observational","source_study_id":null,"derived_from":[],"support":"inconclusive","scope":{"population":["all"]},"period":{"start":"2026-01-01","end":"2026-03-31","valid_until":"2026-03-31"},"observation":{"kind":"expectation_replay","expectation_id":"expectation:E","expectation_digest":"sha256:5b520455d99fb142a470ad64203b1e2dfa5ccdd0b7c963e29052697a9d7a45f4","verdict":"pass","assurance":"replay-observed","event_counts":{"observed":1,"unmapped":0,"missing_required":0},"digests":{"model":"sha256:8528e11094bec6b5c4ead23846333aa8ca58353f65a19d3bbac72eced7e64804","log":"sha256:ae8d990abe3df06a022820e444977eb0cd8991aaf20e101b20a503e5e806755f","mapping":"sha256:2613cda6ee0d84dc506381533a7d08388a6d082dade3b0038d995762f4fe27e4","study_protocol":null}},"formal_result":"not_run","artifact_digest":"sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505"},"life.json":{"schema_version":"fsl-causal-evidence-lifecycle.v0","evidence_id":"OBS_E_C","artifact_digest":"sha256:3b644dbb323900025a8e211cb155ca64993cca42ef4785bd0bb3e77abd0b4505","records":[{"sequence":1,"status":"active","superseded_by":null,"recorded_at":"2026-03-31T00:00:00Z","previous_record_digest":null,"record_digest":"sha256:0480aa8d48f20dd4852f9533cc844161331ba72874dae195d8e55b60f7adee43"}]}}}"#,
];

fn fixture() -> PathBuf {
    let root = scratch_identity::unique(std::env::temp_dir().join("fsl-1422-period"));
    std::fs::create_dir(&root).unwrap();
    for (name, text) in [
        ("model.fsl", MODEL),
        ("causal.fsl", CAUSAL),
        ("mapping.fsl", MAPPING),
        ("log.jsonl", LOG),
        ("scope.json", "{\"population\":[\"all\"]}\n"),
    ] {
        std::fs::write(root.join(name), text).unwrap();
    }
    root
}

fn wire(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn files(root: &Path) -> Files {
    ["artifact.json", "life.json"]
        .into_iter()
        .filter_map(|name| {
            std::fs::read(root.join(name))
                .ok()
                .map(|bytes| (name.to_owned(), bytes))
        })
        .collect()
}

fn observe(root: &Path, start: &str, end: &str, ledger: bool, repeat: usize) -> Raw {
    let args = if ledger {
        vec![
            "causal",
            "ledger",
            "causal.fsl",
            "--evidence",
            "artifact.json",
            "--lifecycle",
            "life.json",
        ]
    } else {
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
            start,
            "--period-end",
            end,
            "--out",
            "artifact.json",
            "--lifecycle-out",
            "life.json",
        ]
    };
    let before = files(root);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(root)
        .args(&args)
        .env("FSLC_CACHE", "off")
        .env("RUST_BACKTRACE", "0")
        .output()
        .unwrap();
    let raw = (
        output.stdout,
        output.stderr,
        output.status.code(),
        files(root),
    );
    eprintln!(
        "ISSUE_1422_PERIOD_OBSERVATION {}",
        json!({"start":start,"end":end,"ledger":ledger,
        "repeat":repeat,"argv":args,"files_before":before,"stdout":raw.0,"stderr":raw.1,"exit":raw.2,"files_after":raw.3})
    );
    raw
}

fn preservation() {
    let literal: Value = serde_json::from_str(PRESERVED[0]).unwrap();
    let expected_files = ["artifact.json", "life.json"]
        .map(|name| (name.to_owned(), wire(&literal["files"][name])))
        .to_vec();
    let mut previous = None;
    for repeat in 0..2 {
        let root = fixture();
        let mut observed = Vec::new();
        for (ledger, key) in [(false, "producer"), (true, "ledger")] {
            let raw = observe(&root, "2026-01-01", "2026-03-31", ledger, repeat);
            assert_eq!(
                (&raw.0, raw.1.as_slice(), raw.2, &raw.3),
                (&wire(&literal[key]), &[][..], Some(0), &expected_files)
            );
            observed.push(raw);
        }
        if let Some(previous) = &previous {
            assert_eq!(&observed, previous);
        }
        previous = Some(observed);
        std::fs::remove_dir_all(root).unwrap();
    }
}

const MALFORMED: [&str; 8] = [
    "2026-02-30",
    "2026-13-01",
    "not-a-date",
    "",
    "2026-+1-01",
    "+026-01-01",
    "2026-1-01",
    "2026-01-01T00:00:00Z",
];

/// A malformed `--period-start`/`--period-end` is a usage refusal while
/// options are read (#1422): no output file is created, and existing ones
/// keep their bytes.
fn refusal(option: &str, preexisting: bool) {
    for value in MALFORMED {
        let packet = json!({"fsl":"1.0","result":"error","kind":"usage",
            "message":format!("{option} requires a valid Gregorian date in YYYY-MM-DD format, got '{value}'")});
        let (start, end) = if option == "--period-start" {
            (value, "2026-03-31")
        } else {
            ("2026-01-01", value)
        };
        let mut previous = None;
        for repeat in 0..2 {
            let root = fixture();
            if preexisting {
                std::fs::write(root.join("artifact.json"), b"ordinary existing evidence\n")
                    .unwrap();
                std::fs::write(root.join("life.json"), b"ordinary existing lifecycle\n").unwrap();
            }
            let before = files(&root);
            assert_eq!(before.len(), if preexisting { 2 } else { 0 });
            let raw = observe(&root, start, end, false, repeat);
            assert_eq!(
                (&raw.0, raw.1.as_slice(), raw.2, &raw.3),
                (&wire(&packet), &[][..], Some(2), &before),
                "{option} {value:?}"
            );
            if let Some(previous) = &previous {
                assert_eq!(&raw, previous);
            }
            previous = Some(raw);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn preservation_valid_period_roundtrip() {
    preservation();
}
#[test]
fn malformed_start_creates_neither_output() {
    refusal("--period-start", false);
}
#[test]
fn malformed_start_does_not_replace_output_files() {
    refusal("--period-start", true);
}
#[test]
fn malformed_end_creates_neither_output() {
    refusal("--period-end", false);
}
#[test]
fn malformed_end_does_not_replace_output_files() {
    refusal("--period-end", true);
}
