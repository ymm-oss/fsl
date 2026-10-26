// SPDX-License-Identifier: Apache-2.0

//! Lemmas use invariant expression types, but type errors reject only the
//! candidate: the target still owns the verdict and exit code (#1448).

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const SOURCE: &str = r"spec L {
  enum Color { Red, Blue }
  type N = 0..3
  state { c: Color, n: N }
  init { c = Red  n = 0 }
  action a() { requires c == Red and n == 0  c = Blue  n = 1 }
  action b() { requires c == Red and n == 1  n = 2 }
  invariant I { n != 2 }
}
";
const INVALID: &str = "c != 7";
const CORRECT: &str = "n != 1 or c != Red";
const ENUM_MISMATCH: &str = "expression of type Int is not assignable to Named(\"Color\")";

fn root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repository root")
        .to_path_buf()
}

struct Fixture {
    source: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let directory = root()
            .join(".claude/work")
            .join(format!("issue-1448-{name}-{}-{nonce}", std::process::id()));
        let cache = directory.join("cache");
        std::fs::create_dir_all(&cache).expect("create isolated worktree-local scratch");
        let source = directory.join("lemma.fsl");
        std::fs::write(&source, SOURCE).expect("write inline issue fixture");
        Self { source, cache }
    }

    fn verify(&self, lemmas: &[&str], cache_enabled: bool) -> (Value, i32) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command
            .current_dir(root())
            .arg("verify")
            .arg(&self.source)
            .args(["--engine", "induction", "--depth", "8"])
            .env_remove("FSLC_UNSTABLE")
            .env_remove("FSLC_CACHE_VERIFY")
            .env("FSLC_CACHE", if cache_enabled { "on" } else { "off" })
            .env("FSLC_CACHE_DIR", &self.cache);
        if !cache_enabled {
            command.arg("--no-cache");
        }
        for lemma in lemmas {
            command.args(["--lemma", lemma]);
        }
        let output = command.output().expect("run native CLI");
        assert!(
            output.stderr.is_empty(),
            "unexpected stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON: {error}; stdout={}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (value, output.status.code().expect("native exit status"))
    }
}

fn issue_cti() -> Value {
    json!({
        "states": [
            {"step": 0, "state": {"c": "Red", "n": 1}},
            {
                "step": 1, "state": {"c": "Red", "n": 2},
                "action": {"name": "b", "params": {}, "loc": {"line": 7, "column": 3}},
                "changes": {"n": {"from": 1, "to": 2}},
            },
        ],
        "violated_at": 1,
    })
}

fn assert_target(output: &Value, status: i32, result: &str, expected_status: i32) {
    assert_eq!(
        (status, &output["result"]),
        (expected_status, &json!(result)),
        "{output}"
    );
    assert_eq!(output["fsl"], "1.0", "{output}");
    assert_eq!(output["spec"], "L", "{output}");
    assert!(output["versions"].is_object(), "{output}");
    assert!(
        output.get("kind").is_none(),
        "candidate error is not a command error: {output}"
    );
}

fn assert_unknown_target(output: &Value, status: i32) {
    assert_target(output, status, "unknown_cti", 1);
    assert_eq!(output["invariant"], "I", "{output}");
    assert_eq!(output["cti"], issue_cti(), "{output}");
    assert_eq!(output["lemma_cti_exclusions"], json!([]), "{output}");
    assert!(
        output.get("auxiliary_invariant_recommendation").is_none(),
        "{output}"
    );
}

fn assert_type_rejection(output: &Value, index: usize, expression: &str, mismatch: &str) {
    let candidate = &output["lemmas"][index];
    let name = format!("AuxiliaryLemma{}", index + 1);
    assert_eq!(candidate["expression"], expression, "{output}");
    assert_eq!(candidate["name"], name, "{output}");
    assert_eq!(
        (
            &candidate["status"],
            &candidate["used"],
            &candidate["proof"]["result"],
            &candidate["proof"]["kind"],
        ),
        (
            &json!("rejected"),
            &json!(false),
            &json!("error"),
            &json!("semantics")
        ),
        "candidate {expression}: {candidate}"
    );
    assert!(
        candidate["proof"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(mismatch)),
        "expected checker assignability text {mismatch:?}; candidate {expression}: {candidate}"
    );
    // An argv type error has no required source-file location. It must not
    // masquerade as either a reachable violation or an induction counterexample.
    for key in ["trace", "cti", "violation_kind", "violated_at_step"] {
        assert!(candidate["proof"].get(key).is_none(), "{candidate}");
    }
    if let Some(checked) = output["invariants_checked"].as_array() {
        assert!(!checked.contains(&json!(name)), "{output}");
    }
}

fn assert_single_type_rejection(expression: &str, mismatch: &str, name: &str) {
    let (output, status) = Fixture::new(name).verify(&[expression], false);
    assert_unknown_target(&output, status);
    assert_eq!(output["lemmas"].as_array().expect("lemma entries").len(), 1);
    assert_type_rejection(&output, 0, expression, mismatch);
}

