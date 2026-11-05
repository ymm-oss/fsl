// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! AP4–AP7: raw-input rejection only after an otherwise-approved evaluation.
//! The working record pathname is fixed across every paired comparison.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT_REPO: AtomicU64 = AtomicU64::new(0);
const CAPTURE_HASH: &str = "21927ff97ae3ddd91ee6759b6f8a22ffd62c169e96441f26a2d877f06841a77f";
const WORKING: &str = "working.json";
const SLOT: &str = "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）";
const INPUTS: [&str; 3] = [
    "glossary.json",
    "./evidence-b.json",
    "nested/../evidence-a.json",
];
const DOCUMENT_FLAGS: [&str; 6] = [
    "--glossary",
    "glossary.json",
    "--evidence",
    "./evidence-b.json",
    "--evidence",
    "nested/../evidence-a.json",
];
const DRIFTS: [(&str, &str); 5] = [
    ("/spec/digest", "spec_changed"),
    ("/target/digest", "rendering_changed"),
    ("/target/generator_version", "renderer_changed"),
    ("/target/claim_set_digest", "claim_set_changed"),
    ("/target/reviewed_digest", "artifact_changed"),
];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_1421_legacy")
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", sha(bytes))
}
fn zero_digest() -> Value {
    json!(format!("sha256:{}", "0".repeat(64)))
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read JSON")).expect("JSON value")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Approval Test")
        .env("GIT_AUTHOR_EMAIL", "approval-test@example.com")
        .env("GIT_AUTHOR_DATE", "2026-10-27T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Approval Test")
        .env("GIT_COMMITTER_EMAIL", "approval-test@example.com")
        .env("GIT_COMMITTER_DATE", "2026-10-27T00:00:00Z")
        .output()
        .expect("fixture git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git UTF-8")
        .trim()
        .to_owned()
}

struct Repo {
    root: PathBuf,
    capture: Value,
}
impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove owned temporary repo");
    }
}
impl Repo {
    fn new() -> Self {
        let bytes = std::fs::read(fixture_dir().join("capture.json")).expect("capture manifest");
        assert_eq!(sha(&bytes), CAPTURE_HASH);
        let capture: Value = serde_json::from_slice(&bytes).expect("capture JSON");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "fslc-1421-digest-{}-{nonce}-{}",
            std::process::id(),
            NEXT_REPO.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create owned fixture");
        let root = root.canonicalize().expect("physical fixture CWD");
        for (name, expected) in capture["files"].as_object().expect("captured files") {
            let data = std::fs::read(fixture_dir().join(name)).expect("immutable captured bytes");
            assert_eq!(sha(&data), expected["sha256"]);
            std::fs::write(root.join(name), data).expect("copy genuine bytes unchanged");
        }
        std::fs::create_dir(root.join("nested")).expect("literal dotdot directory");
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
        write_test_key(&root);
        Self { root, capture }
    }

    fn originals(&self) -> Vec<Vec<u8>> {
        self.capture["files"]
            .as_object()
            .expect("files")
            .keys()
            .map(|name| std::fs::read(self.root.join(name)).expect("fixture snapshot"))
            .collect()
    }

    fn put_raw(&self, name: &str, record: &Value) {
        std::fs::write(
            self.root.join(name),
            serde_json::to_vec_pretty(record).expect("record JSON"),
        )
        .expect("write derived record only");
    }

    fn save(&self, name: &str, record: &Value) {
        let mut copy = record.clone();
        resign(&mut copy);
        self.put_raw(name, &copy);
    }

    fn put(&self, record: &Value) {
        self.save(WORKING, record);
    }

    fn record(&self, version: u8) -> Value {
        let record = if version <= 4 {
            read_json(&self.root.join(format!("approval.v{version}.json")))
        } else {
            self.create(version == 6)
        };
        assert_eq!(record["schema"], format!("fslc.approval.v{version}"));
        record
    }

    fn create(&self, signed: bool) -> Value {
        let mut args = vec![
            "approval",
            "create",
            "./spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "./requirements.md",
            "--approver",
            "alice",
            "-o",
            "created.json",
        ];
        args.extend(DOCUMENT_FLAGS);
        if signed {
            args.extend(["--signing-key", "seed7-private.pem"]);
        }
        let created = checked(&run(&self.root, &args), 0);
        let record = read_json(&self.root.join("created.json"));
        assert_eq!(created["record"], record);
        record
    }
}

fn write_test_key(root: &Path) {
    let mut der = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    der.extend([7; 32]);
    std::fs::write(
        root.join("seed7-private.pem"),
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(der)
        ),
    )
    .expect("deterministic test-only seed key");
}

