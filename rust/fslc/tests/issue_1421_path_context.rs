// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Independent AP1/AP2 acceptance for #1421's adopted creation-directory contract.
//! Exercise the public CLI; schemas are loaded at runtime so the baseline compiles
//! before v5/v6 exist. Legacy signature compatibility and old-binary rejection are
//! separate controls, not simulated by this test.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);
const SPEC_GIT_PATH: &str = "review/spec.fsl";
const ROOT_SOURCE: &str = "./review/./spec.fsl";
const REVIEW_SOURCE: &str = "./spec.fsl";
const DECOY_SOURCE: &str = "./decoy.fsl";
const ARTIFACT: &str = "./requirements.md";
const EVIDENCE: [&str; 2] = ["./evidence-b.json", "nested/../evidence-a.json"];
const SLOT: &str = "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）";

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    fslc_cli::command()
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
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

fn successful(cwd: &Path, arguments: &[&str]) -> Value {
    let output = run(cwd, arguments);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{arguments:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json_output(&output)
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .env("GIT_AUTHOR_DATE", "2026-10-27T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-27T00:00:00Z")
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct ApprovalRepo {
    temporary: PathBuf,
    root: PathBuf,
    review: PathBuf,
    unrelated: PathBuf,
    private: PathBuf,
    public: PathBuf,
}

impl Drop for ApprovalRepo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.temporary).expect("remove temporary approval repository");
    }
}

