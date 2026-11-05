// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! AP3 compatibility controls against immutable approvals produced by actual B.
//! No production reader, signer, or validator is used as the test oracle.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);
const CAPTURE_SHA256: &str = "21927ff97ae3ddd91ee6759b6f8a22ffd62c169e96441f26a2d877f06841a77f";
const SLOT: &str = "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）";
const DOCUMENT_FILES: [&str; 4] = [
    "glossary.json",
    "./evidence-b.json",
    "nested/../evidence-a.json",
    "./requirements.md",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_1421_legacy")
}

fn raw_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read JSON file")).expect("parse JSON file")
}

fn git(root: &Path, args: &[&str]) -> String {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Approval Test")
        .env("GIT_AUTHOR_EMAIL", "approval-test@example.com")
        .env("GIT_AUTHOR_DATE", "2026-10-27T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Approval Test")
        .env("GIT_COMMITTER_EMAIL", "approval-test@example.com")
        .env("GIT_COMMITTER_DATE", "2026-10-27T00:00:00Z")
        .output()
        .expect("run fixture git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 git output")
        .trim()
        .to_owned()
}

struct LegacyRepo {
    root: PathBuf,
    capture: Value,
}

impl Drop for LegacyRepo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove temporary legacy repository");
    }
}

impl LegacyRepo {
    fn new() -> Self {
        let capture_bytes = std::fs::read(fixtures().join("capture.json")).expect("read capture");
        assert_eq!(raw_sha256(&capture_bytes), CAPTURE_SHA256);
        let capture: Value = serde_json::from_slice(&capture_bytes).expect("parse capture");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "fslc-1421-legacy-{}-{nonce}-{}",
            std::process::id(),
            NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create temporary legacy repository");
        let root = root.canonicalize().expect("physical fixture CWD");
        for (name, expected) in capture["files"].as_object().expect("captured files") {
            let bytes = std::fs::read(fixtures().join(name)).expect("read captured file");
            assert_eq!(raw_sha256(&bytes), expected["sha256"]);
            assert_eq!(
                u64::try_from(bytes.len()).expect("file length"),
                expected["bytes"]
            );
            std::fs::write(root.join(name), bytes).expect("copy unchanged captured bytes");
        }
        std::fs::create_dir(root.join("nested")).expect("create literal dotdot input directory");
        git(&root, &["init", "--object-format=sha1", "-q"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(
            &root,
            &[
                "config",
                "core.hooksPath",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            ],
        );
        git(&root, &["add", "spec.fsl"]);
        git(&root, &["commit", "-qm", "approval baseline"]);
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), capture["git"]["commit"]);
        assert_eq!(
            git(&root, &["rev-parse", "HEAD^{tree}"]),
            capture["git"]["tree"]
        );
        assert_eq!(
            git(&root, &["rev-parse", "HEAD:spec.fsl"]),
            capture["git"]["source_blob"]
        );
        Self { root, capture }
    }

    fn originals(&self) -> Vec<Vec<u8>> {
        self.capture["files"]
            .as_object()
            .expect("captured files")
            .keys()
            .map(|name| std::fs::read(self.root.join(name)).expect("read original fixture bytes"))
            .collect()
    }

    fn record(&self, version: u8) -> Value {
        json_file(&self.root.join(format!("approval.v{version}.json")))
    }

    fn public_key(&self) -> PathBuf {
        self.root.join("seed7-public.pem")
    }
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run native approval CLI")
}

fn checked(output: &Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "unexpected stderr");
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn check(cwd: &Path, record: &Path, trust: Option<&Path>) -> Output {
    let mut args = vec![
        "approval",
        "check",
        "./spec.fsl",
        "--record",
        record.to_str().expect("UTF-8 record"),
    ];
    if let Some(key) = trust {
        args.extend(["--trust-key", key.to_str().expect("UTF-8 trust key")]);
    }
    run(cwd, &args)
}

