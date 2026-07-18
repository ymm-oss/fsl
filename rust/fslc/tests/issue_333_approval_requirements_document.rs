// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the `requirements_document` approval target kind
//! (issue #333): `fslc approval create/check/diff --kind
//! requirements_document`, its schema v3 (unsigned) / v4 (signed) revision,
//! and the `claim_set_digest` drift reason. `fslc document generate/check
//! --approval`'s own overlay contract is exercised in
//! `rust/fslc/tests/document_approval_cli.rs`; this file covers the
//! approval-record side only.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::Value;

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn compiled_schema(relative: &str) -> jsonschema::Validator {
    let schema_text = std::fs::read_to_string(repository_file(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
    let schema_value: Value = serde_json::from_str(&schema_text).expect("schema is valid JSON");
    jsonschema::validator_for(&schema_value).expect("schema compiles")
}

fn assert_valid_against(record: &Value, schema_relative: &str) {
    let validator = compiled_schema(schema_relative);
    let errors: Vec<String> = validator
        .iter_errors(record)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema validation errors: {errors:?}");
}

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
}

fn successful(cwd: &Path, arguments: &[&str]) -> Value {
    let output = run(cwd, arguments);
    assert!(
        output.status.success(),
        "command {arguments:?} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json_output(&output)
}

fn failed(cwd: &Path, arguments: &[&str]) -> Value {
    let output = run(cwd, arguments);
    assert!(
        !output.status.success(),
        "command {arguments:?} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    json_output(&output)
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn approval_repo() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!(
        "fslc-issue-333-{}-{nonce}-{}",
        std::process::id(),
        NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed),
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove stale temporary repository");
    }
    std::fs::create_dir_all(&root).expect("create temporary repository");
    std::fs::copy(
        repository_file("tests/fixtures/approval.fsl"),
        root.join("spec.fsl"),
    )
    .expect("copy approval fixture");
    git(&root, &["init", "-q"]);
    git(
        &root,
        &["config", "user.email", "approval-test@example.com"],
    );
    git(&root, &["config", "user.name", "Approval Test"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["add", "spec.fsl"]);
    git(&root, &["commit", "-qm", "approval baseline"]);
    root
}

#[allow(clippy::default_trait_access)]
fn write_signing_keys(root: &Path, seed: u8, prefix: &str) -> (PathBuf, PathBuf) {
    let signing = SigningKey::from_bytes(&[seed; 32]);
    let private = root.join(format!("{prefix}-private.pem"));
    let public = root.join(format!("{prefix}-public.pem"));
    std::fs::write(
        &private,
        signing
            .to_pkcs8_pem(Default::default())
            .expect("encode private key")
            .as_bytes(),
    )
    .expect("write private key");
    std::fs::write(
        &public,
        signing
            .verifying_key()
            .to_public_key_pem(Default::default())
            .expect("encode public key"),
    )
    .expect("write public key");
    (private, public)
}

fn generate_document(root: &Path) -> Value {
    successful(
        root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "-o",
            "requirements.md",
        ],
    )
}

fn edit_artifact(root: &Path, name: &str, from: &str, to: &str) {
    let path = root.join(name);
    let text = std::fs::read_to_string(&path).expect("read artifact");
    assert!(text.contains(from), "fixture text {from:?} not found");
    std::fs::write(&path, text.replace(from, to)).expect("write edited artifact");
}

// --- Happy path ----------------------------------------------------------

#[test]
fn requirements_document_approval_create_and_check_round_trip() {
    let root = approval_repo();
    generate_document(&root);
    let created = successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );
    assert_eq!(created["record"]["schema"], "fslc.approval.v3");
    assert_eq!(created["record"]["target"]["kind"], "requirements_document");
    assert_eq!(
        created["record"]["target"]["digest_algorithm"],
        "fsl-rendered-requirements-document-v1+sha256"
    );
    assert_eq!(
        created["record"]["target"]["claim_set_digest_algorithm"],
        "fsl-rcir-claim-set-v1+sha256"
    );
    assert!(
        created["record"]["target"]["claim_set_digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
    );
    assert_eq!(
        created["record"]["target"]["inputs"]["view"],
        "requirements"
    );
    assert_eq!(created["record"]["target"]["inputs"]["lang"], "ja");
    assert!(created["record"]["target"]["inputs"]["glossary"].is_null());
    assert_eq!(
        created["record"]["target"]["inputs"]["evidence"]
            .as_array()
            .expect("evidence array")
            .len(),
        0
    );
    assert_valid_against(
        &created["record"],
        "schemas/fslc/approval/approval-record.v3.schema.json",
    );

    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(checked["status"], "approved");
    assert_eq!(checked["target_kind"], "requirements_document");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn requirements_document_approval_detects_spec_and_claim_set_drift() {
    let root = approval_repo();
    generate_document(&root);
    successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );

    let spec_path = root.join("spec.fsl");
    let spec = std::fs::read_to_string(&spec_path).expect("read spec");
    let mutated = spec.replace(
        "REQ-190: an approved review remains approved",
        "REQ-190: an approved review always remains approved",
    );
    assert_ne!(spec, mutated, "fixture text must actually change");
    std::fs::write(&spec_path, mutated).expect("write mutated spec");
    git(&root, &["add", "spec.fsl"]);
    git(&root, &["commit", "-qm", "mutate requirement text"]);

    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(checked["status"], "drifted");
    let reasons: Vec<&str> = checked["reasons"]
        .as_array()
        .expect("reasons array")
        .iter()
        .map(|reason| reason.as_str().expect("reason string"))
        .collect();
    assert!(reasons.contains(&"spec_changed"));
    assert!(reasons.contains(&"claim_set_changed"));
    assert!(reasons.contains(&"rendering_changed"));
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn editable_slot_edits_do_not_change_the_approved_digest() {
    let root = approval_repo();
    let generated = generate_document(&root);
    edit_artifact(
        &root,
        "requirements.md",
        "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）",
        "本プロジェクト固有の背景説明。",
    );
    let created = successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );
    assert_eq!(
        created["record"]["target"]["digest"], generated["artifact_digest"],
        "the background-slot edit must not change the recorded digest"
    );
    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(checked["status"], "approved");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn normative_claim_edits_are_rejected_at_approval_create() {
    let root = approval_repo();
    generate_document(&root);
    edit_artifact(
        &root,
        "requirements.md",
        "`approved` が `false`",
        "`approved` が `true`",
    );
    let output = failed(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
        ],
    );
    assert_eq!(output["kind"], "semantics");
    assert!(
        output["reasons"]
            .as_array()
            .expect("reasons array")
            .iter()
            .any(|reason| reason["kind"] == "claim_changed")
    );
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- Glossary/evidence inputs ---------------------------------------------

#[test]
fn requirements_document_approval_records_document_inputs_with_glossary_and_evidence() {
    let root = approval_repo();
    std::fs::write(
        root.join("glossary.json"),
        r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:approve":"承認する"}}"#,
    )
    .expect("write glossary");
    std::fs::write(
        root.join("evidence.json"),
        r#"{"requirement":{"id":"REQ-190"},"completeness":"unbounded"}"#,
    )
    .expect("write evidence");
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--glossary",
            "glossary.json",
            "--evidence",
            "evidence.json",
            "-o",
            "requirements.md",
        ],
    );
    let created = successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "--glossary",
            "glossary.json",
            "--evidence",
            "evidence.json",
            "-o",
            "requirements.approval.json",
        ],
    );
    assert_eq!(
        created["record"]["target"]["inputs"]["glossary"]["path"],
        "glossary.json"
    );
    assert_eq!(
        created["record"]["target"]["inputs"]["evidence"]
            .as_array()
            .expect("evidence array")
            .len(),
        1
    );
    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(checked["status"], "approved");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn changed_glossary_content_surfaces_as_rendering_drift_not_spec_drift() {
    let root = approval_repo();
    std::fs::write(
        root.join("glossary.json"),
        r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:approve":"承認する"}}"#,
    )
    .expect("write glossary");
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--glossary",
            "glossary.json",
            "-o",
            "requirements.md",
        ],
    );
    successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "--glossary",
            "glossary.json",
            "-o",
            "requirements.approval.json",
        ],
    );
    std::fs::write(
        root.join("glossary.json"),
        r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:approve":"レビューを承認する"}}"#,
    )
    .expect("overwrite glossary");
    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(checked["status"], "drifted");
    let reasons: Vec<&str> = checked["reasons"]
        .as_array()
        .expect("reasons array")
        .iter()
        .map(|reason| reason.as_str().expect("reason string"))
        .collect();
    assert_eq!(reasons, vec!["rendering_changed"]);
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- Signed v4 -------------------------------------------------------------