fn canonical(value: &Value, bytes: &mut Vec<u8>) {
    match value {
        Value::Array(items) => {
            bytes.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                canonical(item, bytes);
            }
            bytes.push(b']');
        }
        Value::Object(object) => {
            bytes.push(b'{');
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_unstable();
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                bytes.extend(serde_json::to_vec(key).expect("canonical key"));
                bytes.push(b':');
                canonical(&object[key], bytes);
            }
            bytes.push(b'}');
        }
        _ => bytes.extend(serde_json::to_vec(value).expect("canonical scalar")),
    }
}

fn signature(record: &Value) -> String {
    let mut value = record.clone();
    value["signature"]
        .as_object_mut()
        .expect("signature object")
        .remove("value");
    let mut bytes = record["schema"]
        .as_str()
        .expect("domain")
        .as_bytes()
        .to_vec();
    bytes.push(0);
    canonical(&value, &mut bytes);
    URL_SAFE_NO_PAD.encode(SigningKey::from_bytes(&[7; 32]).sign(&bytes).to_bytes())
}
fn resign(record: &mut Value) {
    if record.get("signature").is_some() {
        let value = signature(record);
        record["signature"]["value"] = json!(value);
    }
}
fn corrupt_signature(record: &mut Value) {
    let mut bytes = URL_SAFE_NO_PAD
        .decode(record["signature"]["value"].as_str().expect("signature"))
        .expect("base64 signature");
    bytes[0] ^= 1;
    record["signature"]["value"] = json!(URL_SAFE_NO_PAD.encode(bytes));
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("native CLI")
}
fn checked(output: &Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}
fn same(output: &Output, exit: i32, expected: &Value) {
    assert_eq!(
        checked(output, exit),
        *expected,
        "complete JSON without exclusions"
    );
}
fn direct(repo: &Repo, trust: Option<&str>) -> Output {
    let mut args = vec!["approval", "check", "./spec.fsl", "--record", WORKING];
    if let Some(key) = trust {
        args.extend(["--trust-key", key]);
    }
    run(&repo.root, &args)
}
fn approval(repo: &Repo) -> Output {
    direct(repo, Some("seed7-public.pem"))
}
fn normal(repo: &Repo, record: &Value) -> Value {
    repo.put(record);
    let value = checked(&approval(repo), 0);
    assert_eq!(value["status"], "approved");
    assert_eq!(value["reasons"], json!([]));
    value
}
fn io(message: &str, warnings: &Value) -> Value {
    json!({"fsl":"1.0", "result":"error", "kind":"io", "message":message, "warnings":warnings})
}
fn wrong_raw(record: &Value, indices: &[usize]) -> Value {
    let mut value = record.clone();
    for &index in indices {
        let entry = if index == 0 {
            &mut value["target"]["inputs"]["glossary"]
        } else {
            &mut value["target"]["inputs"]["evidence"][index - 1]
        };
        assert_ne!(entry["digest"], zero_digest());
        entry["digest"] = zero_digest();
    }
    value
}
fn drifted(record: &Value, mask: usize) -> (Value, Vec<&'static str>) {
    let mut value = record.clone();
    let mut reasons = Vec::new();
    for (index, &(pointer, reason)) in DRIFTS.iter().enumerate() {
        if mask & (1 << index) != 0 {
            *value.pointer_mut(pointer).expect("existing binding") = if index == 2 {
                json!("digest-contract-other-renderer")
            } else {
                zero_digest()
            };
            reasons.push(reason);
        }
    }
    (value, reasons)
}

fn expected_drift(approved: &Value, record: &Value, reasons: &[&str]) -> Value {
    let mut expected = approved.clone();
    expected["status"] = json!("drifted");
    expected["reasons"] = json!(reasons);
    for (field, pointer) in [
        ("baseline_digest", "/spec/digest"),
        ("artifact_digest", "/target/digest"),
        ("claim_set_digest", "/target/claim_set_digest"),
        ("reviewed_digest", "/target/reviewed_digest"),
    ] {
        assert!(
            expected.get(field).is_some(),
            "live complete-evaluation field {field}"
        );
        expected[field] = record.pointer(pointer).expect("record binding").clone();
    }
    expected
}

#[test]
fn ap0_digest_mutant_signer_reproduces_both_genuine_signed_legacy_records() {
    let repo = Repo::new();
    let before = repo.originals();
    for version in [2, 4] {
        let record = read_json(&repo.root.join(format!("approval.v{version}.json")));
        assert_eq!(signature(&record), record["signature"]["value"]);
    }
    assert_eq!(repo.originals(), before);
}