fn assert_full(output: &Output, exit: i32, expected: &Value) {
    assert_eq!(
        checked(output, exit),
        *expected,
        "complete JSON, without exclusions"
    );
}

fn canonical_json(value: &Value, bytes: &mut Vec<u8>) {
    match value {
        Value::Array(items) => {
            bytes.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    bytes.push(b',');
                }
                canonical_json(item, bytes);
            }
            bytes.push(b']');
        }
        Value::Object(object) => {
            bytes.push(b'{');
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend(serde_json::to_vec(key).expect("encode object key"));
                bytes.push(b':');
                canonical_json(&object[key], bytes);
            }
            bytes.push(b'}');
        }
        _ => bytes.extend(serde_json::to_vec(value).expect("encode JSON scalar")),
    }
}

fn signature_payload(record: &Value, domain: &str) -> Vec<u8> {
    let mut payload_record = record.clone();
    payload_record["signature"]
        .as_object_mut()
        .expect("signature object")
        .remove("value");
    let mut payload = domain.as_bytes().to_vec();
    payload.push(0);
    canonical_json(&payload_record, &mut payload);
    payload
}

fn signature_for(record: &Value, domain: &str) -> String {
    let key = SigningKey::from_bytes(&[7; 32]);
    URL_SAFE_NO_PAD.encode(key.sign(&signature_payload(record, domain)).to_bytes())
}

fn verifies(record: &Value, domain: &str) -> bool {
    let bytes = URL_SAFE_NO_PAD
        .decode(record["signature"]["value"].as_str().expect("signature"))
        .expect("base64url signature");
    let signature = Signature::from_slice(&bytes).expect("64-byte signature");
    SigningKey::from_bytes(&[7; 32])
        .verifying_key()
        .verify_strict(&signature_payload(record, domain), &signature)
        .is_ok()
}

fn bad_signature(record: &Value) -> Value {
    let mut mutant = record.clone();
    let mut bytes = URL_SAFE_NO_PAD
        .decode(record["signature"]["value"].as_str().expect("signature"))
        .expect("base64url signature");
    bytes[0] ^= 1;
    mutant["signature"]["value"] = json!(URL_SAFE_NO_PAD.encode(bytes));
    assert!(!verifies(
        &mutant,
        record["schema"].as_str().expect("schema")
    ));
    mutant
}

fn write_mutant(repo: &LegacyRepo, record: &Value) -> PathBuf {
    let path = PathBuf::from("mutant.json");
    std::fs::write(
        repo.root.join(&path),
        serde_json::to_vec_pretty(record).expect("encode mutant"),
    )
    .expect("write derived mutant, never an original fixture");
    path
}

fn invalid_signature(record: &Value, path: &Path, warnings: &Value) -> Value {
    json!({
        "fsl":"1.0", "result":"approval_check", "status":"signature-invalid",
        "signature_status":"signature-invalid", "key_id":record["signature"]["key_id"],
        "reasons":["signature_invalid"], "record":path.to_str().expect("UTF-8 record"),
        "target_kind":record["target"]["kind"], "approver":record["approval"]["approver"],
        "approved_at":record["approval"]["approved_at"], "requirements":record["approval"]["requirements"],
        "baseline_digest":record["spec"]["digest"], "warnings":warnings,
    })
}

fn io_error(message: &str, warnings: &Value) -> Value {
    json!({"fsl":"1.0", "result":"error", "kind":"io", "message":message, "warnings":warnings})
}

#[test]
fn ap3_genuine_v1_through_v4_preserve_complete_b_outputs_and_original_bytes() {
    let repo = LegacyRepo::new();
    let before = repo.originals();
    let public = repo.public_key();
    for version in 1..=4 {
        let name = format!("approval.v{version}.json");
        let record = repo.record(version);
        assert_eq!(record["schema"], format!("fslc.approval.v{version}"));
        assert!(record.get("path_base").is_none());
        assert!(record.get("render_source").is_none());
        assert_eq!(
            record["approval"]["approved_at"],
            repo.capture["approved_at"][&name]
        );
        let trust = if version == 2 || version == 4 {
            Some(public.as_path())
        } else {
            None
        };
        assert_full(
            &check(&repo.root, Path::new(&name), trust),
            0,
            &repo.capture["checks"][&name],
        );
    }
    assert_eq!(
        repo.originals(),
        before,
        "no migration or rewriting of captured records/inputs/artifacts"
    );
}

