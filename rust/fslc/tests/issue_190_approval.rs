// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::{Value, json};

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

fn successful(cwd: &Path, arguments: &[&str]) -> Output {
    let output = run(cwd, arguments);
    assert!(
        output.status.success(),
        "command {arguments:?} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
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
    serde_json::from_slice(&output.stdout).expect("JSON command output")
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

fn git_stdout(cwd: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {arguments:?} failed");
    String::from_utf8(output.stdout)
        .expect("Git output is UTF-8")
        .trim()
        .to_owned()
}

fn approval_repo() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!(
        "fslc-issue-190-{}-{nonce}-{}",
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

fn normalize_digests(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"sha256:")
            && index + 71 <= bytes.len()
            && bytes[index + 7..index + 71]
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            output.extend_from_slice(b"sha256:<digest>");
            index += 71;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).expect("normalized ledger remains UTF-8")
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

#[test]
#[allow(clippy::too_many_lines)]
fn approval_record_reports_approved_then_drifted_and_drives_semantic_diff() {
    let root = approval_repo();
    successful(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    let created = json_output(&successful(
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
            "--depth",
            "2",
            "-o",
            "approval.json",
        ],
    ));
    let baseline = created["record"]["spec"]["digest"]
        .as_str()
        .expect("baseline digest")
        .to_owned();
    assert_eq!(
        created["record"]["approval"]["requirements"],
        json!(["REQ-190"])
    );

    let approved = json_output(&successful(
        &root,
        &["approval", "check", "spec.fsl", "--record", "approval.json"],
    ));
    assert_eq!(approved["status"], "approved");
    assert_eq!(approved["signature_status"], "unsigned");
    assert_eq!(approved["reasons"], json!([]));

    let mut invalid_record: Value = serde_json::from_slice(
        &std::fs::read(root.join("approval.json")).expect("read approval record"),
    )
    .expect("approval record JSON");
    invalid_record["schema"] = json!("fslc.approval.v999");
    std::fs::write(
        root.join("unsupported.json"),
        serde_json::to_vec_pretty(&invalid_record).expect("serialize unsupported record"),
    )
    .expect("write unsupported record");
    let unsupported = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "unsupported.json",
        ],
    );
    assert_eq!(unsupported["kind"], "io");

    invalid_record["schema"] = json!("fslc.approval.v1");
    invalid_record["spec"]["digest"] = json!("sha256:not-a-digest");
    std::fs::write(
        root.join("malformed.json"),
        serde_json::to_vec_pretty(&invalid_record).expect("serialize malformed record"),
    )
    .expect("write malformed record");
    let malformed = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "malformed.json",
        ],
    );
    assert_eq!(malformed["kind"], "io");

    let mut unknown_field = created["record"].clone();
    unknown_field["unexpected"] = json!(true);
    std::fs::write(
        root.join("unknown-field.json"),
        serde_json::to_vec_pretty(&unknown_field).expect("serialize unknown-field record"),
    )
    .expect("write unknown-field record");
    let unknown_field = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "unknown-field.json",
        ],
    );
    assert_eq!(unknown_field["kind"], "io");

    let mut missing_baseline = created["record"].clone();
    missing_baseline["spec"]["git_commit"] = json!("0".repeat(40));
    std::fs::write(
        root.join("missing-baseline.json"),
        serde_json::to_vec_pretty(&missing_baseline).expect("serialize missing baseline"),
    )
    .expect("write missing-baseline record");
    let missing_baseline = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "missing-baseline.json",
        ],
    );
    assert_eq!(missing_baseline["kind"], "io");

    successful(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--approval",
            "approval.json",
            "-o",
            "approved.md",
        ],
    );
    let ledger = std::fs::read_to_string(root.join("approved.md")).expect("approved ledger");
    let risk = ledger
        .split_once("## リスク一覧（要件ID別）")
        .expect("risk table")
        .1
        .split_once("## 要件ID別詳細")
        .expect("risk table end")
        .0;
    let approval = ledger
        .split_once("## 承認照合")
        .expect("approval section")
        .1
        .split_once("## 付録")
        .expect("approval section end")
        .0;
    let snapshot = format!("## リスク一覧（要件ID別）{risk}## 承認照合{approval}");
    assert_eq!(
        normalize_digests(&snapshot).trim(),
        include_str!("../../../tests/snapshots/approval_ledger.md").trim()
    );

    let original = std::fs::read_to_string(root.join("spec.fsl")).expect("read fixture");
    std::fs::write(
        root.join("spec.fsl"),
        format!("{original}\n// review-only comment\n"),
    )
    .expect("append non-semantic comment");
    let comment_only = json_output(&successful(
        &root,
        &["approval", "check", "spec.fsl", "--record", "approval.json"],
    ));
    assert_eq!(comment_only["status"], "approved");

    let mut renderer_record: Value = serde_json::from_slice(
        &std::fs::read(root.join("approval.json")).expect("read approval record"),
    )
    .expect("approval record JSON");
    renderer_record["target"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    std::fs::write(
        root.join("renderer-drift.json"),
        serde_json::to_vec_pretty(&renderer_record).expect("serialize drift record"),
    )
    .expect("write drift record");
    let renderer_drift = json_output(&successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "renderer-drift.json",
        ],
    ));
    assert_eq!(renderer_drift["status"], "drifted");
    assert_eq!(renderer_drift["reasons"], json!(["rendering_changed"]));

    let changed = original.replace("approved = true", "approved = false");
    std::fs::write(root.join("spec.fsl"), &changed).expect("write semantic change");
    let drifted = json_output(&successful(
        &root,
        &["approval", "check", "spec.fsl", "--record", "approval.json"],
    ));
    assert_eq!(drifted["status"], "drifted");
    assert!(
        drifted["reasons"]
            .as_array()
            .expect("drift reasons")
            .contains(&json!("spec_changed"))
    );
    assert_eq!(drifted["baseline_digest"], baseline);

    let diff = json_output(&successful(
        &root,
        &[
            "approval",
            "diff",
            "spec.fsl",
            "--record",
            "approval.json",
            "--depth",
            "2",
        ],
    ));
    assert_eq!(diff["result"], "semantic_diff");
    assert_eq!(diff["approval"]["baseline_digest"], baseline);
    assert_ne!(diff["summary"], json!(["no_semantic_change"]));

    git(&root, &["add", "spec.fsl"]);
    git(&root, &["commit", "-qm", "semantic change"]);
    let mut mismatched_baseline: Value = serde_json::from_slice(
        &std::fs::read(root.join("approval.json")).expect("read approval record"),
    )
    .expect("approval record JSON");
    mismatched_baseline["spec"]["git_commit"] = json!(git_stdout(&root, &["rev-parse", "HEAD"]));
    std::fs::write(
        root.join("mismatched-baseline.json"),
        serde_json::to_vec_pretty(&mismatched_baseline).expect("serialize mismatched baseline"),
    )
    .expect("write mismatched baseline record");
    let mismatch = failed(
        &root,
        &[
            "approval",
            "diff",
            "spec.fsl",
            "--record",
            "mismatched-baseline.json",
            "--depth",
            "2",
        ],
    );
    assert_eq!(mismatch["kind"], "semantics");

    let without_requirement_ids = changed
        .replace(" \"REQ-190: a pending review can be approved\"", "")
        .replace(" \"REQ-190: an approved review remains approved\"", "");
    std::fs::write(root.join("spec.fsl"), without_requirement_ids)
        .expect("remove requirement IDs from current spec");
    successful(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--approval",
            "approval.json",
            "-o",
            "removed-requirement.md",
        ],
    );
    let removed = std::fs::read_to_string(root.join("removed-requirement.md"))
        .expect("removed requirement ledger");
    assert!(removed.contains("REQ-190"));
    assert!(removed.contains("現行仕様に要件IDなし"));
    assert!(removed.contains("drifted"));

    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
