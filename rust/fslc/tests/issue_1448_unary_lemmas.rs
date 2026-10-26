// SPDX-License-Identifier: Apache-2.0

//! Lemma validation depends on #1439's recursive unary operand checks: inferring
//! only the outer Boolean type can still admit an invalid proof premise (#1448).

use std::path::{Path, PathBuf};
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

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn verify_lemma(name: &str, expression: &str) -> (Value, i32) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = root();
    let directory = root.join(".claude/work").join(format!(
        "issue-1448-unary-{name}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create isolated worktree-local fixture");
    let source = directory.join("lemma.fsl");
    std::fs::write(&source, SOURCE).expect("write inline issue fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(root)
        .arg("verify")
        .arg(source)
        .args([
            "--engine",
            "induction",
            "--depth",
            "8",
            "--no-cache",
            "--lemma",
            expression,
        ])
        .env_remove("FSLC_UNSTABLE")
        .env_remove("FSLC_CACHE_VERIFY")
        .env("FSLC_CACHE", "off")
        .env("FSLC_CACHE_DIR", directory.join("cache"))
        .output()
        .expect("run native lemma verification");
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    let status = output.status.code().expect("native exit status");
    eprintln!(
        "lemma={expression}; exit={status}; target={}; candidates={}; recommendation={}",
        value["result"], value["lemmas"], value["auxiliary_invariant_recommendation"]
    );
    (value, status)
}

fn assert_type_rejection(output: &Value, status: i32, expression: &str, mismatch: &str) {
    assert_eq!(output["lemmas"].as_array().expect("lemma entries").len(), 1);
    let candidate = &output["lemmas"][0];
    assert_eq!(candidate["expression"], expression, "{output}");
    assert_eq!(candidate["name"], "AuxiliaryLemma1", "{output}");
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
        "ill-typed unary candidate reached proof or use: {candidate}"
    );
    assert!(
        candidate["proof"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(mismatch)),
        "expected checker text {mismatch:?}; candidate={candidate}"
    );
    for key in ["loc", "trace", "cti", "violation_kind", "violated_at_step"] {
        assert!(candidate["proof"].get(key).is_none(), "{candidate}");
    }
    assert_eq!(
        (status, &output["result"]),
        (1, &json!("unknown_cti")),
        "{output}"
    );
    assert_eq!(output["invariant"], "I", "{output}");
    assert_eq!(output["fsl"], "1.0", "{output}");
    assert_eq!(output["spec"], "L", "{output}");
    assert!(output["versions"].is_object(), "{output}");
    assert!(output.get("kind").is_none(), "{output}");
    assert_eq!(output["lemma_cti_exclusions"], json!([]), "{output}");
    assert!(
        output.get("auxiliary_invariant_recommendation").is_none(),
        "{output}"
    );
    if let Some(checked) = output["invariants_checked"].as_array() {
        assert!(!checked.contains(&json!("AuxiliaryLemma1")), "{output}");
    }
}

#[test]
fn lemma_not_rejects_a_non_boolean_operand() {
    let expression = "not 1";
    let (output, status) = verify_lemma("not", expression);
    assert_type_rejection(
        &output,
        status,
        expression,
        "expression of type Int is not assignable to Bool",
    );
}

#[test]
fn lemma_negation_rejects_enum_operands_before_use_or_recommendation() {
    let expression = "n != 1 or (c != Red and -c == -c)";
    let (output, status) = verify_lemma("neg", expression);
    assert_type_rejection(
        &output,
        status,
        expression,
        "expression of type Named(\"Color\") is not assignable to Int",
    );
}