fn raw_gap(version: u8) {
    let repo = Repo::new();
    let record = repo.record(version);
    let before = repo.originals();
    let baseline = normal(&repo, &record);
    let cases: &[&[usize]] = &[&[0], &[1], &[2], &[2, 1], &[2, 0, 1]];
    for indices in cases {
        repo.put(&wrong_raw(&record, indices));
        let first = *indices.iter().min().expect("at least one mismatch");
        let expected = io(
            &format!("approval input digest mismatch: {}", INPUTS[first]),
            &baseline["warnings"],
        );
        same(&approval(&repo), 2, &expected);
        same(
            &{
                repo.put(&record);
                approval(&repo)
            },
            0,
            &baseline,
        );
    }
    assert_eq!(repo.originals(), before);
}
#[test]
fn ap4_legacy_v3_otherwise_approved_raw_mismatch_is_io2() {
    raw_gap(3);
}
#[test]
fn ap4_legacy_v4_otherwise_approved_raw_mismatch_is_io2() {
    raw_gap(4);
}
#[test]
fn ap4_new_v5_otherwise_approved_raw_mismatch_is_io2() {
    raw_gap(5);
}
#[test]
fn ap4_new_v6_otherwise_approved_raw_mismatch_is_io2() {
    raw_gap(6);
}

fn reversed_evidence_gap(version: u8) {
    let repo = Repo::new();
    let before = repo.originals();
    let generated = checked(
        &run(
            &repo.root,
            &[
                "document",
                "generate",
                "./spec.fsl",
                "--lang",
                "ja",
                "--glossary",
                INPUTS[0],
                "--evidence",
                INPUTS[2],
                "--evidence",
                INPUTS[1],
                "-o",
                "./reversed.md",
            ],
        ),
        0,
    );
    let mut args = vec![
        "approval",
        "create",
        "./spec.fsl",
        "--kind",
        "requirements_document",
        "--artifact",
        "./reversed.md",
        "--approver",
        "alice",
        "--glossary",
        INPUTS[0],
        "--evidence",
        INPUTS[2],
        "--evidence",
        INPUTS[1],
        "-o",
        "created-reversed.json",
    ];
    if version == 6 {
        args.extend(["--signing-key", "seed7-private.pem"]);
    }
    let created = checked(&run(&repo.root, &args), 0);
    let record = read_json(&repo.root.join("created-reversed.json"));
    assert_eq!(created["record"], record);
    assert_eq!(record["schema"], format!("fslc.approval.v{version}"));
    assert_eq!(record["target"]["digest"], generated["artifact_digest"]);
    assert_eq!(record["target"]["inputs"]["evidence"][0]["path"], INPUTS[2]);
    assert_eq!(record["target"]["inputs"]["evidence"][1]["path"], INPUTS[1]);
    let baseline = normal(&repo, &record);
    repo.put(&wrong_raw(&record, &[2, 1]));
    let expected = io(
        &format!("approval input digest mismatch: {}", INPUTS[2]),
        &baseline["warnings"],
    );
    same(&approval(&repo), 2, &expected);
    repo.put(&record);
    same(&approval(&repo), 0, &baseline);
    assert_eq!(repo.originals(), before);
}

#[test]
fn ap4_new_v5_mismatch_selection_follows_reversed_recorded_evidence_order() {
    reversed_evidence_gap(5);
}

#[test]
fn ap4_new_v6_mismatch_selection_follows_reversed_recorded_evidence_order() {
    reversed_evidence_gap(6);
}

#[test]
fn ap5_all_31_ordered_drift_combinations_survive_raw_mismatch_in_v3_through_v6() {
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let approved = normal(&repo, &record);
        for mask in 1..32 {
            let (changed, reasons) = drifted(&record, mask);
            repo.put(&changed);
            let baseline = checked(&approval(&repo), 0);
            assert_eq!(
                baseline,
                expected_drift(&approved, &changed, &reasons),
                "version {version}, mask {mask}"
            );
            repo.put(&wrong_raw(&changed, &[2, 0, 1]));
            same(&approval(&repo), 0, &baseline);
        }
        repo.put(&record);
        same(&approval(&repo), 0, &approved);
        assert_eq!(repo.originals(), before);
    }
}

