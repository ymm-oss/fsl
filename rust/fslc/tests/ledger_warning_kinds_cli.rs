// SPDX-License-Identifier: Apache-2.0

//! Real CLI controls for #1497. All artifacts remain under this worktree.

#[path = "support/report_cli.rs"]
#[allow(dead_code)]
mod cli;
#[path = "support/ledger_fixtures.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BASE: &str = "e16f65bfeae773218d23f425376ea7c23b895bff";
const SPEC: &str = "// SPDX-License-Identifier: Apache-2.0\nspec LedgerWarningKind {\n  state { done: Bool = false }\n  action finish() {\n    requires not done\n    done = true\n  }\n  invariant Typed { done == true or done == false }\n}\n";

fn run(root: &Path, args: &[&str], status: i32) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(status),
        "{args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture() -> PathBuf {
    let root = support::directory("warning-kinds");
    std::fs::write(root.join("spec.fsl"), SPEC).unwrap();
    root
}

fn ledger(root: &Path, mode: &str, status: i32) -> String {
    String::from_utf8(
        run(
            root,
            &["ledger", "spec.fsl", "--depth", "2", "--deadlock", mode],
            status,
        )
        .stdout,
    )
    .unwrap()
}

fn raw_findings(body: &str) -> Value {
    serde_json::from_str(
        body.split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn deadlock_warning_is_not_vacuity_and_repeats_byte_for_byte() {
    let root = fixture();
    let first = ledger(&root, "warn", 0);
    let second = ledger(&root, "warn", 0);
    assert_eq!(
        first.as_bytes(),
        second.as_bytes(),
        "complete artifacts, no excluded fields"
    );
    assert_eq!(
        raw_findings(&first),
        json!([{
            "kind":"deadlock", "message":"deadlock reachable at step 1 (state: done=true)",
            "hint":"add an enabled action, declare intended stops in a terminal { } block, or use --deadlock=ignore if intentional"
        }])
    );
    let expected = "- **検出**: `deadlock` — deadlock\n  - 反例要約: 停止状態の到達（deadlock）: deadlock reachable at step 1 (state: done=true)\n  - 業務翻訳: 実行可能なアクションがない状態に到達する。意図した停止かを確認し、必要なら操作を追加するか terminal を宣言する。\n  - 次アクション: add an enabled action, declare intended stops in a terminal { } block, or use --deadlock=ignore if intentional\n";
    let details = first
        .lines()
        .filter(|line| line.starts_with("- **検出**:") || line.starts_with("  - "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(details, expected);
    assert!(!details.contains("空虚性"));
    assert!(!details.contains("mutate"));
}

#[test]
fn deadlock_error_keeps_failure_and_ignore_has_no_warning() {
    let root = fixture();
    let failed = ledger(&root, "error", 1);
    assert!(failed.contains("`deadlock`"), "{failed}");
    let ignored = ledger(&root, "ignore", 0);
    assert!(!ignored.contains("deadlock"), "{ignored}");
    assert!(!ignored.contains("- **検出**:"), "{ignored}");
    let verify: Value = serde_json::from_slice(
        &run(
            &root,
            &["verify", "spec.fsl", "--depth", "2", "--deadlock", "error"],
            1,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(verify["result"], "violated");
    assert_eq!(verify["violation_kind"], "deadlock");
}

#[test]
fn reachable_only_warning_keeps_checked_reachability_and_accurate_translation() {
    let root = fixture();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/issue_961/no_safety_reachable.fsl"),
        root.join("spec.fsl"),
    )
    .unwrap();
    let verify: Value =
        serde_json::from_slice(&run(&root, &["verify", "spec.fsl", "--depth", "2"], 0).stdout)
            .unwrap();
    assert_eq!(verify["result"], "verified");
    assert_eq!(verify["reachables"]["Grew"]["witnessed_at_step"], 1);
    assert_eq!(verify["reachables"]["Grew"]["witness"][1]["state"]["x"], 1);
    assert!(
        verify["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| { warning["kind"] == "no_user_invariants" })
    );
    let body = ledger(&root, "warn", 0);
    assert_eq!(raw_findings(&body), verify["warnings"]);
    let (presentation, _) = body.split_once("## 付録: 生 JSON 反例（証跡）").unwrap();
    assert!(presentation.lines().any(|line| line == "  - 業務翻訳: 利用者が記述した不変条件や遷移制約がない。必要な安全性の確認対象を見直す。"));
}

#[test]
fn file_output_matches_stdout_and_preserves_the_full_generated_envelope() {
    let root = fixture();
    let stdout = ledger(&root, "warn", 0);
    let output = run(
        &root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--deadlock",
            "warn",
            "-o",
            "ledger.md",
        ],
        0,
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope,
        json!({"fsl":"1.0", "result":"generated", "kind":"audit_ledger", "spec":"LedgerWarningKind", "output":"ledger.md"})
    );
    assert_eq!(
        std::fs::read(root.join("ledger.md")).unwrap(),
        stdout.as_bytes()
    );
}

fn approval_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_1497_base_approval.json")
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn approval_root() -> PathBuf {
    let root = fixture();
    std::fs::write(
        root.join("spec.fsl"),
        SPEC.replace(
            "  invariant Typed",
            "  @requirement(\"REQ-WARNING\", \"warning review\")\n  invariant Typed",
        ),
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "spec.fsl"]);
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
            "warning baseline",
        ],
    );
    root
}

fn create(root: &Path) -> Value {
    run(
        root,
        &[
            "ledger",
            "spec.fsl",
            "--depth",
            "2",
            "--deadlock",
            "warn",
            "-o",
            "ledger.md",
        ],
        0,
    );
    let output = run(
        root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "ledger.md",
            "--approver",
            "warning-reviewer",
            "--depth",
            "2",
            "--deadlock",
            "warn",
            "-o",
            "approval.json",
        ],
        0,
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["record"].clone()
}

fn check(root: &Path, record: &str) -> Value {
    serde_json::from_slice(
        &run(
            root,
            &["approval", "check", "spec.fsl", "--record", record],
            0,
        )
        .stdout,
    )
    .unwrap()
}

#[test]
fn old_warning_rendering_digest_is_drifted_not_an_error() {
    let captured: Value =
        serde_json::from_str(&std::fs::read_to_string(approval_fixture_path()).unwrap()).unwrap();
    assert_eq!(captured["base_sha"], BASE);
    // Ledger approval binds the literal Markdown bytes (no normalization).
    let baseline_bytes = captured["artifact"].as_str().unwrap().as_bytes();
    assert_eq!(
        format!("sha256:{:x}", Sha256::digest(baseline_bytes)),
        captured["record"]["target"]["digest"]
    );
    let root = approval_root();
    assert_eq!(
        std::fs::read_to_string(root.join("spec.fsl")).unwrap(),
        captured["source"].as_str().unwrap()
    );
    let mut record = captured["record"].clone();
    // The captured repository's commit object is not present in a fresh fixture
    // repository. Rebind Git identity only after proving the committed source
    // bytes are exactly the captured source; preserve both evidence digests.
    assert_eq!(record["spec"]["git_commit"].as_str().unwrap().len(), 40);
    let committed = Command::new("git")
        .args(["show", "HEAD:spec.fsl"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(committed.status.success());
    assert_eq!(
        committed.stdout,
        captured["source"].as_str().unwrap().as_bytes()
    );
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap();
    assert_eq!(head.trim().len(), 40);
    record["spec"]["git_commit"] = json!(head.trim());
    // A future package bump is an independent renderer_changed cause. Retain the
    // measured old artifact digest, but keep this test about rendering_changed.
    assert!(
        record["target"]["generator_version"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    record["target"]["generator_version"] = json!(env!("CARGO_PKG_VERSION"));
    std::fs::write(root.join("old.json"), serde_json::to_vec(&record).unwrap()).unwrap();
    let current = check(&root, "old.json");
    assert_eq!(current["status"], "drifted", "{current}");
    assert_eq!(current["reasons"], json!(["rendering_changed"]));
    assert_eq!(
        current["baseline_digest"], current["current_digest"],
        "spec semantics did not change"
    );
    assert_eq!(
        current["artifact_digest"],
        captured["record"]["target"]["digest"]
    );
    assert_ne!(
        current["artifact_digest"],
        current["current_artifact_digest"]
    );
}

#[test]
fn regenerated_warning_approval_remains_approved() {
    let root = approval_root();
    let created = create(&root);
    let checked = check(&root, "approval.json");
    assert_eq!(checked["status"], "approved", "{checked}");
    assert_eq!(checked["reasons"], json!([]));
    assert_eq!(checked["artifact_digest"], created["target"]["digest"]);
    assert_eq!(
        checked["artifact_digest"],
        checked["current_artifact_digest"]
    );
}

#[test]
#[ignore = "one-time capture of the real base CLI approval and artifact"]
fn capture_main_approval_bytes() {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cli::repository_root())
        .output()
        .unwrap();
    assert!(head.status.success());
    assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), BASE);
    assert!(
        !approval_fixture_path().exists(),
        "never overwrite base evidence"
    );
    let root = approval_root();
    let record = create(&root);
    assert_eq!(check(&root, "approval.json")["status"], "approved");
    let captured = json!({"base_sha":BASE, "source":std::fs::read_to_string(root.join("spec.fsl")).unwrap(), "record":record,
        "artifact":std::fs::read_to_string(root.join("ledger.md")).unwrap()});
    std::fs::write(
        approval_fixture_path(),
        serde_json::to_string_pretty(&captured).unwrap() + "\n",
    )
    .unwrap();
}
