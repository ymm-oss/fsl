// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Per-kind approval renderer versions (`target.generator_version`).
//!
//! New records write `"<renderer-id>/<semver>"` from one constant per target
//! kind. A record whose value is a bare crate version (`^\d+\.\d+\.\d+$`,
//! written before renderer versions existed) skips only the
//! `renderer_changed` comparison; every digest binding still applies, and
//! any other value is compared exactly.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    fslc_cli::command()
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
}

fn json_success(cwd: &Path, arguments: &[&str]) -> Value {
    let output = run(cwd, arguments);
    assert_eq!(
        output.status.code(),
        Some(0),
        "command {arguments:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

/// A test-owned Git repository holding the #190 approval fixture as
/// `spec.fsl`; removed on drop, including after a failed assertion.
struct Repo {
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-approval-renderer-{}-{}",
            std::process::id(),
            NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed),
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove stale temporary repository");
        }
        std::fs::create_dir_all(&root).expect("create temporary repository");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/approval.fsl"),
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
        Self { root }
    }

    /// Render the reviewed artifact of `kind` and approve it; returns the record.
    fn approve(&self, kind: &str) -> Value {
        let root = &self.root;
        let artifact = match kind {
            "ledger" => {
                json_success_or_verified(root, &["ledger", "spec.fsl", "--depth", "2", "-o", "a"]);
                "a"
            }
            "html" => {
                json_success_or_verified(
                    root,
                    &[
                        "html",
                        "spec.fsl",
                        "--depth",
                        "2",
                        "--deadlock",
                        "ignore",
                        "-o",
                        "a",
                    ],
                );
                "a"
            }
            "scenarios" => {
                let output = run(
                    root,
                    &[
                        "scenarios",
                        "spec.fsl",
                        "--depth",
                        "2",
                        "--deadlock",
                        "ignore",
                    ],
                );
                assert_eq!(output.status.code(), Some(0), "scenarios");
                std::fs::write(root.join("a"), &output.stdout).expect("write scenarios");
                "a"
            }
            "requirements_document" => {
                json_success(
                    root,
                    &[
                        "document", "generate", "spec.fsl", "--lang", "ja", "-o", "a",
                    ],
                );
                "a"
            }
            other => panic!("unknown kind {other}"),
        };
        let mut arguments = vec![
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            kind,
            "--artifact",
            artifact,
            "--approver",
            "alice",
            "-o",
            "approval.json",
        ];
        if kind != "requirements_document" {
            arguments.extend(["--depth", "2"]);
        }
        if matches!(kind, "html" | "scenarios") {
            arguments.extend(["--deadlock", "ignore"]);
        }
        json_success(root, &arguments);
        serde_json::from_slice(&std::fs::read(root.join("approval.json")).expect("read record"))
            .expect("record JSON")
    }

    fn check(&self, record: &Value) -> Value {
        std::fs::write(
            self.root.join("changed.json"),
            serde_json::to_vec_pretty(record).expect("encode record"),
        )
        .expect("write record");
        json_success(
            &self.root,
            &["approval", "check", "spec.fsl", "--record", "changed.json"],
        )
    }
}