#[test]
fn ap5_existing_file_read_and_parse_errors_precede_raw_mismatch() {
    let cases: &[(&str, Option<&[u8]>, &str)] = &[
        ("glossary.json", None, "io"),
        (INPUTS[1], None, "io"),
        (INPUTS[2], None, "io"),
        ("requirements.md", None, "io"),
        ("spec.fsl", None, "io"),
        ("glossary.json", Some(b"{"), "document"),
        (INPUTS[1], Some(b"{"), "document"),
        (
            "glossary.json",
            Some(br#"{"schema":"fslc.document-glossary.v1","locale":"en","labels":{"action:approve":"Approve"}}"#),
            "document",
        ),
        (INPUTS[2], Some(&[0xff]), "io"),
        ("spec.fsl", Some(b"not an FSL document"), "parse"),
    ];
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let approved = normal(&repo, &record);
        for &(name, contents, kind) in cases {
            let path = repo.root.join(name);
            let bytes = std::fs::read(&path).expect("before fault");
            if let Some(contents) = contents {
                std::fs::write(&path, contents).expect("parse fault");
            } else {
                std::fs::remove_file(&path).expect("read fault");
            }
            repo.put(&record);
            let baseline = checked(&approval(&repo), 2);
            assert_eq!(baseline["result"], "error");
            assert_eq!(baseline["kind"], kind);
            repo.put(&wrong_raw(&record, &[0, 2]));
            same(&approval(&repo), 2, &baseline);
            std::fs::write(&path, bytes).expect("restore faulted bytes");
            repo.put(&record);
            same(&approval(&repo), 0, &approved);
        }
        assert_eq!(repo.originals(), before);
    }
}

#[test]
fn ap5_record_validation_identity_and_git_errors_precede_raw_mismatch() {
    let missing_commit = "0".repeat(40);
    let cases = [
        ("/target/inputs/lang", "unsupported", "io"),
        ("/target/inputs/evidence/1/digest", "malformed-digest", "io"),
        ("/spec/path", "other.fsl", "semantics"),
        ("/spec/git_commit", missing_commit.as_str(), "io"),
    ];
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let approved = normal(&repo, &record);
        for (pointer, value, kind) in cases {
            let mut fault = record.clone();
            *fault.pointer_mut(pointer).expect("field") = json!(value);
            repo.put(&fault);
            let baseline = checked(&approval(&repo), 2);
            assert_eq!(baseline["result"], "error");
            assert_eq!(baseline["kind"], kind);
            repo.put(&wrong_raw(&fault, &[0]));
            same(&approval(&repo), 2, &baseline);
        }
        repo.put(&record);
        same(&approval(&repo), 0, &approved);
        assert_eq!(repo.originals(), before);
    }
}

#[test]
fn ap5_trust_load_missing_trust_and_signature_errors_precede_raw_mismatch() {
    for version in [4, 6] {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let approved = normal(&repo, &record);
        std::fs::write(repo.root.join("bad-key.pem"), "not a PEM key").expect("trust load fault");
        for trust in [None, Some("bad-key.pem")] {
            repo.put(&record);
            let baseline = checked(&direct(&repo, trust), 2);
            assert_eq!(baseline["kind"], "io");
            repo.put(&wrong_raw(&record, &[0]));
            same(&direct(&repo, trust), 2, &baseline);
        }
        let mut bad = record.clone();
        resign(&mut bad);
        corrupt_signature(&mut bad);
        repo.put_raw(WORKING, &bad);
        let invalid = checked(&approval(&repo), 1);
        assert_eq!(invalid["status"], "signature-invalid");
        assert_eq!(invalid["reasons"], json!(["signature_invalid"]));
        let mut mismatch = wrong_raw(&record, &[0, 1]);
        resign(&mut mismatch);
        corrupt_signature(&mut mismatch);
        repo.put_raw(WORKING, &mismatch);
        same(&approval(&repo), 1, &invalid);
        let target = repo.root.join("requirements.md");
        let bytes = std::fs::read(&target).expect("target");
        std::fs::remove_file(&target).expect("combined missing reviewed target");
        same(&approval(&repo), 1, &invalid);
        std::fs::write(target, bytes).expect("restore reviewed bytes");
        repo.put(&record);
        same(&approval(&repo), 0, &approved);
        assert_eq!(repo.originals(), before);
    }
}

fn document(repo: &Repo, record: Option<&str>, output: &str) -> Output {
    let mut args = vec![
        "document",
        "generate",
        "./spec.fsl",
        "--lang",
        "ja",
        "-o",
        output,
    ];
    args.extend(DOCUMENT_FLAGS);
    if let Some(record) = record {
        args.extend(["--approval", record, "--trust-key", "seed7-public.pem"]);
    }
    run(&repo.root, &args)
}
fn edit_slot(path: &Path, text: &str) {
    let old = std::fs::read_to_string(path).expect("document text");
    assert_eq!(old.matches(SLOT).count(), 1);
    std::fs::write(path, old.replace(SLOT, text)).expect("edit only nonnormative slot");
}

#[test]
fn ap6_preapproval_slot_edits_keep_canonical_and_reviewed_bindings_separate() {
    for version in [5, 6] {
        let repo = Repo::new();
        let generated = checked(&document(&repo, None, "requirements.md"), 0);
        edit_slot(
            &repo.root.join("requirements.md"),
            "Reviewed background before approval.",
        );
        let reviewed = std::fs::read(repo.root.join("requirements.md")).expect("reviewed bytes");
        let record = repo.record(version);
        let before = repo.originals();
        assert_eq!(record["target"]["digest"], generated["artifact_digest"]);
        assert_eq!(record["target"]["reviewed_digest"], digest(&reviewed));
        assert_ne!(
            record["target"]["digest"],
            record["target"]["reviewed_digest"]
        );
        normal(&repo, &record);
        assert_eq!(repo.originals(), before);
    }
}

