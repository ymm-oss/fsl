// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for issue #508: `fslc ledger --evidence` used to use
//! external evidence only to label assurance class, never as a finding — a
//! requirement explicitly attached to failing evidence (root `requirements`
//! list, or a nested `findings[]`/`checks[]` item's `requirement.id`) still
//! rendered green with no 🔴 row, and evidence with no requirement
//! attribution at all was silently dropped rather than becoming a
//! spec-level finding. `docs/design/DESIGN-assurance-classes.md` requires that "a
//! failing source never lowers the class of an independently proven
//! requirement — it adds a 要確認 finding." Every
//! `*_is_a_red_finding`/`*_finding` test here fails if the fix is reverted;
//! the `*_stays_green` tests guard that
//! a passing or verdict-less (gate-failure) source is not turned into a
//! false positive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root().join(format!(
        "rust/target/ledger-evidence-findings-cli-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

const SPEC: &str = r#"spec LedgerProbe {
  state { x: Bool = false }
  action stay() { x = x }
  @requirement("REQ-AUDIT-001", "x remains false")
  invariant Safe { not x }
}
"#;

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
}

/// Writes the probe spec and the given evidence JSON into a fresh scratch
/// dir, runs `ledger --evidence`, and returns the rendered Markdown.
fn render(name: &str, evidence_json: &str) -> String {
    let dir = scratch_dir(name);
    fs::write(dir.join("ledger_probe.fsl"), SPEC).expect("write spec");
    fs::write(dir.join("evidence.json"), evidence_json).expect("write evidence");
    let output = run(
        &dir,
        &[
            "ledger",
            "ledger_probe.fsl",
            "--evidence",
            "evidence.json",
            "-o",
            "ledger.md",
        ],
    );
    assert!(
        output.status.success(),
        "ledger --evidence failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::read_to_string(dir.join("ledger.md")).expect("ledger written")
}

fn requirement_row(markdown: &str) -> &str {
    markdown
        .lines()
        .find(|line| line.starts_with("| REQ-AUDIT-001 |"))
        .unwrap_or_else(|| panic!("REQ-AUDIT-001 row present in {markdown}"))
}

#[test]
fn ledger_evidence_loader_errors_preserve_full_errors_and_destinations() {
    type EvidenceCase = (&'static str, Option<&'static [u8]>, Option<&'static str>);
    let cases: [EvidenceCase; 4] = [
        ("missing", None, None),
        (
            "malformed",
            Some(b"{not json"),
            Some("invalid JSON: key must be a string at line 1 column 2"),
        ),
        (
            "nonobject",
            Some(b"[]"),
            Some("evidence JSON must contain an object envelope"),
        ),
        (
            "invalid-utf8",
            Some(&[0xff]),
            Some("evidence.json: stream did not contain valid UTF-8"),
        ),
    ];
    for (name, input, message) in cases {
        let dir = scratch_dir(name);
        fs::write(dir.join("ledger_probe.fsl"), SPEC).expect("write valid probe spec");
        let evidence = dir.join("evidence.json");
        fs::write(&evidence, b"{}").expect("write valid evidence control");
        let args = [
            "ledger",
            "ledger_probe.fsl",
            "--evidence",
            "evidence.json",
            "-o",
            "ledger.md",
        ];
        let control = run(&dir, &args);
        assert_eq!(control.status.code(), Some(0), "valid ledger control");
        let destination = dir.join("ledger.md");
        assert!(!fs::read(&destination).unwrap().is_empty());
        fs::remove_file(&destination).expect("remove positive-control output");
        if let Some(bytes) = input {
            fs::write(&evidence, bytes).expect("write invalid evidence bytes");
        } else {
            fs::remove_file(&evidence).expect("remove evidence for missing-file case");
        }
        let message = message.map_or_else(
            || {
                // The OS error string is platform-specific; preserve it in full
                // behind the argument path the IO owner prefixes (#1170).
                let error = fs::read_to_string(&evidence).expect_err("evidence must be absent");
                format!("evidence.json: {error}")
            },
            str::to_owned,
        );
        let expected = serde_json::json!({
            "fsl": "1.0",
            "result": "error",
            "kind": "io",
            "message": message,
        });
        let preserved = b"existing ledger output\0\xff\n";
        for exists in [false, true] {
            if exists {
                fs::write(&destination, preserved).expect("write existing output");
            }
            let output = run(&dir, &args);
            assert_eq!(output.status.code(), Some(2));
            let value: serde_json::Value =
                serde_json::from_slice(&output.stdout).expect("complete JSON error stdout");
            assert_eq!(value, expected, "complete ledger error for {name}");
            assert!(output.stderr.is_empty(), "{:?}", output.stderr);
            if exists {
                assert_eq!(fs::read(&destination).unwrap(), preserved);
            } else {
                assert!(!destination.exists(), "refusal must not create an output");
            }
        }
        if let Some(bytes) = input {
            assert_eq!(fs::read(&evidence).unwrap(), bytes);
        } else {
            assert!(
                !evidence.exists(),
                "loader must not create missing evidence"
            );
        }
        fs::remove_dir_all(dir).expect("remove loader failure fixture");
    }
}

#[test]
fn ledger_evidence_root_attached_failure_is_a_red_finding() {
    let markdown = render(
        "root-attached",
        r#"{"result":"replay_nonconformant","formal_result":"not_run","evidence":{"kind":"runtime_replay"},"requirements":["REQ-AUDIT-001"],"findings":[{"kind":"human_approval_required_before_irreversible_tool","requirement":{"id":"REQ-AUDIT-001"}}]}"#,
    );
    let row = requirement_row(&markdown);
    assert!(row.contains("🔴 要確認"), "{row}");
    assert!(row.contains("external_evidence"), "{row}");
    assert!(
        markdown.contains("外部証跡『evidence.json』が非適合/失敗を報告"),
        "{markdown}"
    );
}

#[test]
fn ledger_evidence_nested_attached_failure_is_a_red_finding() {
    // No root `requirements` list — attribution comes solely from the
    // nested `findings[0].requirement.id`.
    let markdown = render(
        "nested-attached",
        r#"{"result":"replay_nonconformant","formal_result":"not_run","evidence":{"kind":"runtime_replay"},"findings":[{"kind":"human_approval_required_before_irreversible_tool","requirement":{"id":"REQ-AUDIT-001"}}]}"#,
    );
    let row = requirement_row(&markdown);
    assert!(row.contains("🔴 要確認"), "{row}");
    // The assurance-class column must also recognize the nested
    // attribution (not just the finding), so the external evidence's class
    // is folded into the same row rather than silently dropped.
    assert!(row.contains("replay-observed"), "{row}");
}

#[test]
fn ledger_evidence_unattached_failure_is_a_spec_level_finding() {
    // No requirement attribution anywhere: this must not be silently
    // dropped, but it also cannot upgrade any specific requirement row —
    // it becomes a （仕様全体） spec-level finding.
    let markdown = render(
        "unattached",
        r#"{"result":"replay_nonconformant","formal_result":"not_run","evidence":{"kind":"runtime_replay"}}"#,
    );
    let row = requirement_row(&markdown);
    assert!(row.contains("🟢"), "{row}");
    assert!(
        markdown.contains("（仕様全体） | 要件ID未付与の検出 | 🔴 要確認"),
        "{markdown}"
    );
}

#[test]
fn ledger_evidence_passing_result_stays_green() {
    let markdown = render(
        "passing",
        r#"{"result":"conformant","formal_result":"not_run","evidence":{"kind":"runtime_replay"},"requirements":["REQ-AUDIT-001"]}"#,
    );
    let row = requirement_row(&markdown);
    assert!(row.contains('🟢'), "{row}");
    assert!(!row.contains("external_evidence"), "{row}");
}

#[test]
fn ledger_evidence_gate_failure_without_verdict_stays_green() {
    // A gate failure (no Wilson interval computed) carries no
    // pass/fail verdict at all — `not_run` class, but not a finding.
    let markdown = render(
        "gate-failure",
        r#"{"result":"dataset_invalid","formal_result":"not_run","requirements":["REQ-AUDIT-001"]}"#,
    );
    let row = requirement_row(&markdown);
    assert!(row.contains('🟢'), "{row}");
    assert!(!row.contains("external_evidence"), "{row}");
}
