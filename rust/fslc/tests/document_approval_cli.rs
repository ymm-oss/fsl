// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for `fslc document generate`/`check --approval` (issue
//! #333): the "Approval records" overlay section, its intent-fidelity
//! disclaimer, drift rejection, and `document check`'s reproduction
//! contract. The `requirements_document` approval-record side itself
//! (schema v3/v4, `claim_set_digest`, signing) is exercised in
//! `rust/fslc/tests/issue_333_approval_requirements_document.rs`.

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
        "fslc-document-approval-{}-{nonce}-{}",
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

/// Generate the canonical document, approve it, and return the approval
/// record's path (relative to `root`).
fn generate_and_approve(root: &Path) -> &'static str {
    generate_and_approve_for_lang(root, "ja")
}

/// Like [`generate_and_approve`], but under an explicit `--lang` (an
/// approval record binds to one specific rendering, so testing the
/// non-default locale needs its own artifact/record pair, not a reused
/// ja-approved record fed into an `en` render).
fn generate_and_approve_for_lang(root: &Path, lang: &str) -> &'static str {
    let artifact = if lang == "en" {
        "requirements.en.md"
    } else {
        "requirements.md"
    };
    let record = if lang == "en" {
        "requirements.en.approval.json"
    } else {
        "requirements.approval.json"
    };
    successful(
        root,
        &[
            "document", "generate", "spec.fsl", "--lang", lang, "-o", artifact,
        ],
    );
    successful(
        root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            artifact,
            "--approver",
            "alice",
            "-o",
            record,
        ],
    );
    record
}

// --- Happy path --------------------------------------------------------------

#[test]
fn document_generate_renders_matching_approval_overlay() {
    let root = approval_repo();
    let record = generate_and_approve(&root);
    let generated = successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            record,
            "-o",
            "requirements.approved.md",
        ],
    );
    assert!(
        generated["approvals"]["digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
    );
    assert_eq!(generated["approvals"]["records"], 1);

    let markdown =
        std::fs::read_to_string(root.join("requirements.approved.md")).expect("read artifact");
    assert!(markdown.contains("approval_digest: sha256:"));
    assert!(markdown.contains("## 承認記録"));
    assert!(markdown.contains("承認者: alice"));
    assert!(markdown.contains("署名: なし"));
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn approval_overlay_includes_the_intent_fidelity_disclaimer_in_both_locales() {
    // Without -o, `document generate` prints the raw Markdown directly to
    // stdout (the established no-`-o` bypass convention) rather than a JSON
    // envelope, so read stdout as text here.
    let root = approval_repo();
    let record = generate_and_approve(&root);
    let ja = run(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            record,
        ],
    );
    assert!(ja.status.success(), "{:?}", ja.stderr);
    assert!(String::from_utf8_lossy(&ja.stdout).contains("原意への忠実性を証明するものではない"));

    let en_record = generate_and_approve_for_lang(&root, "en");
    let en = run(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "en",
            "--approval",
            en_record,
        ],
    );
    assert!(en.status.success(), "{:?}", en.stderr);
    assert!(
        String::from_utf8_lossy(&en.stdout)
            .contains("It does not prove fidelity to original intent.")
    );
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- Drift / diagnostics -------------------------------------------------------

#[test]
fn document_generate_rejects_a_drifted_approval_record() {
    let root = approval_repo();
    let record = generate_and_approve(&root);
    let spec_path = root.join("spec.fsl");
    let spec = std::fs::read_to_string(&spec_path).expect("read spec");
    let mutated = spec.replace(
        "REQ-190: an approved review remains approved",
        "REQ-190: an approved review always remains approved",
    );
    assert_ne!(spec, mutated);
    std::fs::write(&spec_path, mutated).expect("write mutated spec");

    let output = failed(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            record,
        ],
    );
    assert_eq!(output["code"], "FSL-DOC-APPROVAL-DRIFTED");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn document_generate_rejects_non_requirements_document_approval_records() {
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
    let output = failed(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "ledger.approval.json",
        ],
    );
    assert_eq!(output["code"], "FSL-DOC-APPROVAL-INVALID");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn unverifiable_signed_records_block_document_generate() {
    let root = approval_repo();
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "-o",
            "requirements.md",
        ],
    );
    let (private, public) = write_signing_keys(&root, 11, "alice");
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

    // No --trust-key at all: no key matches the record's key ID, so this
    // fails the same "io"-kind way `fslc approval check` already does for
    // an unmatched trust anchor.
    let output = failed(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "requirements.approval.json",
        ],
    );
    assert_eq!(output["kind"], "io");

    // A trust key that does not match the signer.
    let (_, wrong_public) = write_signing_keys(&root, 12, "mallory");
    let wrong_public = wrong_public.to_str().expect("utf8 path").to_owned();
    let output = failed(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "requirements.approval.json",
            "--trust-key",
            &wrong_public,
        ],
    );
    assert_eq!(output["kind"], "io");

    // The matching trust key succeeds.
    let generated = successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "requirements.approval.json",
            "--trust-key",
            &public,
            "-o",
            "requirements.approved.md",
        ],
    );
    assert_eq!(generated["result"], "generated");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn a_tampered_signature_with_a_matching_trust_key_is_rejected() {
    let root = approval_repo();
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "-o",
            "requirements.md",
        ],
    );
    let (private, public) = write_signing_keys(&root, 13, "alice");
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
    // Flip exactly one *interior* character (never the last): see the
    // matching comment in issue_333_approval_requirements_document.rs's
    // `tampered_v4_signature_is_rejected`.
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

    let output = failed(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            "requirements.approval.json",
            "--trust-key",
            &public,
        ],
    );
    assert_eq!(output["code"], "FSL-DOC-APPROVAL-INVALID");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

// --- `fslc document check --approval` ---------------------------------------

#[test]
fn document_check_reproduces_the_approval_overlay() {
    let root = approval_repo();
    let record = generate_and_approve(&root);
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            record,
            "-o",
            "requirements.approved.md",
        ],
    );
    let output = run(
        &root,
        &[
            "document",
            "check",
            "spec.fsl",
            "requirements.approved.md",
            "--approval",
            record,
        ],
    );
    assert!(output.status.success(), "{:?}", output.stderr);
    let checked = json_output(&output);
    assert_eq!(checked["result"], "document_conformant");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn document_check_reports_approval_changed_when_the_flag_is_omitted() {
    let root = approval_repo();
    let record = generate_and_approve(&root);
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "--approval",
            record,
            "-o",
            "requirements.approved.md",
        ],
    );
    let output = run(
        &root,
        &["document", "check", "spec.fsl", "requirements.approved.md"],
    );
    assert_eq!(output.status.code(), Some(1));
    let checked = json_output(&output);
    let reasons = checked["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "approval_changed"
                && reason["code"] == "FSL-DOC-APPROVAL-CHANGED"
                && reason["detail"] == "generated_with_approval")
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason["kind"] == "edit_outside_slot")
    );
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn document_without_approval_on_an_approval_less_artifact_is_unaffected() {
    let root = approval_repo();
    successful(
        &root,
        &[
            "document",
            "generate",
            "spec.fsl",
            "--lang",
            "ja",
            "-o",
            "requirements.md",
        ],
    );
    let output = run(&root, &["document", "check", "spec.fsl", "requirements.md"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_output(&output)["result"], "document_conformant");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}