fn json_success_or_verified(cwd: &Path, arguments: &[&str]) {
    let output = run(cwd, arguments);
    assert_eq!(
        output.status.code(),
        Some(0),
        "command {arguments:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn with_generator_version(record: &Value, value: &str) -> Value {
    let mut changed = record.clone();
    changed["target"]["generator_version"] = json!(value);
    changed
}

fn assert_status(checked: &Value, status: &str, reasons: &Value, label: &str) {
    assert_eq!(checked["status"], status, "{label}: {checked}");
    assert_eq!(checked["reasons"], *reasons, "{label}: {checked}");
}

/// Guard for each kind's renderer constant: pins the recorded
/// `generator_version` together with the record's artifact digest of a fixed
/// render. When the rendered bytes change, this fails and forces a deliberate
/// decision: bump that kind's renderer version if the presentation changed,
/// or update only the pinned digest if a verifier result changed the bytes.
/// It cannot enforce the bump itself — both edits make it pass again.
fn assert_pinned(kind: &str, generator_version: &str, digest: &str) {
    for round in 0..2 {
        let repo = Repo::new();
        let record = repo.approve(kind);
        assert_eq!(
            (
                record["target"]["generator_version"].as_str(),
                record["target"]["digest"].as_str()
            ),
            (Some(generator_version), Some(digest)),
            "{kind} render (round {round}) moved: bump its renderer version if the presentation \
             changed, otherwise update only the pinned digest"
        );
        let checked = repo.check(&record);
        assert_status(&checked, "approved", &json!([]), kind);
    }
}

#[test]
fn ledger_renderer_version_is_pinned_with_its_render_digest() {
    assert_pinned(
        "ledger",
        "fslc-ledger-renderer/1.0.0",
        "sha256:b5d4f574b898365072645269680016b5dab41158ecc2945ce22d4565797157f7",
    );
}

#[test]
fn html_renderer_version_is_pinned_with_its_render_digest() {
    assert_pinned(
        "html",
        "fslc-html-renderer/1.0.0",
        "sha256:8db000a3eea6fde244a1dd00d8130f8244268210a38f496ea79cbd333ff100e0",
    );
}

#[test]
fn scenarios_renderer_version_is_pinned_with_its_render_digest() {
    assert_pinned(
        "scenarios",
        "fslc-scenarios-renderer/1.0.0",
        "sha256:ef2e880d98567185daa491e894cf626af57ff97279583c8c7992180490ed8f3c",
    );
}

#[test]
fn requirements_document_renderer_version_is_pinned_with_its_render_digest() {
    assert_pinned(
        "requirements_document",
        "fslc-document-renderer/1.3.0",
        "sha256:0572212e853af2f44211720eed85908dbe32cce909ef2e7c31cefd562f185e49",
    );
}

#[test]
fn legacy_crate_version_with_matching_digests_is_approved() {
    for kind in ["ledger", "requirements_document"] {
        let repo = Repo::new();
        let record = repo.approve(kind);
        for legacy in ["4.12.0", "4.7.0", "0.0.0", env!("CARGO_PKG_VERSION")] {
            let checked = repo.check(&with_generator_version(&record, legacy));
            assert_status(
                &checked,
                "approved",
                &json!([]),
                &format!("{kind} {legacy}"),
            );
        }
    }
}

#[test]
fn legacy_crate_version_with_changed_rendering_is_drifted() {
    for kind in ["ledger", "requirements_document"] {
        let repo = Repo::new();
        let record = repo.approve(kind);
        let mut changed = with_generator_version(&record, "4.12.0");
        changed["target"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        let checked = repo.check(&changed);
        assert_status(
            &checked,
            "drifted",
            &json!(["rendering_changed"]),
            &format!("{kind} legacy rendering"),
        );
    }
}

#[test]
fn new_form_with_another_renderer_id_or_version_is_renderer_changed() {
    let repo = Repo::new();
    let record = repo.approve("ledger");
    for other in [
        "fslc-ledger-renderer/0.9.0",
        "fslc-ledger-renderer/1.0.1",
        "fslc-html-renderer/1.0.0",
        "fslc-document-renderer/1.3.0",
    ] {
        let checked = repo.check(&with_generator_version(&record, other));
        assert_status(&checked, "drifted", &json!(["renderer_changed"]), other);
    }
}

#[test]
fn semver_lookalikes_are_still_compared() {
    let repo = Repo::new();
    let record = repo.approve("scenarios");
    for lookalike in [
        "4.12",
        "v4.12.0",
        "4.12.0-rc.1",
        "4.12.0.1",
        " 4.12.0",
        "4.12.0 ",
        "4..12.0",
        "4.12.x",
        "４.12.0",
    ] {
        let checked = repo.check(&with_generator_version(&record, lookalike));
        assert_status(
            &checked,
            "drifted",
            &json!(["renderer_changed"]),
            &format!("{lookalike:?}"),
        );
    }
}