#[test]
fn ap3_independent_signer_reproduces_b_v2_and_v4_and_rejects_domain_substitution() {
    let repo = LegacyRepo::new();
    let before = repo.originals();
    for version in [2, 4] {
        let record = repo.record(version);
        let domain = format!("fslc.approval.v{version}");
        assert_eq!(
            signature_for(&record, &domain),
            record["signature"]["value"]
        );
        assert!(verifies(&record, &domain));
        assert!(!verifies(&record, "fslc.approval.v6"));
    }
    assert_eq!(repo.originals(), before);
}

fn legacy_solver_target_not_read(version: u8) {
    let original = LegacyRepo::new();
    let caller = LegacyRepo::new();
    let original_before = original.originals();
    let caller_before = caller.originals();
    let record = original.root.join(format!("approval.v{version}.json"));
    let public = original.public_key();
    let trust = if version == 2 {
        Some(public.as_path())
    } else {
        None
    };
    let baseline = checked(&check(&original.root, &record, trust), 0);
    assert_eq!(baseline["status"], "approved");
    assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    let path = caller.root.join("ledger.md");
    let bytes = std::fs::read(&path).expect("read caller solver target");
    std::fs::write(&path, b"caller-local solver target decoy\n")
        .expect("write distinct solver decoy");
    assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    std::fs::remove_file(&path).expect("remove caller solver target");
    assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    std::fs::write(&path, bytes).expect("restore exact caller solver target");
    assert_eq!(original.originals(), original_before);
    assert_eq!(caller.originals(), caller_before);
}

#[test]
fn ap3_legacy_v1_and_v2_keep_live_solver_targets_out_of_check() {
    for version in [1, 2] {
        legacy_solver_target_not_read(version);
    }
}

fn legacy_local_reviewed_target(version: u8) {
    let original = LegacyRepo::new();
    let caller = LegacyRepo::new();
    let original_before = original.originals();
    let caller_before = caller.originals();
    let record = original.root.join(format!("approval.v{version}.json"));
    let public = original.public_key();
    let trust = if version == 4 {
        Some(public.as_path())
    } else {
        None
    };
    let baseline = checked(&check(&original.root, &record, trust), 0);
    assert_eq!(baseline["status"], "approved");
    assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    let path = caller.root.join("requirements.md");
    let bytes = std::fs::read(&path).expect("read caller reviewed bytes");
    let source = std::str::from_utf8(&bytes).expect("UTF-8 reviewed target");
    assert!(source.contains(SLOT));
    let changed = source.replace(SLOT, "Caller-local reviewed background.");
    assert_ne!(changed.as_bytes(), bytes);
    std::fs::write(&path, changed.as_bytes()).expect("write valid caller-local reviewed decoy");
    let mut expected = baseline.clone();
    expected["status"] = json!("drifted");
    expected["reasons"] = json!(["artifact_changed"]);
    expected["current_reviewed_digest"] =
        json!(format!("sha256:{}", raw_sha256(changed.as_bytes())));
    assert_full(&check(&caller.root, &record, trust), 0, &expected);
    std::fs::write(&path, bytes).expect("restore exact caller reviewed bytes");
    assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    assert_eq!(original.originals(), original_before);
    assert_eq!(caller.originals(), caller_before);
}

#[test]
fn ap3_legacy_v3_and_v4_read_caller_local_reviewed_bytes_with_supported_source_labels() {
    for version in [3, 4] {
        legacy_local_reviewed_target(version);
    }
}

