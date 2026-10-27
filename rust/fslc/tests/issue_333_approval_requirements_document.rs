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
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
    approval_repo_with_date(None)
}

fn approval_repo_with_date(date: Option<&str>) -> PathBuf {
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
    if date.is_some() {
        let source = std::fs::read_to_string(repository_file("tests/fixtures/approval.fsl"))
            .expect("read approval fixture");
        std::fs::write(root.join("spec.fsl"), source.replace("\r\n", "\n"))
            .expect("write fixed LF approval fixture");
    } else {
        std::fs::copy(
            repository_file("tests/fixtures/approval.fsl"),
            root.join("spec.fsl"),
        )
        .expect("copy approval fixture");
    }
    git(&root, &["init", "-q"]);
    git(
        &root,
        &["config", "user.email", "approval-test@example.com"],
    );
    git(&root, &["config", "user.name", "Approval Test"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    if date.is_some() {
        git(&root, &["config", "core.autocrlf", "false"]);
    }
    git(&root, &["add", "spec.fsl"]);
    if let Some(date) = date {
        let output = Command::new("git")
            .args(["commit", "-qm", "approval baseline"])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .current_dir(&root)
            .output()
            .expect("commit fixed approval baseline");
        assert!(
            output.status.success(),
            "fixed baseline: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        git(&root, &["commit", "-qm", "approval baseline"]);
    }
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

// #1475 uses the same test-owned Git baseline as the approval tests above.
// Drop also cleans up on a detector assertion failure.
fn assert_base_golden(actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1475_golden/approval-fixed.json");
    let expected: Value = serde_json::from_slice(&std::fs::read(path).expect("read base golden"))
        .expect("base golden JSON");
    assert_eq!(expected["base"], "c25f8653b9c83a247321f375f98cd59f430b6326");
    assert_eq!(actual, &expected["observed"], "full base approval output");
}

struct PanicAuditApproval {
    root: PathBuf,
    record: Value,
    fixed_bytes: Vec<u8>,
    markdown: Vec<u8>,
}

impl Drop for PanicAuditApproval {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove panic-audit approval repository");
    }
}

impl PanicAuditApproval {
    fn new() -> Self {
        // Fixed author/committer clock and LF blob yield the identical Git
        // baseline on base and head, without excluding git_commit or digests.
        let root = approval_repo_with_date(Some("2026-10-27T00:00:00Z"));
        generate_document(&root);
        let markdown = std::fs::read(root.join("requirements.md")).expect("reviewed bytes");
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
                "record.json",
            ],
        );
        let mut record = created["record"].clone();
        let original = std::fs::read_to_string(root.join("record.json")).expect("created record");
        let old_timestamp = record["approval"]["approved_at"]
            .as_str()
            .expect("timestamp");
        assert_eq!(original.matches(old_timestamp).count(), 1);
        // Retain production field ordering, indentation and final newline.
        let fixed_bytes = original
            .replace(old_timestamp, "2026-10-27T00:00:00Z")
            .into_bytes();
        record["approval"]["approved_at"] = json!("2026-10-27T00:00:00Z");
        assert_eq!(
            serde_json::from_slice::<Value>(&fixed_bytes).expect("fixed record JSON"),
            record
        );
        std::fs::write(root.join("record.json"), &fixed_bytes).expect("fix canonical timestamp");
        Self {
            root,
            record,
            fixed_bytes,
            markdown,
        }
    }

    fn checked(&self) -> Value {
        let output = run(
            &self.root,
            &["approval", "check", "spec.fsl", "--record", "record.json"],
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let checked = json_output(&output);
        assert_eq!(checked["status"], "approved");
        checked
    }

    fn timestamp(&self, value: &str) {
        let mut record = self.record.clone();
        record["approval"]["approved_at"] = json!(value);
        let mut bytes = serde_json::to_vec_pretty(&record).expect("encode mutated timestamp");
        bytes.push(b'\n');
        std::fs::write(self.root.join("record.json"), bytes).expect("write mutated record");
    }

    fn timestamp_refusal(&self, value: &str) {
        self.timestamp(value);
        let output = run(
            &self.root,
            &["approval", "check", "spec.fsl", "--record", "record.json"],
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "timestamp={value:?}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            json_output(&output),
            json!({
                "fsl":"1.0", "result":"error", "kind":"io",
                "message":"approval record requires a canonical UTC approval timestamp"
            })
        );
    }
}

#[test]
fn issue_1475_newline_filename_approval_uses_shared_document_refusal() {
    let fixture = PanicAuditApproval::new();
    fixture.checked();
    let name = "spec.fsl\nnot-a-frontmatter-field";
    std::fs::copy(fixture.root.join("spec.fsl"), fixture.root.join(name))
        .expect("copy newline-named spec");
    git(&fixture.root, &["add", "--", name]);
    git(
        &fixture.root,
        &["commit", "-qm", "newline filename baseline"],
    );
    let accepted = run(&fixture.root, &["check", name]);
    assert_eq!(accepted.status.code(), Some(0));
    assert_eq!(json_output(&accepted)["result"], "ok");
    let output = run(
        &fixture.root,
        &[
            "approval",
            "create",
            name,
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "newline-record.json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        json_output(&output),
        json!({
            "fsl":"1.0", "result":"error", "kind":"document",
            "message":"malformed frontmatter line: \"not-a-frontmatter-field\""
        })
    );
    assert!(!fixture.root.join("newline-record.json").exists());
    assert_eq!(
        std::fs::read(fixture.root.join("requirements.md")).expect("reviewed artifact"),
        fixture.markdown
    );
}

macro_rules! timestamp_digit_detector {
    ($name:ident, $offset:literal) => {
        #[test]
        fn $name() {
            let fixture = PanicAuditApproval::new();
            fixture.checked(); // Prove Git, artifact binding and all digests are valid first.
            let mut bytes = b"2026-10-27T00:00:00Z".to_vec();
            bytes[$offset] = b'!';
            fixture.timestamp_refusal(std::str::from_utf8(&bytes).expect("ASCII timestamp"));
        }
    };
}
timestamp_digit_detector!(issue_1475_timestamp_year_digit_one, 0);
timestamp_digit_detector!(issue_1475_timestamp_year_digit_two, 1);
timestamp_digit_detector!(issue_1475_timestamp_year_digit_three, 2);
timestamp_digit_detector!(issue_1475_timestamp_year_digit_four, 3);
timestamp_digit_detector!(issue_1475_timestamp_month_digit_one, 5);
timestamp_digit_detector!(issue_1475_timestamp_month_digit_two, 6);
timestamp_digit_detector!(issue_1475_timestamp_day_digit_one, 8);
timestamp_digit_detector!(issue_1475_timestamp_day_digit_two, 9);
timestamp_digit_detector!(issue_1475_timestamp_hour_digit_one, 11);
timestamp_digit_detector!(issue_1475_timestamp_hour_digit_two, 12);
timestamp_digit_detector!(issue_1475_timestamp_minute_digit_one, 14);
timestamp_digit_detector!(issue_1475_timestamp_minute_digit_two, 15);
timestamp_digit_detector!(issue_1475_timestamp_second_digit_one, 17);
timestamp_digit_detector!(issue_1475_timestamp_second_digit_two, 18);

#[test]
fn issue_1475_fixed_timestamp_record_and_document_bytes_are_preserved() {
    let fixture = PanicAuditApproval::new();
    let checked = fixture.checked();
    let mut preserved = json!({
        "record": fixture.record,
        "record_sha256": format!("{:x}", Sha256::digest(&fixture.fixed_bytes)),
        "reviewed_markdown_sha256": format!("{:x}", Sha256::digest(&fixture.markdown)),
        "check": checked,
    });
    println!(
        "FIXED APPROVAL RECORD\n{}\nREVIEWED MARKDOWN\n{}\nAPPROVAL CHECK {checked}",
        String::from_utf8_lossy(&fixture.fixed_bytes),
        String::from_utf8_lossy(&fixture.markdown)
    );
    assert_eq!(
        checked,
        fixture.checked(),
        "same-binary repeat determinism, no exclusions"
    );
    let mut first_overlay = None;
    for _ in 0..2 {
        let generated = run(
            &fixture.root,
            &[
                "document",
                "generate",
                "spec.fsl",
                "--lang",
                "ja",
                "--approval",
                "record.json",
                "-o",
                "overlay.md",
            ],
        );
        assert_eq!(
            generated.status.code(),
            Some(0),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&generated.stdout),
            String::from_utf8_lossy(&generated.stderr)
        );
        assert_eq!(json_output(&generated)["result"], "generated");
        let overlay = (
            json_output(&generated),
            std::fs::read(fixture.root.join("overlay.md")).expect("overlay bytes"),
        );
        preserved["overlay"] = json!({"generate": overlay.0,
            "markdown_sha256": format!("{:x}", Sha256::digest(&overlay.1))});
        if let Some(first) = &first_overlay {
            assert_eq!(&overlay, first, "full overlay envelope and Markdown bytes");
        } else {
            first_overlay = Some(overlay);
        }
        assert_eq!(
            std::fs::read(fixture.root.join("record.json")).expect("record readback"),
            fixture.fixed_bytes
        );
        assert_eq!(
            std::fs::read(fixture.root.join("requirements.md")).expect("reviewed bytes"),
            fixture.markdown
        );
        let record: Value = serde_json::from_slice(&fixture.fixed_bytes).expect("canonical record");
        assert_eq!(
            record, fixture.record,
            "all fields, including target/claim-set/reviewed digests"
        );
    }
    assert_base_golden(&preserved);
    fixture.timestamp("2024-02-29T23:59:59Z");
    fixture.checked();
}

#[test]
fn issue_1475_timestamp_nondigit_above_zero_and_calendar_ranges_stay_rejected() {
    let fixture = PanicAuditApproval::new();
    fixture.checked();
    for value in [
        "A026-10-27T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "2026-10-27T24:00:00Z",
        "2026-10-27T00:60:00Z",
        "2026-10-27T00:00:60Z",
        "2026-13-27T00:00:00Z",
        "2026-10-00T00:00:00Z",
        "2026-10-27 00:00:00Z",
        "2026-10-27T00:00:00",
    ] {
        fixture.timestamp_refusal(value);
    }
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
fn editable_slot_is_bound_separately_from_the_canonical_rendering() {
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
        "the background-slot edit must not change the canonical digest"
    );
    assert_eq!(
        created["record"]["target"]["reviewed_digest_algorithm"],
        "fsl-reviewed-requirements-document-v1+sha256"
    );
    assert_ne!(
        created["record"]["target"]["reviewed_digest"], created["record"]["target"]["digest"],
        "the literal reviewed bytes must be bound independently"
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

    edit_artifact(
        &root,
        "requirements.md",
        "本プロジェクト固有の背景説明。",
        "承認後に差し替えられた背景説明。",
    );
    let drifted = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(drifted["status"], "drifted");
    assert_eq!(drifted["reasons"], json!(["artifact_changed"]));

    let generate = failed(
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
    assert_eq!(generate["code"], "FSL-DOC-APPROVAL-DRIFTED");
    assert_eq!(generate["reasons"], json!(["artifact_changed"]));

    edit_artifact(
        &root,
        "requirements.md",
        "承認後に差し替えられた背景説明。",
        "本プロジェクト固有の背景説明。",
    );
    let restored = successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "requirements.approval.json",
        ],
    );
    assert_eq!(restored["status"], "approved");
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
fn requirements_document_record_without_reviewed_digest_is_rejected() {
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
    let record_path = root.join("requirements.approval.json");
    let mut record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("read record"))
            .expect("parse record");
    record["target"]
        .as_object_mut()
        .expect("target object")
        .remove("reviewed_digest");
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
            "requirements.approval.json",
        ],
    );
    assert_eq!(output["kind"], "io");
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

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