#[test]
fn signed_requirements_document_approval_uses_schema_v4_and_verifies() {
    let root = approval_repo();
    generate_document(&root);
    let (private, public) = write_signing_keys(&root, 7, "alice");
    let private = private.to_str().expect("utf8 path").to_owned();
    let public = public.to_str().expect("utf8 path").to_owned();
    let created = successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "--signing-key",
            &private,
            "-o",
            "requirements.approval.json",
        ],
    );
    assert_eq!(created["record"]["schema"], "fslc.approval.v4");
    assert_valid_against(
        &created["record"],
        "schemas/fslc/approval/approval-record.v4.schema.json",
    );

    let checked = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
            "--trust-key",
            &public,
        ],
    );
    assert_eq!(checked["status"], "approved");
    assert_eq!(checked["signature_status"], "signed");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn tampered_v4_signature_is_rejected() {
    let root = approval_repo();
    generate_document(&root);
    let (private, public) = write_signing_keys(&root, 9, "alice");
    let private = private.to_str().expect("utf8 path").to_owned();
    let public = public.to_str().expect("utf8 path").to_owned();
    successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "--signing-key",
            &private,
            "-o",
            "requirements.approval.json",
        ],
    );
    let record_path = root.join("requirements.approval.json");
    let mut record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("read record"))
            .expect("parse record");
    // Flip exactly one *interior* character (never the last): the last
    // base64url character of a 64-byte signature only uses 2 of its 6 bits,
    // and `TrustStore::verify` rejects a non-canonical encoding before ever
    // attempting Ed25519 verification — tampering it could non-deterministically
    // produce an "io"-kind error instead of the intended "signature-invalid".
    let original = record["signature"]["value"]
        .as_str()
        .expect("signature value")
        .to_owned();
    let mut chars: Vec<char> = original.chars().collect();
    let middle = chars.len() / 2;
    chars[middle] = if chars[middle] == 'A' { 'B' } else { 'A' };
    let tampered: String = chars.into_iter().collect();
    record["signature"]["value"] = Value::String(tampered);
    std::fs::write(
        &record_path,
        serde_json::to_string_pretty(&record).expect("serialize tampered record"),
    )
    .expect("write tampered record");

    let output = run(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
            "--trust-key",
            &public,
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let checked = json_output(&output);
    assert_eq!(checked["status"], "signature-invalid");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- Usage errors -----------------------------------------------------------

#[test]
fn solver_flags_are_rejected_with_requirements_document_kind() {
    let root = approval_repo();
    generate_document(&root);
    let output = failed(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "--depth",
            "4",
        ],
    );
    assert_eq!(output["kind"], "usage");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn document_flags_are_rejected_with_a_solver_kind() {
    let root = approval_repo();
    successful(&root, &["ledger", "spec.fsl", "-o", "review.md"]);
    let output = failed(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "alice",
            "--glossary",
            "glossary.json",
        ],
    );
    assert_eq!(output["kind"], "usage");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn approval_create_rejects_an_artifact_that_already_carries_an_approval_overlay() {
    let root = approval_repo();
    generate_document(&root);
    let created = successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );
    assert_eq!(created["result"], "created");
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "requirements.approval.json",
            "-o",
            "requirements.approved.md",
        ],
    );
    let output = failed(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.approved.md",
            "--approver",
            "bob",
        ],
    );
    assert_eq!(output["kind"], "semantics");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- Closed-contract negative controls --------------------------------------

#[test]
fn a_ledger_kind_record_hand_edited_into_a_document_schema_is_rejected() {
    let root = approval_repo();
    successful(&root, &["ledger", "spec.fsl", "-o", "review.md"]);
    successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "alice",
            "-o",
            "ledger.approval.json",
        ],
    );
    let record_path = root.join("ledger.approval.json");
    let mut record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("read record"))
            .expect("parse record");
    record["schema"] = Value::String("fslc.approval.v3".to_owned());
    std::fs::write(
        &record_path,
        serde_json::to_string_pretty(&record).expect("serialize hand-edited record"),
    )
    .expect("write hand-edited record");

    let output = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "ledger.approval.json",
        ],
    );
    assert_eq!(output["kind"], "io");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- `approval diff` acceptance smoke ---------------------------------------

#[test]
fn approval_diff_accepts_a_requirements_document_record() {
    let root = approval_repo();
    generate_document(&root);
    successful(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );
    let diffed = successful(
        &root,
        &[
            "approval",
            "diff",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert!(diffed.get("result").is_some());
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}