impl ApprovalRepo {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temporary = std::env::temp_dir().join(format!(
            "fslc-issue-1421-{}-{nonce}-{}",
            std::process::id(),
            NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&temporary).expect("create temporary directory");
        // Only the test-owned fixture base is made physical, matching native
        // getcwd. Record arguments below keep their literal ./ and ../ forms.
        let temporary = temporary
            .canonicalize()
            .expect("physical temporary directory");
        let root = temporary.join("repo");
        let review = root.join("review");
        let unrelated = temporary.join("unrelated");
        std::fs::create_dir_all(&review).expect("create review directory");
        std::fs::create_dir_all(&unrelated).expect("create unrelated directory");
        let source = std::fs::read_to_string(repository_file("tests/fixtures/approval.fsl"))
            .expect("read approval fixture");
        std::fs::write(root.join(SPEC_GIT_PATH), source.replace("\r\n", "\n"))
            .expect("write approval fixture");
        git(&root, &["init", "-q"]);
        git(
            &root,
            &["config", "user.email", "approval-test@example.com"],
        );
        git(&root, &["config", "user.name", "Approval Test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        git(&root, &["add", SPEC_GIT_PATH]);
        git(&root, &["commit", "-qm", "approval baseline"]);
        let (private, public) = write_signing_keys(&root);
        Self {
            temporary,
            root,
            review,
            unrelated,
            private,
            public,
        }
    }

    fn check_output(&self, cwd: &Path, record: &Path, signed: bool) -> Output {
        let spec = self.root.join(SPEC_GIT_PATH);
        let mut args = vec![
            "approval",
            "check",
            spec.to_str().expect("UTF-8 spec path"),
            "--record",
            record.to_str().expect("UTF-8 record path"),
        ];
        if signed {
            args.extend([
                "--trust-key",
                self.public.to_str().expect("UTF-8 public key"),
            ]);
        }
        run(cwd, &args)
    }

    fn check(&self, cwd: &Path, record: &Path, signed: bool) -> Value {
        let output = self.check_output(cwd, record, signed);
        assert_eq!(
            output.status.code(),
            Some(0),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        json_output(&output)
    }
}

#[allow(clippy::default_trait_access)]
fn write_signing_keys(root: &Path) -> (PathBuf, PathBuf) {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let private = root.join("alice-private.pem");
    let public = root.join("alice-public.pem");
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

fn write_inputs(cwd: &Path, decoy: bool) {
    std::fs::create_dir_all(cwd.join("nested")).expect("create literal dotdot input directory");
    let label = if decoy {
        "別の承認"
    } else {
        "承認する"
    };
    let glossary = json!({
        "schema":"fslc.document-glossary.v1", "locale":"ja",
        "labels":{"action:approve":label}
    });
    std::fs::write(cwd.join("glossary.json"), glossary.to_string()).expect("write glossary");
    let evidence = if decoy {
        [
            ("evidence-b.json", "bounded"),
            ("evidence-a.json", "unbounded"),
        ]
    } else {
        [
            ("evidence-b.json", "unbounded"),
            ("evidence-a.json", "bounded"),
        ]
    };
    for (name, completeness) in evidence {
        std::fs::write(
            cwd.join(name),
            json!({"requirement":{"id":"REQ-190"},"completeness":completeness,"depth":2})
                .to_string(),
        )
        .expect("write evidence");
    }
}

fn generate_document(cwd: &Path, source: &str) -> Value {
    successful(
        cwd,
        &[
            "document",
            "generate",
            source,
            "--lang",
            "ja",
            "--glossary",
            "glossary.json",
            "--evidence",
            EVIDENCE[0],
            "--evidence",
            EVIDENCE[1],
            "-o",
            ARTIFACT,
        ],
    )
}

fn generate_document_decoy(repo: &ApprovalRepo, cwd: &Path) {
    std::fs::copy(repo.root.join(SPEC_GIT_PATH), cwd.join(DECOY_SOURCE))
        .expect("copy caller-local document decoy spec");
    write_inputs(cwd, true);
    generate_document(cwd, DECOY_SOURCE);
}

fn create(
    repo: &ApprovalRepo,
    cwd: &Path,
    source: &str,
    kind: &str,
    artifact: &str,
    signed: bool,
) -> (Value, PathBuf) {
    let name = format!(
        "{kind}-{}.approval.json",
        if signed { "signed" } else { "unsigned" }
    );
    let mut args = vec![
        "approval",
        "create",
        source,
        "--kind",
        kind,
        "--artifact",
        artifact,
        "--approver",
        "alice",
        "-o",
        &name,
    ];
    if kind == "requirements_document" {
        args.extend([
            "--glossary",
            "glossary.json",
            "--evidence",
            EVIDENCE[0],
            "--evidence",
            EVIDENCE[1],
        ]);
    } else {
        args.extend(["--depth", "2", "--deadlock", "ignore"]);
    }
    if signed {
        args.extend([
            "--signing-key",
            repo.private.to_str().expect("UTF-8 private key"),
        ]);
    }
    let reviewed_bytes = std::fs::read(cwd.join(artifact))
        .expect("snapshot reviewed artifact before approval create");
    let created = successful(cwd, &args);
    assert_eq!(
        std::fs::read(cwd.join(artifact)).expect("read reviewed artifact after approval create"),
        reviewed_bytes,
        "approval creation preserves exact reviewed {kind} bytes"
    );
    let record_path = cwd.join(name);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&record_path).expect("read record"))
        .expect("parse record");
    assert_eq!(created["record"], on_disk, "returned and published record");
    (on_disk, record_path)
}

fn schema(version: u8) -> jsonschema::Validator {
    let relative = format!("schemas/fslc/approval/approval-record.v{version}.schema.json");
    let text = std::fs::read_to_string(repository_file(&relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
    let value: Value = serde_json::from_str(&text).expect("schema is valid JSON");
    jsonschema::validator_for(&value).expect("schema compiles")
}

fn stored_base(repo: &ApprovalRepo, cwd: &Path) -> &'static str {
    if cwd == repo.root { "." } else { "review" }
}

fn assert_new_record(record: &Value, base: &str, source: &str, artifact: &str, signed: bool) {
    assert_eq!(
        record["schema"],
        if signed {
            "fslc.approval.v6"
        } else {
            "fslc.approval.v5"
        }
    );
    assert_eq!(record["path_base"], base);
    assert_eq!(record["render_source"], source);
    assert_eq!(record["spec"]["path"], SPEC_GIT_PATH);
    assert_eq!(record["target"]["path"], artifact);
    let validator = schema(if signed { 6 } else { 5 });
    let errors: Vec<String> = validator
        .iter_errors(record)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:?}");

    for field in ["path_base", "render_source"] {
        let mut missing = record.clone();
        missing
            .as_object_mut()
            .expect("record object")
            .remove(field);
        assert!(!validator.is_valid(&missing), "schema must require {field}");
        let mut empty = record.clone();
        empty[field] = json!("");
        assert!(
            !validator.is_valid(&empty),
            "schema must reject empty {field}"
        );
    }
    let mut relative = record.clone();
    relative["path_base"] = json!("/absolute/base");
    assert!(
        !validator.is_valid(&relative),
        "schema must require a repository-relative base"
    );
    let mut unknown = record.clone();
    unknown["unexpected"] = json!(true);
    assert!(!validator.is_valid(&unknown), "new record remains closed");
    for field in ["spec", "target", "approval"] {
        let mut nested = record.clone();
        nested[field]["unexpected"] = json!(true);
        assert!(!validator.is_valid(&nested), "{field} remains closed");
    }
    if signed {
        let mut signature_extra = record.clone();
        signature_extra["signature"]["unexpected"] = json!(true);
        assert!(
            !validator.is_valid(&signature_extra),
            "signature remains closed"
        );
    }
    let mut wrong_kind = record.clone();
    wrong_kind["target"]["kind"] = json!("unknown");
    assert!(
        !validator.is_valid(&wrong_kind),
        "target kind remains closed"
    );
    let mut mixed = record.clone();
    if record["target"]["kind"] == "requirements_document" {
        mixed["target"]["inputs"]["depth"] = json!(2);
        let mut file_extra = record.clone();
        file_extra["target"]["inputs"]["glossary"]["base"] = json!(".");
        assert!(!validator.is_valid(&file_extra), "FileInput remains closed");
        let mut evidence_extra = record.clone();
        evidence_extra["target"]["inputs"]["evidence"][0]["base"] = json!(".");
        assert!(
            !validator.is_valid(&evidence_extra),
            "evidence FileInput remains closed"
        );
    } else {
        mixed["target"]["inputs"]["view"] = json!("requirements");
    }
    assert!(
        !validator.is_valid(&mixed),
        "solver/document input branches stay closed"
    );
}

fn snapshot(paths: &[PathBuf]) -> Vec<Vec<u8>> {
    paths
        .iter()
        .map(|path| std::fs::read(path).expect("snapshot file"))
        .collect()
}

fn assert_io(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_output(output);
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "io");
    value
}

fn document_cross_cwd(subdirectory: bool, signed: bool) {
    let repo = ApprovalRepo::new();
    let cwd = if subdirectory {
        &repo.review
    } else {
        &repo.root
    };
    let source = if subdirectory {
        REVIEW_SOURCE
    } else {
        ROOT_SOURCE
    };
    write_inputs(cwd, false);
    let generated = generate_document(cwd, source);
    let canonical = std::fs::read(cwd.join(ARTIFACT)).expect("read canonical document");
    let text = String::from_utf8(canonical).expect("UTF-8 document");
    assert!(
        text.contains(&format!("source: {source}\n")),
        "literal source label"
    );
    for evidence in EVIDENCE {
        assert!(text.contains(evidence), "literal evidence label {evidence}");
    }
    assert!(text.contains(SLOT), "known editable slot fixture");
    let reviewed = text.replace(SLOT, "本プロジェクト固有の背景説明。");
    std::fs::write(cwd.join(ARTIFACT), reviewed.as_bytes()).expect("write reviewed slot");
    let (record, record_path) = create(
        &repo,
        cwd,
        source,
        "requirements_document",
        ARTIFACT,
        signed,
    );
    assert_new_record(&record, stored_base(&repo, cwd), source, ARTIFACT, signed);
    assert_eq!(record["target"]["digest"], generated["artifact_digest"]);
    assert_eq!(
        record["target"]["reviewed_digest"],
        format!("sha256:{:x}", Sha256::digest(reviewed.as_bytes()))
    );
    assert_ne!(
        record["target"]["digest"],
        record["target"]["reviewed_digest"]
    );
    assert_eq!(
        record["target"]["inputs"]["glossary"]["path"],
        "glossary.json"
    );
    assert_eq!(
        record["target"]["inputs"]["evidence"]
            .as_array()
            .expect("evidence array")
            .iter()
            .map(|input| input["path"].as_str().expect("evidence path"))
            .collect::<Vec<_>>(),
        EVIDENCE
    );
    assert!(
        !reviewed.contains(cwd.to_str().expect("UTF-8 CWD")),
        "no resolved I/O labels"
    );
    let baseline = repo.check(cwd, &record_path, signed);
    assert_eq!(baseline["status"], "approved");
    assert_eq!(
        baseline["signature_status"],
        if signed { "signed" } else { "unsigned" }
    );
    assert_eq!(baseline["reasons"], json!([]));

    // These are well-formed alternate artifacts and parseable alternate inputs,
    // rather than absent files that an accidental CWD read would reject anyway.
    let spec = repo.root.join(SPEC_GIT_PATH);
    for caller in [&repo.root, &repo.review, &repo.unrelated] {
        if caller != cwd {
            generate_document_decoy(&repo, caller);
            for input in ["glossary.json", EVIDENCE[0], EVIDENCE[1]] {
                assert_ne!(
                    std::fs::read(caller.join(input)).expect("read decoy input"),
                    std::fs::read(cwd.join(input)).expect("read original input"),
                    "each offered decoy input is byte-distinct: {input}"
                );
            }
            assert_ne!(
                std::fs::read(caller.join(ARTIFACT)).expect("read decoy"),
                text.as_bytes(),
                "decoy rendering differs from the original canonical document"
            );
            assert_ne!(
                std::fs::read(caller.join(ARTIFACT)).expect("read decoy"),
                reviewed.as_bytes()
            );
        }
    }
    assert_document_checks_and_missing_files(&repo, cwd, &record_path, signed, &baseline, spec);
}

fn assert_document_checks_and_missing_files(
    repo: &ApprovalRepo,
    cwd: &Path,
    record_path: &Path,
    signed: bool,
    baseline: &Value,
    spec: PathBuf,
) {
    let mut paths = vec![spec, record_path.to_path_buf()];
    for directory in [&repo.root, &repo.review, &repo.unrelated] {
        for name in [ARTIFACT, "glossary.json", EVIDENCE[0], EVIDENCE[1]] {
            paths.push(directory.join(name));
        }
        if directory != cwd {
            paths.push(directory.join(DECOY_SOURCE));
        }
    }
    let before = snapshot(&paths);
    for caller in [&repo.root, &repo.review, &repo.unrelated] {
        assert_eq!(
            repo.check(caller, record_path, signed),
            *baseline,
            "full cross-CWD JSON"
        );
    }
    assert_eq!(
        snapshot(&paths),
        before,
        "checks preserve all original and decoy bytes"
    );

    // Each missing real file is tested independently with the same valid decoys.
    for name in ["glossary.json", EVIDENCE[0], EVIDENCE[1], ARTIFACT] {
        let path = cwd.join(name);
        let bytes = std::fs::read(&path).expect("read original file");
        std::fs::remove_file(&path).expect("remove original file");
        assert_io(&repo.check_output(&repo.unrelated, record_path, signed));
        std::fs::write(&path, bytes).expect("restore original file");
        assert_eq!(
            repo.check(&repo.unrelated, record_path, signed),
            *baseline,
            "restored positive control"
        );
    }
    assert_eq!(
        snapshot(&paths),
        before,
        "missing-file checks preserve other bytes"
    );
}

#[test]
fn ap1_ap2_unsigned_document_created_at_root_uses_original_base_and_literal_labels() {
    document_cross_cwd(false, false);
}

#[test]
fn ap1_ap2_unsigned_document_created_in_subdirectory_uses_original_base_and_literal_labels() {
    document_cross_cwd(true, false);
}

#[test]
fn ap1_ap2_signed_document_created_at_root_uses_original_base_and_literal_labels() {
    document_cross_cwd(false, true);
}

#[test]
fn ap1_ap2_signed_document_created_in_subdirectory_uses_original_base_and_literal_labels() {
    document_cross_cwd(true, true);
}

fn generate_solver(cwd: &Path, source: &str, kind: &str, artifact: &str) {
    let mut args = vec![kind, source, "--depth", "2", "--deadlock", "ignore"];
    if kind != "scenarios" {
        args.extend(["-o", artifact]);
    }
    let output = run(cwd, &args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if kind == "scenarios" {
        std::fs::write(cwd.join(artifact), output.stdout).expect("write scenarios artifact");
    }
}

fn generate_solver_decoy(repo: &ApprovalRepo, cwd: &Path, kind: &str, artifact: &str) {
    let source =
        std::fs::read_to_string(repo.root.join(SPEC_GIT_PATH)).expect("read original solver spec");
    std::fs::write(
        cwd.join(DECOY_SOURCE),
        source
            .replace("spec ApprovalReview {", "spec DecoyApprovalReview {")
            .replace("action approve()", "action decoy_approve()")
            .replace("REQ-190", "REQ-1421-DECOY"),
    )
    .expect("write alternate solver spec");
    generate_solver(cwd, DECOY_SOURCE, kind, artifact);
}

#[test]
fn ap0_existing_generation_accepts_fixture_sources_and_distinct_decoys() {
    // This positive control has no approval or future-schema assertions. It
    // exposes setup/admission failures independently on the unchanged binary.
    for subdirectory in [false, true] {
        let repo = ApprovalRepo::new();
        let (cwd, source) = if subdirectory {
            (&repo.review, REVIEW_SOURCE)
        } else {
            (&repo.root, ROOT_SOURCE)
        };
        write_inputs(cwd, false);
        generate_document(cwd, source);
        let original = std::fs::read(cwd.join(ARTIFACT)).expect("read original document");
        for caller in [&repo.root, &repo.review, &repo.unrelated] {
            if caller != cwd {
                generate_document_decoy(&repo, caller);
                for input in ["glossary.json", EVIDENCE[0], EVIDENCE[1]] {
                    assert_ne!(
                        std::fs::read(caller.join(input)).expect("read document decoy input"),
                        std::fs::read(cwd.join(input)).expect("read original document input"),
                        "distinct input at {input}"
                    );
                }
                assert_ne!(
                    std::fs::read(caller.join(ARTIFACT)).expect("read document decoy"),
                    original,
                    "distinct document decoy"
                );
            }
        }
        for kind in ["ledger", "html", "scenarios"] {
            let artifact = format!("./{kind}.fixture.review");
            generate_solver(cwd, source, kind, &artifact);
            let original = std::fs::read(cwd.join(&artifact)).expect("read original solver target");
            for caller in [&repo.root, &repo.review, &repo.unrelated] {
                if caller != cwd {
                    generate_solver_decoy(&repo, caller, kind, &artifact);
                    assert_ne!(
                        std::fs::read(caller.join(&artifact)).expect("read solver decoy"),
                        original,
                        "distinct {kind} decoy"
                    );
                }
            }
        }
    }
}

fn solver_cross_cwd(signed: bool) {
    let repo = ApprovalRepo::new();
    for kind in ["ledger", "html", "scenarios"] {
        let (cwd, source) = if kind == "ledger" {
            (&repo.root, ROOT_SOURCE)
        } else {
            (&repo.review, REVIEW_SOURCE)
        };
        let artifact = format!("./{kind}.review");
        generate_solver(cwd, source, kind, &artifact);
        let artifact_path = cwd.join(&artifact);
        let reviewed_bytes = std::fs::read(&artifact_path).expect("read reviewed solver target");
        let (record, record_path) = create(&repo, cwd, source, kind, &artifact, signed);
        assert_new_record(&record, stored_base(&repo, cwd), source, &artifact, signed);
        let baseline = repo.check(cwd, &record_path, signed);
        assert_eq!(baseline["status"], "approved", "{kind}");
        assert_eq!(baseline["target_kind"], kind);
        assert_eq!(
            baseline["signature_status"],
            if signed { "signed" } else { "unsigned" }
        );
        let mut paths = vec![
            repo.root.join(SPEC_GIT_PATH),
            artifact_path.clone(),
            record_path.clone(),
        ];
        let mut decoys = Vec::new();
        for caller in [&repo.root, &repo.review, &repo.unrelated] {
            if caller != cwd {
                // Generate a valid, observably different solver target rather
                // than relying on volatile metadata or the caller's pathname.
                generate_solver_decoy(&repo, caller, kind, &artifact);
                let path = caller.join(&artifact);
                let bytes = std::fs::read(&path).expect("read solver decoy");
                assert_ne!(bytes, reviewed_bytes, "distinct {kind} decoy");
                paths.push(path.clone());
                paths.push(caller.join(DECOY_SOURCE));
                decoys.push((path, bytes));
            }
        }
        let before = snapshot(&paths);
        for caller in [&repo.root, &repo.review, &repo.unrelated] {
            assert_eq!(
                repo.check(caller, &record_path, signed),
                baseline,
                "full {kind} cross-CWD JSON"
            );
        }
        assert_eq!(
            snapshot(&paths),
            before,
            "solver checks preserve all target bytes"
        );

        // Solver checks regenerate from spec/inputs and must never read live
        // original or decoy targets. Documents have a different reviewed-byte
        // contract, covered by the missing-target I/O controls above.
        for (path, _) in &decoys {
            std::fs::write(path, b"mutated decoy target; must not be read\n")
                .expect("mutate solver decoy");
        }
        for caller in [&repo.root, &repo.review, &repo.unrelated] {
            assert_eq!(
                repo.check(caller, &record_path, signed),
                baseline,
                "mutated {kind} decoys do not enter authoritative evaluation"
            );
        }
        for (path, bytes) in &decoys {
            std::fs::write(path, bytes).expect("restore exact solver decoy bytes");
        }
        assert_eq!(snapshot(&paths), before, "restored solver decoy bytes");
        std::fs::remove_file(&artifact_path).expect("remove original solver target");
        for caller in [&repo.root, &repo.review, &repo.unrelated] {
            assert_eq!(
                repo.check(caller, &record_path, signed),
                baseline,
                "missing original {kind} target preserves full approved JSON and exit 0"
            );
        }
        std::fs::write(&artifact_path, &reviewed_bytes).expect("restore exact solver target bytes");
        assert_eq!(
            snapshot(&paths),
            before,
            "restored solver target and decoy bytes"
        );
    }
}

#[test]
fn ap1_ap2_unsigned_solver_kinds_keep_their_creation_render_source() {
    solver_cross_cwd(false);
}

#[test]
fn ap1_ap2_signed_solver_kinds_keep_their_creation_render_source() {
    solver_cross_cwd(true);
}

#[test]
fn ap2_legacy_v1_through_v4_reject_each_added_context_field_before_signature_checks() {
    let repo = ApprovalRepo::new();
    write_inputs(&repo.root, false);
    generate_document(&repo.root, ROOT_SOURCE);
    generate_solver(&repo.root, ROOT_SOURCE, "ledger", "./ledger.md");
    for (kind, artifact, signed, version) in [
        ("ledger", "./ledger.md", false, 1),
        ("ledger", "./ledger.md", true, 2),
        ("requirements_document", ARTIFACT, false, 3),
        ("requirements_document", ARTIFACT, true, 4),
    ] {
        let (record, record_path) = create(&repo, &repo.root, ROOT_SOURCE, kind, artifact, signed);
        assert_new_record(&record, ".", ROOT_SOURCE, artifact, signed);
        let original_bytes = std::fs::read(&record_path).expect("read new record");
        let mut legacy = record.clone();
        legacy["schema"] = json!(format!("fslc.approval.v{version}"));
        legacy
            .as_object_mut()
            .expect("legacy object")
            .remove("path_base");
        legacy
            .as_object_mut()
            .expect("legacy object")
            .remove("render_source");
        let validator = schema(version);
        assert!(
            validator.is_valid(&legacy),
            "control has the existing legacy shape"
        );
        // This is a parsing control, not a migrated legacy approval or a claim
        // that a v6 signature can verify under an older signing domain.
        for fields in [
            &["path_base"][..],
            &["render_source"][..],
            &["path_base", "render_source"][..],
        ] {
            let mut added = legacy.clone();
            for field in fields {
                added[*field] = record[*field].clone();
            }
            assert!(
                !validator.is_valid(&added),
                "legacy v{version} rejects {fields:?}"
            );
            let mutated = repo.root.join("legacy-context.json");
            let bytes = serde_json::to_vec_pretty(&added).expect("serialize relabelled record");
            std::fs::write(&mutated, &bytes).expect("write relabelled record");
            let error = assert_io(&repo.check_output(&repo.root, &mutated, signed));
            let message = error["message"].as_str().expect("error message");
            assert!(
                message.contains("unknown field"),
                "must reject context during parsing: {error}"
            );
            assert!(
                fields.iter().any(|field| message.contains(*field)),
                "reject the added context: {error}"
            );
            assert_eq!(
                std::fs::read(&mutated).expect("read rejected record"),
                bytes,
                "no silent migration"
            );
        }
        assert_eq!(
            std::fs::read(&record_path).expect("read new record"),
            original_bytes
        );
    }
}

#[test]
fn ap2_new_context_is_required_by_the_authoritative_reader() {
    let repo = ApprovalRepo::new();
    write_inputs(&repo.root, false);
    generate_document(&repo.root, ROOT_SOURCE);
    let (record, record_path) = create(
        &repo,
        &repo.root,
        ROOT_SOURCE,
        "requirements_document",
        ARTIFACT,
        false,
    );
    assert_new_record(&record, ".", ROOT_SOURCE, ARTIFACT, false);
    let baseline = repo.check(&repo.root, &record_path, false);
    assert_eq!(baseline["status"], "approved");
    for field in ["path_base", "render_source"] {
        for replacement in [None, Some("")] {
            let mut invalid = record.clone();
            if let Some(value) = replacement {
                invalid[field] = json!(value);
            } else {
                invalid
                    .as_object_mut()
                    .expect("record object")
                    .remove(field);
            }
            std::fs::write(
                &record_path,
                serde_json::to_vec_pretty(&invalid).expect("encode invalid record"),
            )
            .expect("write invalid record");
            assert_io(&repo.check_output(&repo.root, &record_path, false));
        }
    }
    let mut relative = record.clone();
    relative["path_base"] = json!("/absolute/base");
    std::fs::write(
        &record_path,
        serde_json::to_vec_pretty(&relative).expect("encode absolute base"),
    )
    .expect("write absolute base");
    assert_io(&repo.check_output(&repo.root, &record_path, false));
    std::fs::write(
        &record_path,
        serde_json::to_vec_pretty(&record).expect("encode original record"),
    )
    .expect("restore original record");
    assert_eq!(repo.check(&repo.root, &record_path, false), baseline);
}
