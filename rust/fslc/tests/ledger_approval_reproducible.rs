// SPDX-License-Identifier: Apache-2.0

#[path = "support/report_cli.rs"]
#[allow(dead_code)]
mod cli;
#[path = "support/ledger_fixtures.rs"]
mod support;

use std::path::Path;
use std::process::{Command, Output};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::{Value, json};

const VIOLATED: &str = "tests/fixtures/rust_port/sweep_counterexample.fsl";
const CTI: &str = "tests/fixtures/rust_port/induction_unknown_cti.fsl";

fn args(induction: bool) -> Vec<&'static str> {
    let mut args = vec!["--depth", "3", "--deadlock", "ignore"];
    if induction {
        args.extend(["--engine", "induction"]);
    }
    args
}

fn run(root: &Path, arguments: &[&str], status: i32) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(root)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(status),
        "{arguments:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_bytes(fixture: &str, induction: bool) {
    let extra = args(induction);
    let (first, status) = cli::run("ledger", fixture, 3, &extra);
    assert_eq!(status, 1, "{first}");
    let (second, status) = cli::run("ledger", fixture, 3, &extra);
    assert_eq!(status, 1, "{second}");
    // Compare the complete artifact; retain a compact, observed difference on failure.
    let differences = first
        .lines()
        .zip(second.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, (a, b))| format!("line {}: produced {a} <> {b}", index + 1))
        .collect::<Vec<_>>();
    assert!(
        first.as_bytes() == second.as_bytes(),
        "identical inputs must produce identical bytes:\n{}",
        differences.join("\n")
    );
}

#[test]
fn violated_ledger_bytes_are_reproducible() {
    assert_bytes(VIOLATED, false);
}
#[test]
fn cti_ledger_bytes_are_reproducible() {
    assert_bytes(CTI, true);
}

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn approval_root(fixture: &str, declaration: &str) -> std::path::PathBuf {
    let root = support::directory("approval");
    std::fs::write(
        root.join("spec.fsl"),
        support::annotated_fixture(fixture, declaration),
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "spec.fsl"]);
    // Fixture-local identity only; do not modify repository or global git config.
    git(
        &root,
        &[
            "-c",
            "user.name=Ledger Test",
            "-c",
            "user.email=ledger-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "test baseline",
        ],
    );
    root
}

#[allow(clippy::default_trait_access)]
fn write_keys(root: &Path) {
    // Deterministic test-only key material, never a user's private key.
    let key = SigningKey::from_bytes(&[7; 32]);
    std::fs::write(
        root.join("private.pem"),
        key.to_pkcs8_pem(Default::default()).unwrap().as_bytes(),
    )
    .unwrap();
    std::fs::write(
        root.join("public.pem"),
        key.verifying_key()
            .to_public_key_pem(Default::default())
            .unwrap(),
    )
    .unwrap();
}

fn create(root: &Path, induction: bool, ledger_status: i32) {
    write_keys(root);
    let mut ledger = vec!["ledger", "spec.fsl", "-o", "review.md"];
    ledger.extend(args(induction));
    run(root, &ledger, ledger_status);
    let mut create = vec![
        "approval",
        "create",
        "spec.fsl",
        "--kind",
        "ledger",
        "--artifact",
        "review.md",
        "--approver",
        "alice",
        "--signing-key",
        "private.pem",
        "-o",
        "approval.json",
    ];
    create.extend(args(induction));
    let output = run(root, &create, 0);
    let created: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        created["record"]["approval"]["requirements"],
        json!(["REQ-OBLIGATION"])
    );
}

fn check(root: &Path) -> Value {
    serde_json::from_slice(
        &run(
            root,
            &[
                "approval",
                "check",
                "spec.fsl",
                "--record",
                "approval.json",
                "--trust-key",
                "public.pem",
            ],
            0,
        )
        .stdout,
    )
    .unwrap()
}

fn assert_drift(root: &Path) {
    let path = root.join("spec.fsl");
    let original = std::fs::read_to_string(&path).unwrap();
    // One requirement-text character changes rendered approval content, not a comment.
    let changed = original.replacen("review obligation", "Review obligation", 1);
    assert_ne!(original, changed);
    assert_eq!(
        original
            .bytes()
            .zip(changed.bytes())
            .filter(|(a, b)| a != b)
            .count(),
        1
    );
    std::fs::write(path, changed).unwrap();
    let drifted = check(root);
    assert_eq!(drifted["status"], "drifted", "{drifted}");
    assert_eq!(
        drifted["reasons"],
        json!(["rendering_changed"]),
        "{drifted}"
    );
}

fn round_trip(fixture: &str, declaration: &str, induction: bool) {
    let root = approval_root(fixture, declaration);
    create(&root, induction, 1);
    let approved = check(&root);
    assert_eq!(approved["status"], "approved", "{approved}");
    assert_eq!(approved["signature_status"], "signed", "{approved}");
    assert_eq!(approved["reasons"], json!([]));
    assert_drift(&root);
}

#[test]
fn violated_ledger_approval_round_trip_then_one_character_drift() {
    round_trip(VIOLATED, "invariant AtMostOne", false);
}
#[test]
fn cti_ledger_approval_round_trip_then_one_character_drift() {
    round_trip(CTI, "invariant Sync", true);
}

#[test]
fn one_character_spec_drift_is_detected_without_ambient_findings() {
    // Independent negative control can run before failed-ledger create is repaired.
    let root = approval_root(
        "examples/gallery/valid/tiny_bounded_counter.fsl",
        "invariant NeverAboveCap",
    );
    create(&root, false, 0);
    let approved = check(&root);
    assert_eq!(approved["status"], "approved", "{approved}");
    assert_drift(&root);
}