fn legacy_missing_local_files(version: u8) {
    let original = LegacyRepo::new();
    let caller = LegacyRepo::new();
    let original_before = original.originals();
    let caller_before = caller.originals();
    let record = original.root.join(format!("approval.v{version}.json"));
    let public = original.public_key();
    let trust = if version == 4 {
        Some(public.as_path())
    } else {
        None
    };
    let baseline = checked(&check(&caller.root, &record, trust), 0);
    assert_eq!(baseline["status"], "approved");
    for name in DOCUMENT_FILES {
        let path = caller.root.join(name);
        let bytes = std::fs::read(&path).expect("read local input/target");
        std::fs::remove_file(&path).expect("remove only caller-local file");
        let error = std::fs::read(&path).expect_err("local file must really be absent");
        let expected = io_error(&error.to_string(), &baseline["warnings"]);
        assert_full(&check(&caller.root, &record, trust), 2, &expected);
        assert_full(&check(&original.root, &record, trust), 0, &baseline);
        std::fs::write(&path, bytes).expect("restore exact caller-local bytes");
        assert_full(&check(&caller.root, &record, trust), 0, &baseline);
    }
    assert_eq!(
        original.originals(),
        original_before,
        "record-side originals never become fallback data"
    );
    assert_eq!(caller.originals(), caller_before);
}

#[test]
fn ap3_legacy_v3_and_v4_do_not_fallback_to_record_side_inputs_or_reviewed_targets() {
    for version in [3, 4] {
        legacy_missing_local_files(version);
    }
}

fn assert_signature_faults(repo: &LegacyRepo, record: &Value, name: &Path, warnings: &Value) {
    let public = repo.public_key();
    assert!(warnings.is_array());
    let missing_trust = io_error(
        &format!(
            "no trusted Ed25519 public key matches '{}'",
            record["signature"]["key_id"].as_str().expect("key id")
        ),
        warnings,
    );
    assert_full(&check(&repo.root, name, None), 2, &missing_trust);
    let mutant = bad_signature(record);
    let mutant_path = write_mutant(repo, &mutant);
    let invalid = invalid_signature(&mutant, &mutant_path, warnings);
    assert_full(&check(&repo.root, &mutant_path, Some(&public)), 1, &invalid);
    for file in DOCUMENT_FILES {
        let path = repo.root.join(file);
        let bytes = std::fs::read(&path).expect("read file before fault");
        std::fs::remove_file(&path).expect("remove input/target after signature controls");
        assert_full(&check(&repo.root, name, None), 2, &missing_trust);
        assert_full(&check(&repo.root, &mutant_path, Some(&public)), 1, &invalid);
        std::fs::write(&path, bytes).expect("restore faulted file bytes");
    }
}

#[test]
fn ap3_v2_and_v4_trust_and_signature_failures_precede_input_and_target_faults() {
    for version in [2, 4] {
        let repo = LegacyRepo::new();
        let before = repo.originals();
        let record = repo.record(version);
        let name = PathBuf::from(format!("approval.v{version}.json"));
        let warnings = &repo.capture["checks"][name.to_str().expect("record name")]["warnings"];
        assert_signature_faults(&repo, &record, &name, warnings);
        assert_eq!(repo.originals(), before);
    }
}

