// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the `--evidence` flag on `fslc document generate`
//! and `fslc document check` (issue #332). Classification and rendering
//! integration are covered at the library level in `rust/fsl-tools/tests/
//! document_evidence.rs`; this file exercises the CLI's own contract:
//! diagnostics, the envelope, digest separation, and `check` parity —
//! including the payoff of the `skip_claim_text`/`skip_residue_text` split
//! in `fsl_tools::document_check`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

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
        "fslc-issue-332-{}-{nonce}-{}{suffix}",
        std::process::id(),
        NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed),
    ))
}

fn write_evidence(json: &str) -> PathBuf {
    let path = temp_path(".evidence.json");
    std::fs::write(&path, json).expect("write evidence");
    path
}

const CANCEL_SYSTEM: &str = "examples/pm/cancel_system.fsl";

fn assert_base_golden(actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1475_golden/document-ja-evidence.json");
    let expected: Value = serde_json::from_slice(&std::fs::read(path).expect("read base golden"))
        .expect("base golden JSON");
    assert_eq!(expected["base"], "c25f8653b9c83a247321f375f98cd59f430b6326");
    assert_eq!(actual, &expected["observed"], "full base document output");
}

struct NormalEvidenceFixture(PathBuf);

impl Drop for NormalEvidenceFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove normal evidence fixture");
    }
}

impl NormalEvidenceFixture {
    fn new() -> Self {
        let directory = temp_path(".normal-evidence");
        std::fs::create_dir_all(directory.join("examples/pm")).expect("create fixture directory");
        for name in ["cancel_system.fsl", "cancel_flow.fsl"] {
            let relative = Path::new("examples/pm").join(name);
            let source =
                std::fs::read_to_string(root().join(&relative)).expect("read original source");
            std::fs::write(directory.join(relative), source.replace("\r\n", "\n"))
                .expect("copy source bytes");
        }
        std::fs::write(
            directory.join("evidence.json"),
            r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded","result":"verified"}"#,
        )
        .expect("write fixed evidence bytes");
        Self(directory)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("run native fslc")
    }
}