#[test]
fn ap6_real_glossary_and_slot_drifts_preserve_complete_results_and_reason_order() {
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let approved = normal(&repo, &record);
        let target = repo.root.join("requirements.md");
        let target_bytes = std::fs::read(&target).expect("target");
        let glossary = repo.root.join(INPUTS[0]);
        let glossary_bytes = std::fs::read(&glossary).expect("glossary");
        for (glossary_changed, slot_changed, reasons) in [
            (true, false, vec!["rendering_changed"]),
            (false, true, vec!["artifact_changed"]),
            (true, true, vec!["rendering_changed", "artifact_changed"]),
        ] {
            if glossary_changed {
                let mut value = read_json(&glossary);
                value["labels"]["action:approve"] = json!("レビューを承認する");
                std::fs::write(
                    &glossary,
                    serde_json::to_vec(&value).expect("glossary JSON"),
                )
                .expect("valid glossary change");
            }
            if slot_changed {
                edit_slot(&target, "Changed after approval.");
            }
            let mut matching = record.clone();
            matching["target"]["inputs"]["glossary"]["digest"] =
                json!(digest(&std::fs::read(&glossary).expect("loaded glossary")));
            repo.put(&matching);
            let baseline = checked(&approval(&repo), 0);
            let canonical = checked(&document(&repo, None, "observed-canonical.md"), 0);
            let mut expected = expected_drift(&approved, &matching, &reasons);
            expected["current_artifact_digest"] = canonical["artifact_digest"].clone();
            expected["current_reviewed_digest"] = json!(digest(
                &std::fs::read(&target).expect("current reviewed bytes")
            ));
            assert_eq!(baseline, expected);
            repo.put(&wrong_raw(&matching, &[0, 1]));
            same(&approval(&repo), 0, &baseline);
            std::fs::write(&target, &target_bytes).expect("restore reviewed bytes");
            std::fs::write(&glossary, &glossary_bytes).expect("restore glossary bytes");
        }
        repo.put(&record);
        same(&approval(&repo), 0, &approved);
        assert_eq!(repo.originals(), before);
    }
}

fn ledger(repo: &Repo, records: &[&str], output: &str) -> Output {
    let mut args = vec![
        "ledger",
        "./spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "-o",
        output,
        "--trust-key",
        "seed7-public.pem",
    ];
    for record in records {
        args.extend(["--approval", *record]);
    }
    run(&repo.root, &args)
}
fn ledger_observation(repo: &Repo, records: &[&str]) -> (Value, Vec<u8>) {
    let result = checked(&ledger(repo, records, "order.md"), 0);
    let bytes = std::fs::read(repo.root.join("order.md")).expect("full ledger bytes");
    (result, bytes)
}

#[test]
fn ap7_ledger_retains_cli_record_order_and_complete_drift_results() {
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        let (left, _) = drifted(&record, 1);
        let (right, _) = drifted(&record, 4);
        repo.save("left.json", &left);
        repo.save("right.json", &right);
        let single_right = ledger_observation(&repo, &["right.json"]);
        assert_eq!(
            ledger_observation(&repo, &["left.json", "right.json"]),
            single_right
        );
        let single_left = ledger_observation(&repo, &["left.json"]);
        assert_eq!(
            ledger_observation(&repo, &["right.json", "left.json"]),
            single_left
        );
        assert_ne!(
            single_left.1, single_right.1,
            "order has an observable last-wins effect"
        );
        repo.save("left.json", &wrong_raw(&left, &[0]));
        repo.save("right.json", &wrong_raw(&right, &[1]));
        assert_eq!(
            ledger_observation(&repo, &["left.json", "right.json"]),
            single_right
        );
        assert_eq!(
            ledger_observation(&repo, &["right.json", "left.json"]),
            single_left
        );
        assert_eq!(repo.originals(), before);
    }
}