#[allow(clippy::too_many_lines)]
fn signed_v2_records_require_a_matching_trust_anchor_and_fail_closed() {
    let root = approval_repo();
    let (private, public) = write_signing_keys(&root, 7, "approval");
    let (_, wrong_public) = write_signing_keys(&root, 9, "wrong");
    successful(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    let created = json_output(&successful(
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
            "--depth",
            "2",
            "--signing-key",
            private.to_str().expect("private path"),
            "-o",
            "signed.json",
        ],
    ));
    assert_eq!(created["record"]["schema"], "fslc.approval.v2");
    assert_eq!(created["record"]["signature"]["algorithm"], "ed25519");

    let missing_trust = failed(
        &root,
        &["approval", "check", "spec.fsl", "--record", "signed.json"],
    );
    assert_eq!(missing_trust["kind"], "io");

    let checked = json_output(&successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "signed.json",
            "--trust-key",
            public.to_str().expect("public path"),
        ],
    ));
    assert_eq!(checked["status"], "approved");
    assert_eq!(checked["signature_status"], "signed");

    let wrong_trust = failed(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "signed.json",
            "--trust-key",
            wrong_public.to_str().expect("wrong public path"),
        ],
    );
    assert_eq!(wrong_trust["kind"], "io");

    let record = created["record"].clone();
    let mut reordered = serde_json::Map::new();
    for key in ["signature", "approval", "target", "spec", "schema"] {
        reordered.insert(key.to_owned(), record[key].clone());
    }
    std::fs::write(
        root.join("reordered.json"),
        serde_json::to_vec_pretty(&Value::Object(reordered)).expect("serialize reordered record"),
    )
    .expect("write reordered record");
    successful(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "reordered.json",
            "--trust-key",
            public.to_str().expect("public path"),
        ],
    );

    let mut tampered = record;
    tampered["approval"]["approver"] = json!("mallory");
    std::fs::write(
        root.join("tampered.json"),
        serde_json::to_vec_pretty(&tampered).expect("serialize tampered record"),
    )
    .expect("write tampered record");
    let invalid_output = run(
        &root,
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "tampered.json",
            "--trust-key",
            public.to_str().expect("public path"),
        ],
    );
    assert_eq!(invalid_output.status.code(), Some(1));
    let invalid = json_output(&invalid_output);
    assert_eq!(invalid["status"], "signature-invalid");
    assert_eq!(invalid["signature_status"], "signature-invalid");

    successful(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--approval",
            "tampered.json",
            "--trust-key",
            public.to_str().expect("public path"),
            "-o",
            "invalid-ledger.md",
        ],
    );
    let invalid_ledger =
        std::fs::read_to_string(root.join("invalid-ledger.md")).expect("invalid ledger");
    assert!(invalid_ledger.contains("❌ signature-invalid"));
    assert!(!invalid_ledger.contains("✅ approved"));

    successful(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--approval",
            "signed.json",
            "--trust-key",
            public.to_str().expect("public path"),
            "-o",
            "signed-ledger.md",
        ],
    );
    let ledger = std::fs::read_to_string(root.join("signed-ledger.md")).expect("signed ledger");
    assert!(ledger.contains("✅ approved (signed)"));

    let diff = json_output(&successful(
        &root,
        &[
            "approval",
            "diff",
            "spec.fsl",
            "--record",
            "signed.json",
            "--depth",
            "2",
            "--trust-key",
            public.to_str().expect("public path"),
        ],
    ));
    assert_eq!(diff["approval"]["signature_status"], "signed");

    let original = std::fs::read_to_string(root.join("spec.fsl")).expect("read fixture");
    std::fs::write(
        root.join("spec.fsl"),
        original.replace("approved = true", "approved = false"),
    )
    .expect("write semantic change");
    successful(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--approval",
            "signed.json",
            "--trust-key",
            public.to_str().expect("public path"),
            "-o",
            "signed-drift-ledger.md",
        ],
    );
    let signed_drift =
        std::fs::read_to_string(root.join("signed-drift-ledger.md")).expect("signed drift ledger");
    assert!(signed_drift.contains("⚠ drifted (signed; since sha256:"));

    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn html_and_scenarios_are_supported_review_targets() {
    let root = approval_repo();
    for (kind, artifact, render_arguments) in [
        (
            "html",
            "review.html",
            vec![
                "html",
                "spec.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
                "-o",
                "review.html",
            ],
        ),
        (
            "scenarios",
            "scenarios.json",
            vec![
                "scenarios",
                "spec.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ],
        ),
    ] {
        let rendered = successful(&root, &render_arguments);
        if kind == "scenarios" {
            std::fs::write(root.join(artifact), &rendered.stdout)
                .expect("write scenarios artifact");
        }
        let record = format!("{kind}.approval.json");
        successful(
            &root,
            &[
                "approval",
                "create",
                "spec.fsl",
                "--kind",
                kind,
                "--artifact",
                artifact,
                "--approver",
                "alice",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
                "-o",
                &record,
            ],
        );
        let checked = json_output(&successful(
            &root,
            &["approval", "check", "spec.fsl", "--record", &record],
        ));
        assert_eq!(checked["status"], "approved", "target {kind}");
    }
    std::fs::remove_dir_all(root).expect("remove temporary repository");
}

#[test]
fn approval_creation_rejects_stale_unknown_dirty_and_unidentified_inputs() {
    let root = approval_repo();
    successful(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );

    std::fs::write(root.join("review.md"), "stale review\n").expect("write stale review");
    let stale = failed(
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
            "--depth",
            "2",
        ],
    );
    assert_eq!(stale["kind"], "semantics");

    successful(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    let unknown = failed(
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
            "--requirement",
            "REQ-UNKNOWN",
            "--depth",
            "2",
        ],
    );
    assert_eq!(unknown["kind"], "semantics");

    let source = std::fs::read_to_string(root.join("spec.fsl")).expect("read spec");
    std::fs::write(root.join("spec.fsl"), format!("{source}\n// dirty\n"))
        .expect("dirty tracked spec");
    let dirty = failed(
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
            "--depth",
            "2",
        ],
    );
    assert_eq!(dirty["kind"], "io");

    std::fs::write(
        root.join("spec.fsl"),
        source
            .replace(" \"REQ-190: a pending review can be approved\"", "")
            .replace(" \"REQ-190: an approved review remains approved\"", ""),
    )
    .expect("remove requirement IDs");
    git(&root, &["add", "spec.fsl"]);
    git(&root, &["commit", "-qm", "remove requirement IDs"]);
    successful(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    let unidentified = failed(
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
            "--depth",
            "2",
        ],
    );
    assert_eq!(unidentified["kind"], "semantics");

    std::fs::remove_dir_all(root).expect("remove temporary repository");
}