fn create_v6(repo: &LegacyRepo) -> (Value, Value) {
    // Standard RFC 8410 PKCS#8 seed encoding, deterministic test-only material.
    let mut der = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    der.extend([7; 32]);
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(der)
    );
    std::fs::write(repo.root.join("seed7-private.pem"), pem).expect("write test-only signing key");
    let created = checked(
        &run(
            &repo.root,
            &[
                "approval",
                "create",
                "./spec.fsl",
                "--kind",
                "requirements_document",
                "--artifact",
                "./requirements.md",
                "--approver",
                "alice",
                "--glossary",
                "glossary.json",
                "--evidence",
                DOCUMENT_FILES[1],
                "--evidence",
                DOCUMENT_FILES[2],
                "--signing-key",
                "./seed7-private.pem",
                "-o",
                "new-v6.json",
            ],
        ),
        0,
    );
    let record = json_file(&repo.root.join("new-v6.json"));
    assert_eq!(created["record"], record);
    assert_eq!(record["schema"], "fslc.approval.v6");
    assert_eq!(
        signature_for(&record, "fslc.approval.v6"),
        record["signature"]["value"]
    );
    assert!(verifies(&record, "fslc.approval.v6"));
    let baseline = checked(
        &check(
            &repo.root,
            Path::new("new-v6.json"),
            Some(&repo.public_key()),
        ),
        0,
    );
    assert_eq!(baseline["status"], "approved");
    assert_eq!(baseline["signature_status"], "signed");
    assert_eq!(baseline["reasons"], json!([]));
    (record, baseline)
}

#[test]
fn ap3_v6_trust_and_signature_failures_precede_input_and_target_faults() {
    let repo = LegacyRepo::new();
    let before = repo.originals();
    let (record, baseline) = create_v6(&repo);
    let path = repo.root.join("new-v6.json");
    let original_bytes = std::fs::read(&path).expect("new v6 bytes");
    assert_signature_faults(
        &repo,
        &record,
        Path::new("new-v6.json"),
        &baseline["warnings"],
    );
    assert_full(
        &check(
            &repo.root,
            Path::new("new-v6.json"),
            Some(&repo.public_key()),
        ),
        0,
        &baseline,
    );
    assert_eq!(std::fs::read(path).expect("new v6 bytes"), original_bytes);
    assert_eq!(repo.originals(), before);
}

#[test]
fn ap3_v6_signatures_cover_both_context_fields_before_missing_input_reads() {
    let repo = LegacyRepo::new();
    let before = repo.originals();
    let (record, baseline) = create_v6(&repo);
    let original_bytes = std::fs::read(repo.root.join("new-v6.json")).expect("new v6 bytes");
    std::fs::create_dir(repo.root.join("missing-input-base")).expect("create empty alternate base");
    for (field, value) in [
        ("path_base", "missing-input-base"),
        ("render_source", "./different-display.fsl"),
    ] {
        let mut mutant = record.clone();
        mutant[field] = json!(value);
        assert!(!verifies(&mutant, "fslc.approval.v6"));
        let path = write_mutant(&repo, &mutant);
        let expected = invalid_signature(&mutant, &path, &baseline["warnings"]);
        assert_full(
            &check(&repo.root, &path, Some(&repo.public_key())),
            1,
            &expected,
        );
    }
    assert_full(
        &check(
            &repo.root,
            Path::new("new-v6.json"),
            Some(&repo.public_key()),
        ),
        0,
        &baseline,
    );
    assert_eq!(
        std::fs::read(repo.root.join("new-v6.json")).expect("new v6 bytes"),
        original_bytes
    );
    assert_eq!(repo.originals(), before);
}

#[test]
fn ap3_v6_rejects_validly_encoded_signature_from_the_v4_domain() {
    let repo = LegacyRepo::new();
    let before = repo.originals();
    let (record, baseline) = create_v6(&repo);
    let mut mutant = record.clone();
    mutant["signature"]["value"] = json!(signature_for(&record, "fslc.approval.v4"));
    assert!(verifies(&mutant, "fslc.approval.v4"));
    assert!(!verifies(&mutant, "fslc.approval.v6"));
    let path = write_mutant(&repo, &mutant);
    let expected = invalid_signature(&mutant, &path, &baseline["warnings"]);
    assert_full(
        &check(&repo.root, &path, Some(&repo.public_key())),
        1,
        &expected,
    );
    assert_full(
        &check(
            &repo.root,
            Path::new("new-v6.json"),
            Some(&repo.public_key()),
        ),
        0,
        &baseline,
    );
    assert_eq!(repo.originals(), before);
}