// #1475: valid artifact, hostile *fresh-side* evidence. An artifact-only
// malformed marker exercises drift/1, not this renderer/checker panic.
#[test]
fn issue_1475_marker_in_fresh_evidence_is_a_document_refusal() {
    let evidence = write_evidence(
        r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded","result":"verified"}"#,
    );
    let (artifact, _) = generate_to_file(&[&evidence]);
    let before = std::fs::read(&artifact).expect("normal artifact bytes");
    std::fs::write(&evidence, r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded","result":"verified\n<!-- fsl:bogus -->\n"}"#).expect("write hostile evidence");
    let output = check(CANCEL_SYSTEM, &artifact, &[&evidence]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        json_stdout(&output),
        serde_json::json!({
            "fsl": "1.0", "result": "error", "kind": "document",
            "message": "malformed generated-block marker: \"<!-- fsl:bogus -->\""
        })
    );
    assert_eq!(
        std::fs::read(&artifact).expect("artifact not overwritten"),
        before
    );
}

#[test]
fn issue_1475_normal_evidence_bytes_and_full_check_envelope_are_stable() {
    // Stable relative labels prevent ambient scratch paths entering Markdown,
    // evidence digests or JSON. No output field/digest is excluded.
    let fixture = NormalEvidenceFixture::new();
    let generate_args = [
        "document",
        "generate",
        CANCEL_SYSTEM,
        "--lang",
        "ja",
        "--evidence",
        "evidence.json",
        "-o",
        "document.md",
    ];
    let generation = fixture.run(&generate_args);
    assert_eq!(generation.status.code(), Some(0));
    assert!(generation.stderr.is_empty());
    let generated = json_stdout(&generation);
    let artifact = fixture.0.join("document.md");
    let before = std::fs::read(&artifact).expect("normal artifact bytes");
    let check_args = [
        "document",
        "check",
        CANCEL_SYSTEM,
        "document.md",
        "--evidence",
        "evidence.json",
    ];
    let output = fixture.run(&check_args);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let value = json_stdout(&output);
    assert_eq!(value["result"], "document_conformant");
    println!(
        "DOCUMENT NORMAL GENERATE {generated}\nDOCUMENT NORMAL CHECK {value}\n{}",
        String::from_utf8_lossy(&before)
    );
    assert_eq!(value["spec_digest"], generated["spec_digest"]);
    assert_eq!(value["claim_set_digest"], generated["claim_set_digest"]);
    assert_base_golden(&serde_json::json!({
        "generate": generated, "check": value,
        "markdown_sha256": format!("{:x}", Sha256::digest(&before)),
    }));
    assert_eq!(
        value,
        json_stdout(&fixture.run(&check_args)),
        "same-binary repeat determinism"
    );
    let regenerated = fixture.run(&generate_args);
    assert_eq!(regenerated.status.code(), Some(0));
    assert_eq!(generated, json_stdout(&regenerated));
    assert_eq!(std::fs::read(&artifact).expect("regenerated bytes"), before);
}

fn generate(extra_args: &[&str]) -> Output {
    let mut args = vec!["document", "generate", CANCEL_SYSTEM, "--lang", "ja"];
    args.extend_from_slice(extra_args);
    run(&args)
}

fn generate_to_file(evidence: &[&Path]) -> (PathBuf, Value) {
    let out = temp_path(".md");
    let mut args = vec!["document", "generate", CANCEL_SYSTEM, "--lang", "ja"];
    let evidence_strs: Vec<String> = evidence
        .iter()
        .map(|path| path.to_str().expect("utf8 path").to_owned())
        .collect();
    for evidence_str in &evidence_strs {
        args.push("--evidence");
        args.push(evidence_str);
    }
    let out_str = out.to_str().expect("utf8 path").to_owned();
    args.push("-o");
    args.push(&out_str);
    let output = run(&args);
    assert!(output.status.success(), "{:?}", output.stderr);
    (out, json_stdout(&output))
}

fn check(spec: &str, artifact: &Path, evidence: &[&Path]) -> Output {
    let mut args = vec![
        "document",
        "check",
        spec,
        artifact.to_str().expect("utf8 path"),
    ];
    let evidence_strs: Vec<String> = evidence
        .iter()
        .map(|path| path.to_str().expect("utf8 path").to_owned())
        .collect();
    for evidence_str in &evidence_strs {
        args.push("--evidence");
        args.push(evidence_str);
    }
    run(&args)
}

// --- Happy path --------------------------------------------------------------

#[test]
fn generate_with_evidence_overlays_assurance_and_records_digest() {
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (out, envelope) = generate_to_file(&[&evidence]);
    assert_eq!(envelope["result"], "generated");
    assert!(
        envelope["evidence"]["digest"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );
    assert_eq!(envelope["evidence"]["files"], 1);
    assert!(envelope.get("warnings").is_none());

    let markdown = std::fs::read_to_string(&out).expect("read generated markdown");
    assert!(markdown.contains("evidence_digest: sha256:"));
    let start = markdown.find("### REQ-1").expect("REQ-1 section exists");
    let rest = &markdown[start..];
    let end = rest.find("### REQ-2").expect("REQ-2 section exists");
    assert!(rest[..end].contains("proved(induction)"));
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&evidence);
}

// --- Diagnostics ---------------------------------------------------------------

#[test]
fn generate_evidence_unmatched_file_warns_by_default() {
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-99"},"completeness":"unbounded"}"#);
    let evidence_str = evidence.to_str().expect("utf8 path");
    let out = temp_path(".md");
    let out_str = out.to_str().expect("utf8 path");
    let output = run(&[
        "document",
        "generate",
        CANCEL_SYSTEM,
        "--lang",
        "ja",
        "--evidence",
        evidence_str,
        "-o",
        out_str,
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let envelope = json_stdout(&output);
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "FSL-DOC-EVIDENCE-UNMATCHED");
    assert_eq!(warnings[0]["target"], evidence_str);
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn generate_evidence_unmatched_file_fails_under_strict_on_a_fixture_with_no_other_coverage_issues()
{
    // Mirrors issue #330's own equivalent test: cancel_system.fsl has
    // pre-existing unsupported targets unrelated to evidence, which would
    // otherwise mask FSL-DOC-EVIDENCE-UNMATCHED under --strict. The empty
    // `init` keeps `init` out of the unsupported targets, so the fixture
    // declares no acceptance case: the requirement-trace gate could not
    // replay one from an incomplete initial state, and the document commands
    // refuse a spec whose traces it cannot replay (#1243).
    let spec = temp_path(".fsl");
    std::fs::write(
        &spec,
        r#"requirements Clean {
  state { counter: Int }
  init { }
  @requirement("REQ-1", "Bump can happen")
  fair action bump() { counter = counter + 1 }
}
"#,
    )
    .expect("write spec");
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-99"},"completeness":"unbounded"}"#);
    let spec_str = spec
        .strip_prefix(root())
        .expect("repository-local fixture")
        .to_str()
        .expect("utf8 path");
    let evidence_str = evidence.to_str().expect("utf8 path");

    let clean = run(&["document", "generate", spec_str, "--lang", "ja", "--strict"]);
    assert!(
        clean.status.success(),
        "fixture must have zero coverage issues before evidence is added: {:?}",
        clean.stderr
    );

    let output = run(&[
        "document",
        "generate",
        spec_str,
        "--lang",
        "ja",
        "--strict",
        "--evidence",
        evidence_str,
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-EVIDENCE-UNMATCHED");
    let _ = std::fs::remove_file(&spec);
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn generate_evidence_malformed_json_exits_two() {
    let evidence = write_evidence("{not json");
    let evidence_str = evidence.to_str().expect("utf8 path");
    let output = generate(&["--evidence", evidence_str]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-EVIDENCE-INVALID");
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn generate_evidence_non_object_json_exits_two() {
    let evidence = write_evidence("[1, 2, 3]");
    let evidence_str = evidence.to_str().expect("utf8 path");
    let output = generate(&["--evidence", evidence_str]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["code"], "FSL-DOC-EVIDENCE-INVALID");
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn generate_missing_evidence_file_exits_two_as_io_error() {
    let output = generate(&["--evidence", "/nonexistent/evidence.json"]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["kind"], "io");
}

// --- Digest separation --------------------------------------------------------

#[test]
fn changing_the_evidence_changes_artifact_digest_but_not_claim_set_digest() {
    let (out_none, envelope_none) = generate_to_file(&[]);
    let evidence_a = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (out_a, envelope_a) = generate_to_file(&[&evidence_a]);
    let evidence_b = write_evidence(
        r#"{"requirement":{"id":"REQ-1"},"completeness":"bounded","checked_to_depth":8}"#,
    );
    let (out_b, envelope_b) = generate_to_file(&[&evidence_b]);

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
    let _ = std::fs::remove_file(&evidence_a);
    let _ = std::fs::remove_file(&evidence_b);
}

#[test]
fn evidence_file_order_does_not_change_the_recorded_digest() {
    let evidence_a = write_evidence(
        r#"{"requirement":{"id":"REQ-1"},"completeness":"bounded","checked_to_depth":8,"result":"verified"}"#,
    );
    let evidence_b = write_evidence(
        r#"{"requirement":{"id":"REQ-1"},"completeness":"bounded","checked_to_depth":8,"result":"violated"}"#,
    );
    let (out_forward, forward) = generate_to_file(&[&evidence_a, &evidence_b]);
    let (out_reversed, reversed) = generate_to_file(&[&evidence_b, &evidence_a]);
    assert_eq!(
        forward["evidence"]["digest"],
        reversed["evidence"]["digest"]
    );
    assert_eq!(forward["artifact_digest"], reversed["artifact_digest"]);
    assert_eq!(
        std::fs::read_to_string(&out_forward).expect("read forward artifact"),
        std::fs::read_to_string(&out_reversed).expect("read reversed artifact")
    );
    let _ = std::fs::remove_file(&out_forward);
    let _ = std::fs::remove_file(&out_reversed);
    let _ = std::fs::remove_file(&evidence_a);
    let _ = std::fs::remove_file(&evidence_b);
}

// --- `fslc document check` evidence parity ------------------------------------

#[test]
fn check_is_conformant_when_the_same_evidence_is_supplied() {
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (artifact, _) = generate_to_file(&[&evidence]);
    let output = check(CANCEL_SYSTEM, &artifact, &[&evidence]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn check_reports_evidence_changed_when_the_flag_is_omitted() {
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (artifact, _) = generate_to_file(&[&evidence]);
    let output = check(CANCEL_SYSTEM, &artifact, &[]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "evidence_changed"
                && reason["code"] == "FSL-DOC-EVIDENCE-CHANGED"
                && reason["detail"] == "generated_with_evidence")
    );
    // The evidence-derived residue text legitimately differs, but that must
    // not surface as a separate edit_outside_slot reason.
    assert!(
        !reasons
            .iter()
            .any(|reason| reason["kind"] == "edit_outside_slot")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn check_reports_evidence_changed_when_spurious_evidence_is_supplied() {
    let (artifact, _) = generate_to_file(&[]);
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let output = check(CANCEL_SYSTEM, &artifact, &[&evidence]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "evidence_changed"
                && reason["detail"] == "generated_without_evidence")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&evidence);
}

#[test]
fn check_reports_evidence_changed_when_the_evidence_content_differs() {
    let evidence_a = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (artifact, _) = generate_to_file(&[&evidence_a]);
    let evidence_b = write_evidence(
        r#"{"requirement":{"id":"REQ-1"},"completeness":"bounded","checked_to_depth":8}"#,
    );
    let output = check(CANCEL_SYSTEM, &artifact, &[&evidence_b]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "evidence_changed"
                && reason["detail"] == "evidence_digest_mismatch")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&evidence_a);
    let _ = std::fs::remove_file(&evidence_b);
}

#[test]
fn check_without_evidence_on_an_evidenceless_artifact_is_unaffected() {
    let (artifact, _) = generate_to_file(&[]);
    let output = check(CANCEL_SYSTEM, &artifact, &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
}

// --- The split-gate payoff: a genuine hand-edit is still caught -------------

#[test]
fn a_hand_edit_inside_a_claim_block_is_still_caught_even_when_evidence_also_changed() {
    // Acceptance-relevant regression guard for the skip_claim_text /
    // skip_residue_text split (fsl_tools::document_check): unlike a
    // renderer or glossary change, an evidence-only change must not mask a
    // genuine hand-edit inside a claim block.
    let evidence = write_evidence(r#"{"requirement":{"id":"REQ-1"},"completeness":"unbounded"}"#);
    let (artifact, _) = generate_to_file(&[&evidence]);
    let text = std::fs::read_to_string(&artifact).expect("read artifact");
    assert!(text.contains("CancelForm"), "fixture text not found");
    let edited_text = text.replace("CancelForm", "CancelFormX");
    let edited = temp_path(".md");
    std::fs::write(&edited, edited_text).expect("write edited artifact");

    // check omits --evidence, so evidence_changed also fires.
    let output = check(CANCEL_SYSTEM, &edited, &[]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "evidence_changed")
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_changed"
                && reason["code"] == "FSL-DOC-BLOCK-DRIFT"
                && reason["detail"] == "artifact_edited")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
    let _ = std::fs::remove_file(&evidence);
}