#[test]
fn ap7_ledger_otherwise_approved_mismatches_refuse_in_cli_record_order() {
    let repo = Repo::new();
    let record = repo.record(6);
    let before = repo.originals();
    normal(&repo, &record);
    let mut malformed = record.clone();
    malformed["target"]["inputs"]["glossary"]["digest"] = json!("malformed");
    repo.save("left.json", &malformed);
    let mut expected = checked(&ledger(&repo, &["left.json"], "order.md"), 2);
    assert_eq!(expected["fsl"], "1.0");
    assert_eq!(expected["result"], "error");
    assert_eq!(expected["kind"], "io");
    repo.save("left.json", &wrong_raw(&record, &[0]));
    repo.save("right.json", &wrong_raw(&record, &[1]));
    for (records, input) in [
        (["left.json", "right.json"], INPUTS[0]),
        (["right.json", "left.json"], INPUTS[1]),
    ] {
        expected["message"] = json!(format!("approval input digest mismatch: {input}"));
        same(&ledger(&repo, &records, "order.md"), 2, &expected);
    }
    assert_eq!(repo.originals(), before);
}

#[test]
fn ap7_direct_drift_ledger_verification_and_publication_keep_distinct_exit_rules() {
    let repo = Repo::new();
    let before = repo.originals();
    let source_path = repo.root.join("spec.fsl");
    let bytes = std::fs::read(&source_path).expect("source");
    let source = std::str::from_utf8(&bytes).expect("source UTF-8");
    let assignment = "\n    approved = true\n";
    assert_eq!(source.matches(assignment).count(), 1);
    std::fs::write(
        &source_path,
        source.replace(assignment, "\n    approved = false\n"),
    )
    .expect("known violated invariant");
    let record = read_json(&repo.root.join("approval.v1.json"));
    repo.put(&record);
    let drift = checked(&approval(&repo), 0);
    assert_eq!(drift["status"], "drifted");
    let without = checked(&ledger(&repo, &[], "violated.md"), 1);
    same(&ledger(&repo, &[WORKING], "violated.md"), 1, &without);
    let publication = checked(&ledger(&repo, &[WORKING], "missing-parent/out.md"), 2);
    assert_eq!(publication["result"], "error");
    assert_eq!(publication["kind"], "io");
    std::fs::write(source_path, bytes).expect("restore exact source bytes");
    assert_eq!(repo.originals(), before);
}

fn combined_record_digest(repo: &Repo, name: &str) -> String {
    digest(digest(&std::fs::read(repo.root.join(name)).expect("record bytes")).as_bytes())
}
fn replace_approval_digest(bytes: &[u8], old: &str, new: &str) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).expect("overlay UTF-8");
    let field = format!("approval_digest: {old}\n");
    assert_eq!(text.matches(&field).count(), 1);
    text.replace(&field, &format!("approval_digest: {new}\n"))
        .into_bytes()
}
fn changed_overlay(repo: &Repo, baseline: &Value, bytes: &[u8]) -> (Value, Vec<u8>) {
    let combined = combined_record_digest(repo, WORKING);
    let changed = replace_approval_digest(
        bytes,
        baseline["approvals"]["digest"]
            .as_str()
            .expect("old combined digest"),
        &combined,
    );
    let mut result = baseline.clone();
    result["approvals"]["digest"] = json!(combined);
    result["artifact_digest"] = json!(digest(&changed));
    (result, changed)
}

fn lightweight_generate(version: u8) {
    let repo = Repo::new();
    let record = repo.record(version);
    let before = repo.originals();
    normal(&repo, &record);
    let generated = checked(&document(&repo, Some(WORKING), "overlay.md"), 0);
    let bytes = std::fs::read(repo.root.join("overlay.md")).expect("overlay bytes");
    let mut changed = wrong_raw(&record, &[0, 1, 2]);
    changed["spec"]["git_commit"] = json!("0".repeat(40));
    changed["target"]["inputs"]["glossary"]["path"] = json!("absent/stored-glossary.json");
    for (index, input) in changed["target"]["inputs"]["evidence"]
        .as_array_mut()
        .expect("evidence")
        .iter_mut()
        .enumerate()
    {
        input["path"] = json!(format!("absent/stored-evidence-{index}.json"));
    }
    changed["target"]["generator_version"] = json!("ignored-by-lightweight-generate");
    repo.put(&changed);
    let (expected, expected_bytes) = changed_overlay(&repo, &generated, &bytes);
    same(&document(&repo, Some(WORKING), "overlay.md"), 0, &expected);
    assert_eq!(
        std::fs::read(repo.root.join("overlay.md")).expect("overlay"),
        expected_bytes
    );
    let authoritative = checked(&approval(&repo), 2);
    assert_eq!(authoritative["kind"], "io");
    assert_eq!(repo.originals(), before);
}
#[test]
fn ap7_generate_uses_explicit_inputs_without_stored_input_or_git_baseline_admission() {
    for version in 3..=6 {
        lightweight_generate(version);
    }
}