fn assert_correct_candidate_used(output: &Value, index: usize) {
    let name = format!("AuxiliaryLemma{}", index + 1);
    assert_eq!(
        output["lemmas"][index],
        json!({
            "expression": CORRECT, "name": name, "status": "proved", "used": true,
            "proof": {"result": "proved", "k": 1, "checked_to_depth": 8, "completeness": "unbounded"},
        }),
        "{output}"
    );
    assert_eq!(
        output["lemma_cti_exclusions"],
        json!([{"lemma": CORRECT, "target": "I", "k": 1, "violated_steps": [0], "cti": issue_cti()}]),
        "{output}"
    );
    assert_eq!(
        output["auxiliary_invariant_recommendation"],
        json!({
            "message": "write the used proved lemmas into the specification as auxiliary invariants",
            "declarations": [format!("invariant {name} {{ {CORRECT} }}")],
        }),
        "{output}"
    );
    assert!(
        output["invariants_checked"]
            .as_array()
            .expect("checked invariants")
            .contains(&json!(name)),
        "{output}"
    );
}

#[test]
fn lemma_rejects_enum_integer_inequality_before_proof() {
    assert_single_type_rejection(INVALID, ENUM_MISMATCH, "enum-int-seven");
}

#[test]
fn lemma_rejects_enum_integer_zero_comparison_as_a_type_error() {
    assert_single_type_rejection("c != 0", ENUM_MISMATCH, "enum-int-zero");
}

#[test]
fn lemma_recursively_checks_disjunction_operands() {
    assert_single_type_rejection("c != 0 or n != 1", ENUM_MISMATCH, "disjunction");
}

#[test]
fn lemma_requires_a_boolean_candidate() {
    assert_single_type_rejection(
        "7",
        "expression of type Int is not assignable to Bool",
        "non-boolean",
    );
}

#[test]
fn lemma_preserves_well_typed_proved_and_used_candidate() {
    let (output, status) = Fixture::new("correct").verify(&[CORRECT], false);
    assert_target(&output, status, "proved", 0);
    assert_eq!(output["lemmas"].as_array().expect("lemma entries").len(), 1);
    assert_correct_candidate_used(&output, 0);
}

#[test]
fn lemma_preserves_well_typed_rejected_candidate() {
    let (output, status) = Fixture::new("false").verify(&["c == Red"], false);
    assert_unknown_target(&output, status);
    assert_eq!(
        output["lemmas"],
        json!([{
            "expression": "c == Red", "name": "AuxiliaryLemma1", "status": "rejected", "used": false,
            "proof": {
                "result": "violated", "violation_kind": "invariant", "invariant": "AuxiliaryLemma1",
                "violated_at_step": 1,
                "trace": [
                    {"step": 0, "state": {"c": "Red", "n": 0}},
                    {
                        "step": 1, "state": {"c": "Blue", "n": 1},
                        "action": {"name": "a", "params": {}, "loc": {"line": 6, "column": 3}},
                        "changes": {"c": {"from": "Red", "to": "Blue"}, "n": {"from": 0, "to": 1}},
                    },
                ],
            },
        }]),
        "{output}"
    );
}

#[test]
fn ill_typed_lemma_does_not_block_a_valid_candidate() {
    let fixture = Fixture::new("mixed");
    // Execute both orders even when the first invalid candidate exposes the
    // baseline defect. Candidate names must continue to reflect CLI order.
    let runs = [
        fixture.verify(&[INVALID, CORRECT], false),
        fixture.verify(&[CORRECT, INVALID], false),
    ];
    for (order, (output, status)) in runs.iter().enumerate() {
        eprintln!(
            "mixed order {order}: exit={status}, result={}, lemmas={}",
            output["result"], output["lemmas"]
        );
    }
    for (order, (output, status)) in runs.iter().enumerate() {
        assert_target(output, *status, "proved", 0);
        assert_eq!(output["lemmas"].as_array().expect("lemma entries").len(), 2);
        assert_correct_candidate_used(output, 1 - order);
        assert_type_rejection(output, order, INVALID, ENUM_MISMATCH);
    }
}

#[test]
fn enabled_cache_preserves_lemma_type_rejection_on_repeated_runs() {
    let fixture = Fixture::new("enabled-cache");
    // FSLC_CACHE_DIR is isolated and shared by these two enabled-cache runs.
    // Native lemma runs currently bypass the verdict cache; do not demand a hit.
    // Run twice before asserting, so the red baseline still exercises both.
    let runs = [
        fixture.verify(&[INVALID], true),
        fixture.verify(&[INVALID], true),
    ];
    for (run, (output, status)) in runs.iter().enumerate() {
        eprintln!(
            "cache-enabled run {}: exit={status}, result={}, lemmas={}",
            run + 1,
            output["result"],
            output["lemmas"]
        );
    }
    for (output, status) in &runs {
        assert_unknown_target(output, *status);
        assert_eq!(output["lemmas"].as_array().expect("lemma entries").len(), 1);
        assert_type_rejection(output, 0, INVALID, ENUM_MISMATCH);
    }
}
