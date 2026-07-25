// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for issue #508: `fslc ledger --evidence` used to use
//! external evidence only to label assurance class, never as a finding — a
//! requirement explicitly attached to failing evidence (root `requirements`
//! list, or a nested `findings[]`/`checks[]` item's `requirement.id`) still
//! rendered green with no 🔴 row, and evidence with no requirement
//! attribution at all was silently dropped rather than becoming a
//! spec-level finding. `docs/DESIGN-assurance-classes.md`: "a failing
//! source never lowers the class of an independently proven requirement —
//! it adds a 要確認 finding." Every `*_is_a_red_finding`/`*_finding` test
//! here fails if the fix is reverted; the `*_stays_green` tests guard that
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