#[test]
fn ap7_generate_resolves_new_reviewed_target_but_keeps_current_explicit_rendering() {
    for version in [5, 6] {
        let original = Repo::new();
        let caller = Repo::new();
        let record = original.record(version);
        let original_before = original.originals();
        let caller_before = caller.originals();
        original.put(&record);
        let record_path = original.root.join(WORKING);
        let record_arg = record_path.to_str().expect("absolute record path");
        let key = original.root.join("seed7-public.pem");
        let key_arg = key.to_str().expect("absolute trust key");
        let mut args = vec![
            "document",
            "generate",
            "./spec.fsl",
            "--lang",
            "ja",
            "-o",
            "overlay.md",
            "--approval",
            record_arg,
            "--trust-key",
            key_arg,
        ];
        args.extend(DOCUMENT_FLAGS);
        let expected = checked(&run(&original.root, &args), 0);
        let bytes = std::fs::read(original.root.join("overlay.md")).expect("original overlay");
        // The reviewed target comes from the checkout of the spec being generated.
        let path = original.root.join("requirements.md");
        let reviewed = std::fs::read(&path).expect("real reviewed target");
        std::fs::remove_file(caller.root.join("requirements.md"))
            .expect("no caller reviewed target");
        let failure = checked(&run(&caller.root, &args), 2);
        assert_eq!(failure["kind"], "io");
        std::fs::write(caller.root.join("requirements.md"), &reviewed)
            .expect("caller's own reviewed target");
        std::fs::remove_file(&path).expect("remove original reviewed target");
        same(&run(&caller.root, &args), 0, &expected);
        assert_eq!(
            std::fs::read(caller.root.join("overlay.md")).expect("caller overlay"),
            bytes
        );
        std::fs::write(path, reviewed).expect("restore original reviewed bytes");
        let glossary_path = caller.root.join(INPUTS[0]);
        let glossary_bytes = std::fs::read(&glossary_path).expect("caller glossary");
        let mut glossary = read_json(&glossary_path);
        glossary["labels"]["action:approve"] = json!("呼び出し側の異なる明示用語");
        std::fs::write(
            &glossary_path,
            serde_json::to_vec(&glossary).expect("glossary JSON"),
        )
        .expect("valid caller-local explicit glossary");
        let drift = json!({
            "fsl":"1.0", "result":"error", "kind":"document",
            "message":"a supplied --approval record does not match the current rendering",
            "code":"FSL-DOC-APPROVAL-DRIFTED", "reasons":["rendering_changed"],
            "warnings":expected["warnings"],
        });
        same(&run(&caller.root, &args), 2, &drift);
        std::fs::write(glossary_path, glossary_bytes).expect("restore caller glossary bytes");
        same(&run(&caller.root, &args), 0, &expected);
        assert_eq!(original.originals(), original_before);
        assert_eq!(caller.originals(), caller_before);
    }
}

#[test]
fn ap7_generate_keeps_signature_first_within_record_and_sorted_deduplicated_drift_reasons() {
    for version in [4, 6] {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        normal(&repo, &record);
        let (drift, _) = drifted(&record, 31);
        repo.put(&drift);
        repo.save("other.json", &drift);
        let mut args = vec![
            "document",
            "generate",
            "./spec.fsl",
            "--lang",
            "ja",
            "--approval",
            WORKING,
            "--approval",
            "other.json",
            "--trust-key",
            "seed7-public.pem",
        ];
        args.extend(DOCUMENT_FLAGS);
        let baseline = checked(&run(&repo.root, &args), 2);
        assert_eq!(baseline["code"], "FSL-DOC-APPROVAL-DRIFTED");
        assert_eq!(
            baseline["reasons"],
            json!([
                "artifact_changed",
                "claim_set_changed",
                "rendering_changed",
                "spec_changed"
            ])
        );
        repo.put(&wrong_raw(&drift, &[0, 1]));
        repo.save("other.json", &wrong_raw(&drift, &[2]));
        same(&run(&repo.root, &args), 2, &baseline);
        let mut bad = record.clone();
        resign(&mut bad);
        corrupt_signature(&mut bad);
        repo.put_raw(WORKING, &bad);
        let invalid = checked(&document(&repo, Some(WORKING), "overlay.md"), 2);
        assert_eq!(invalid["code"], "FSL-DOC-APPROVAL-INVALID");
        let mut bad_mismatch = wrong_raw(&record, &[0, 1, 2]);
        resign(&mut bad_mismatch);
        corrupt_signature(&mut bad_mismatch);
        repo.put_raw(WORKING, &bad_mismatch);
        same(&document(&repo, Some(WORKING), "overlay.md"), 2, &invalid);
        let path = repo.root.join("requirements.md");
        let bytes = std::fs::read(&path).expect("target");
        std::fs::remove_file(&path)
            .expect("missing target cannot beat signature failure within record");
        same(&document(&repo, Some(WORKING), "overlay.md"), 2, &invalid);
        std::fs::write(path, bytes).expect("restore reviewed bytes");
        assert_eq!(repo.originals(), before);
    }
}

