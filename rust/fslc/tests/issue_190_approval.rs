// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
