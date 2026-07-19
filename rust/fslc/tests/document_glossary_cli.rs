// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the `--glossary` flag on `fslc document generate`
//! and `fslc document check` (issue #330). Parsing/validation and rendering
//! integration are covered at the library level in `rust/fsl-tools/tests/
//! document_glossary.rs`; this file exercises the CLI's own contract:
//! diagnostics, the envelope, digest separation, and `check` parity.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn temp_path(suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let directory = root().join("rust/target/fslc-tests");
    std::fs::create_dir_all(&directory).expect("create repository-local test directory");
    directory.join(format!(
        "fslc-issue-330-{}-{nonce}-{}{suffix}",
        std::process::id(),
        NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed),
    ))
}

fn write_glossary(labels_json: &str) -> PathBuf {
    let path = temp_path(".glossary.json");
    let text =
        format!(r#"{{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{labels_json}}}"#);
    std::fs::write(&path, text).expect("write glossary");
    path
}

const CANCEL_SYSTEM: &str = "examples/pm/cancel_system.fsl";

fn generate(extra_args: &[&str]) -> Output {
    let mut args = vec!["document", "generate", CANCEL_SYSTEM, "--lang", "ja"];
    args.extend_from_slice(extra_args);
    run(&args)
}

fn generate_to_file(glossary: Option<&Path>) -> (PathBuf, Value) {
    let out = temp_path(".md");
    let mut args = vec!["document", "generate", CANCEL_SYSTEM, "--lang", "ja"];
    let glossary_str;
    if let Some(glossary) = glossary {
        glossary_str = glossary.to_str().expect("utf8 path").to_owned();
        args.push("--glossary");
        args.push(&glossary_str);
    }
    let out_str = out.to_str().expect("utf8 path").to_owned();
    args.push("-o");
    args.push(&out_str);
    let output = run(&args);
    assert!(output.status.success(), "{:?}", output.stderr);
    (out, json_stdout(&output))
}

// --- Happy path --------------------------------------------------------------

#[test]
fn generate_with_glossary_applies_labels_and_records_digest() {
    let glossary = write_glossary(
        r#"{"action:submit_cancel":"解約フォームを送信する","state:scr":"契約画面状態","enum:Screen.CancelForm":"解約フォーム"}"#,
    );
    let (out, envelope) = generate_to_file(Some(&glossary));
    assert_eq!(envelope["result"], "generated");
    assert!(
        envelope["glossary"]["digest"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );
    assert_eq!(envelope["glossary"]["labels"], 3);
    assert!(envelope.get("warnings").is_none());

    let markdown = std::fs::read_to_string(&out).expect("read generated markdown");
    assert!(markdown.contains("glossary_digest: sha256:"));
    assert!(markdown.contains("解約フォームを送信する（`submit_cancel`）"));
    assert!(markdown.contains("## 用語集"));
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&glossary);
}

// --- Diagnostics ---------------------------------------------------------------

#[test]
fn generate_glossary_conflict_is_always_an_error() {
    let glossary = write_glossary(r#"{"action:submit_cancel":"A","action:submit_cancel":"B"}"#);
    let glossary_str = glossary.to_str().expect("utf8 path");
    let output = generate(&["--glossary", glossary_str]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-LABEL-CONFLICT");
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn generate_unknown_label_warns_by_default() {
    let glossary = write_glossary(r#"{"action:nonexistent":"存在しない"}"#);
    let glossary_str = glossary.to_str().expect("utf8 path");
    let out = temp_path(".md");
    let out_str = out.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "generate",
        CANCEL_SYSTEM,
        "--lang",
        "ja",
        "--glossary",
        glossary_str,
        "-o",
        out_str,
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let envelope = json_stdout(&output);
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "FSL-DOC-LABEL-UNKNOWN");
    assert_eq!(warnings[0]["target"], "action:nonexistent");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn generate_unknown_label_fails_under_strict_on_a_fixture_with_no_other_coverage_issues() {
    // cancel_system.fsl itself has pre-existing unsupported targets
    // unrelated to the glossary (issue #327), which would otherwise mask
    // FSL-DOC-LABEL-UNKNOWN under --strict; use a minimal fixture with an
    // empty init block (so `init` is not itself an unsupported target) to
    // observe the label diagnostic in isolation.
    let spec = temp_path(".fsl");
    std::fs::write(
        &spec,
        r#"requirements Clean {
  state { counter: Int }
  init { }
  @requirement("REQ-1", "Bump can happen")
  fair action bump() { counter = counter + 1 }
  acceptance AC-1 "bump happens" { bump() expect true }
}
"#,
    )
    .expect("write spec");
    let glossary = write_glossary(r#"{"action:nonexistent":"存在しない"}"#);
    let spec_str = spec
        .strip_prefix(root())
        .expect("repository-local fixture")
        .to_str()
        .expect("utf8 path");
    let glossary_str = glossary.to_str().expect("utf8 path");

    let clean = run(&["document", "generate", spec_str, "--lang", "ja", "--strict"]);
    assert!(
        clean.status.success(),
        "fixture must have zero coverage issues before the glossary is added: {:?}",
        clean.stderr
    );

    let output = run(&[
        "document",
        "generate",
        spec_str,
        "--lang",
        "ja",
        "--strict",
        "--glossary",
        glossary_str,
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-LABEL-UNKNOWN");
    let _ = std::fs::remove_file(&spec);
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn generate_glossary_wrong_schema_exits_two() {
    let glossary = temp_path(".glossary.json");
    std::fs::write(&glossary, r#"{"schema":"wrong","locale":"ja","labels":{}}"#)
        .expect("write glossary");
    let glossary_str = glossary.to_str().expect("utf8 path");
    let output = generate(&["--glossary", glossary_str]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-GLOSSARY-INVALID");
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn generate_glossary_locale_must_match_lang() {
    let glossary = write_glossary(r"{}");
    // write_glossary always writes locale "ja"; request "en" to mismatch.
    let glossary_str = glossary.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "generate",
        CANCEL_SYSTEM,
        "--lang",
        "en",
        "--glossary",
        glossary_str,
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-GLOSSARY-INVALID");
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn generate_missing_glossary_file_exits_two_as_io_error() {
    let output = generate(&["--glossary", "/nonexistent/glossary.json"]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["kind"], "io");
}

// --- Digest separation (the issue's own required test) -----------------------

#[test]
fn changing_the_glossary_changes_artifact_digest_but_not_claim_set_digest() {
    let (out_none, envelope_none) = generate_to_file(None);
    let glossary_a = write_glossary(r#"{"action:submit_cancel":"ラベルA"}"#);
    let (out_a, envelope_a) = generate_to_file(Some(&glossary_a));
    let glossary_b = write_glossary(r#"{"action:submit_cancel":"ラベルB"}"#);
    let (out_b, envelope_b) = generate_to_file(Some(&glossary_b));

    assert_eq!(
        envelope_none["claim_set_digest"],
        envelope_a["claim_set_digest"]
    );
    assert_eq!(
        envelope_a["claim_set_digest"],
        envelope_b["claim_set_digest"]
    );
    assert_eq!(envelope_none["spec_digest"], envelope_a["spec_digest"]);

    assert_ne!(
        envelope_none["artifact_digest"],
        envelope_a["artifact_digest"]
    );
    assert_ne!(envelope_a["artifact_digest"], envelope_b["artifact_digest"]);

    for path in [&out_none, &out_a, &out_b] {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&glossary_a);
    let _ = std::fs::remove_file(&glossary_b);
}

// --- `fslc document check` glossary parity ------------------------------------

#[test]
fn check_is_conformant_when_the_same_glossary_is_supplied() {
    let glossary = write_glossary(r#"{"action:submit_cancel":"解約フォームを送信する"}"#);
    let glossary_str = glossary.to_str().expect("utf8 path");
    let (artifact, _) = generate_to_file(Some(&glossary));
    let artifact_str = artifact.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "check",
        CANCEL_SYSTEM,
        artifact_str,
        "--glossary",
        glossary_str,
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn check_reports_glossary_changed_when_the_flag_is_omitted() {
    let glossary = write_glossary(r#"{"action:submit_cancel":"解約フォームを送信する"}"#);
    let (artifact, _) = generate_to_file(Some(&glossary));
    let artifact_str = artifact.to_str().expect("utf8 path");
    let output = run(&["document", "check", CANCEL_SYSTEM, artifact_str]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "glossary_changed"
                && reason["code"] == "FSL-DOC-GLOSSARY-CHANGED"
                && reason["detail"] == "generated_with_glossary")
    );
    // No claim/residue noise should accompany the one meaningful reason.
    assert!(
        !reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_changed")
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason["kind"] == "edit_outside_slot")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn check_reports_glossary_changed_when_a_spurious_glossary_is_supplied() {
    let (artifact, _) = generate_to_file(None);
    let glossary = write_glossary(r#"{"action:submit_cancel":"解約フォームを送信する"}"#);
    let glossary_str = glossary.to_str().expect("utf8 path");
    let artifact_str = artifact.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "check",
        CANCEL_SYSTEM,
        artifact_str,
        "--glossary",
        glossary_str,
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "glossary_changed"
                && reason["detail"] == "generated_without_glossary")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&glossary);
}

#[test]
fn check_reports_glossary_changed_when_the_glossary_content_differs() {
    let glossary_a = write_glossary(r#"{"action:submit_cancel":"ラベルA"}"#);
    let (artifact, _) = generate_to_file(Some(&glossary_a));
    let glossary_b = write_glossary(r#"{"action:submit_cancel":"ラベルB"}"#);
    let glossary_b_str = glossary_b.to_str().expect("utf8 path");
    let artifact_str = artifact.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "check",
        CANCEL_SYSTEM,
        artifact_str,
        "--glossary",
        glossary_b_str,
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "glossary_changed"
                && reason["detail"] == "glossary_digest_mismatch")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&glossary_a);
    let _ = std::fs::remove_file(&glossary_b);
}

#[test]
fn check_without_glossary_on_a_glossaryless_artifact_is_unaffected() {
    // Regression guard for issue #329's own behavior.
    let (artifact, _) = generate_to_file(None);
    let artifact_str = artifact.to_str().expect("utf8 path");
    let output = run(&["document", "check", CANCEL_SYSTEM, artifact_str]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
}