fn structural_check(repo: &Repo) -> Output {
    let mut args = vec![
        "document",
        "check",
        "./spec.fsl",
        "overlay.md",
        "--approval",
        WORKING,
    ];
    args.extend(DOCUMENT_FLAGS);
    run(&repo.root, &args)
}
#[test]
fn ap7_document_check_is_structural_and_still_rejects_a_wrong_derived_approval_digest() {
    for version in [4, 6] {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        normal(&repo, &record);
        let generated = checked(&document(&repo, Some(WORKING), "overlay.md"), 0);
        let bytes =
            std::fs::read(repo.root.join("overlay.md")).expect("authentic generated overlay");
        let mut forged = wrong_raw(&record, &[0, 1, 2]);
        resign(&mut forged);
        corrupt_signature(&mut forged);
        repo.put_raw(WORKING, &forged);
        let authoritative = checked(&approval(&repo), 1);
        assert_eq!(authoritative["reasons"], json!(["signature_invalid"]));
        let admission = checked(&document(&repo, Some(WORKING), "overlay.md"), 2);
        assert_eq!(admission["code"], "FSL-DOC-APPROVAL-INVALID");
        let (_, derived) = changed_overlay(&repo, &generated, &bytes);
        std::fs::write(repo.root.join("overlay.md"), &derived)
            .expect("test-authored structural witness, not authenticity evidence");
        let expected = checked(&structural_check(&repo), 0);
        assert_eq!(expected["result"], "document_conformant");
        let target = repo.root.join("requirements.md");
        let reviewed = std::fs::read(&target).expect("target");
        std::fs::remove_file(&target).expect("structural check must not read reviewed target");
        same(&structural_check(&repo), 0, &expected);
        let correct = combined_record_digest(&repo, WORKING);
        let wrong = replace_approval_digest(
            &derived,
            &correct,
            zero_digest().as_str().expect("zero digest"),
        );
        std::fs::write(repo.root.join("overlay.md"), wrong)
            .expect("rejecting structural negative control");
        let rejected = checked(&structural_check(&repo), 1);
        assert_eq!(rejected["result"], "document_drifted");
        assert!(rejected["reasons"].as_array().expect("reasons").iter().any(|r| r["kind"] == "approval_changed" && r["code"] == "FSL-DOC-APPROVAL-CHANGED"));
        std::fs::write(target, reviewed).expect("restore exact target");
        assert_eq!(repo.originals(), before);
    }
}

fn approval_diff(repo: &Repo) -> Output {
    run(
        &repo.root,
        &[
            "approval",
            "diff",
            "./spec.fsl",
            "--record",
            WORKING,
            "--depth",
            "2",
            "--trust-key",
            "seed7-public.pem",
        ],
    )
}
#[test]
fn ap7_diff_ignores_live_targets_and_inputs_but_keeps_signature_and_spec_baseline_checks() {
    for version in 3..=6 {
        let repo = Repo::new();
        let record = repo.record(version);
        let before = repo.originals();
        normal(&repo, &record);
        let baseline = checked(&approval_diff(&repo), 0);
        let mut removed = Vec::new();
        for name in [INPUTS[0], INPUTS[1], INPUTS[2], "requirements.md"] {
            let path = repo.root.join(name);
            removed.push((path.clone(), std::fs::read(&path).expect("live bytes")));
            std::fs::remove_file(path).expect("remove live target/input");
        }
        repo.put(&wrong_raw(&record, &[0, 1, 2]));
        same(&approval_diff(&repo), 0, &baseline);
        let (bad_baseline, _) = drifted(&record, 1);
        repo.put(&bad_baseline);
        let semantic = checked(&approval_diff(&repo), 2);
        assert_eq!(semantic["kind"], "semantics");
        repo.put(&wrong_raw(&bad_baseline, &[0]));
        same(&approval_diff(&repo), 2, &semantic);
        if record.get("signature").is_some() {
            let mut bad = record.clone();
            resign(&mut bad);
            corrupt_signature(&mut bad);
            repo.put_raw(WORKING, &bad);
            let invalid = checked(&approval_diff(&repo), 1);
            assert_eq!(invalid["status"], "signature-invalid");
            let mut bad_mismatch = wrong_raw(&record, &[0, 1, 2]);
            resign(&mut bad_mismatch);
            corrupt_signature(&mut bad_mismatch);
            repo.put_raw(WORKING, &bad_mismatch);
            same(&approval_diff(&repo), 1, &invalid);
        }
        for (path, bytes) in removed {
            std::fs::write(path, bytes).expect("restore live bytes");
        }
        repo.put(&record);
        same(&approval_diff(&repo), 0, &baseline);
        assert_eq!(repo.originals(), before);
    }
}
