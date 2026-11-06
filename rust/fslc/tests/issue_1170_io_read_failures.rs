// SPDX-License-Identifier: Apache-2.0

//! Issue #1170 phase 2: an input file the binary reads itself that cannot be
//! read is reported the same way on each retired route, `kind:"io"`, exit 2,
//! message `<path>: <error>`.
//!
//! One positive case per retired route, each compared in full against the OS
//! error the same read produces, plus negative controls that keep the
//! neighboring non-IO failures in their own kinds.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "fslc-1170-io-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove stale scratch");
    }
    std::fs::create_dir_all(&root).expect("create scratch");
    root
}

fn run(cwd: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// The complete envelope for `relative` (named as the user named it) failing
/// to be read from `cwd`.
fn unreadable(cwd: &Path, relative: &str) -> Value {
    let error = std::fs::read(cwd.join(relative)).expect_err("input must be unreadable");
    json!({"fsl": "1.0", "result": "error", "kind": "io", "message": format!("{relative}: {error}")})
}

fn assert_unreadable(cwd: &Path, args: &[&str], relative: &str) {
    let (mut actual, status) = run(cwd, args);
    if let Some(object) = actual.as_object_mut() {
        object.remove("versions");
    }
    assert_eq!((actual, status), (unreadable(cwd, relative), 2), "{args:?}");
}

fn copy(cwd: &Path, from: &str, to: &str) {
    std::fs::copy(repository().join(from), cwd.join(to)).expect("copy fixture");
}

/// Route a: `approval create --kind requirements_document` read its
/// `--artifact`, `--glossary` and `--evidence` during option parsing and
/// reported a read failure as `kind:"usage"`.
#[test]
fn approval_document_inputs_report_io() {
    let cwd = scratch();
    copy(&cwd, "tests/fixtures/approval.fsl", "spec.fsl");
    let (generated, status) = run(
        &cwd,
        &["document", "generate", "spec.fsl", "-o", "requirements.md"],
    );
    assert_eq!(status, 0, "{generated}");
    let create = [
        "approval",
        "create",
        "spec.fsl",
        "--kind",
        "requirements_document",
    ];
    let mut args = create.to_vec();
    args.extend(["--artifact", "missing.md", "--approver", "a"]);
    assert_unreadable(&cwd, &args, "missing.md");
    for flag in ["--glossary", "--evidence"] {
        let mut args = create.to_vec();
        args.extend([
            "--artifact",
            "requirements.md",
            "--approver",
            "a",
            flag,
            "missing.json",
        ]);
        assert_unreadable(&cwd, &args, "missing.json");
    }
    // Negative control: a readable artifact with unusable frontmatter is
    // still the usage error it was.
    std::fs::write(cwd.join("plain.md"), "# no frontmatter\n").expect("write artifact");
    let mut args = create.to_vec();
    args.extend(["--artifact", "plain.md", "--approver", "a"]);
    let (actual, status) = run(&cwd, &args);
    assert_eq!(
        (actual["kind"].as_str(), status),
        (Some("usage"), 2),
        "{actual}"
    );
}

/// Route b: the domain commands reported an unreadable spec as an unlocated
/// `kind:"semantics"` (the #780-era exception).
#[test]
fn domain_commands_report_io() {
    let cwd = scratch();
    for command in [
        ["domain", "check"],
        ["domain", "analyze"],
        ["db", "observe"],
    ] {
        let mut args = command.to_vec();
        args.push("missing.fsl");
        if command[0] == "db" {
            args.extend(["--trace", "trace.json"]);
        }
        assert_unreadable(&cwd, &args, "missing.fsl");
    }
    // Negative control: a readable domain document that does not parse is
    // not an IO failure.
    std::fs::write(cwd.join("broken.fsl"), "domain Broken {\n").expect("write spec");
    let (actual, status) = run(&cwd, &["domain", "check", "broken.fsl"]);
    assert_eq!(status, 2, "{actual}");
    assert_ne!(actual["kind"], "io", "{actual}");
}

/// Route c: inputs that reported the bare OS error without the path.
#[test]
fn pathless_inputs_name_their_path() {
    let cwd = scratch();
    copy(&cwd, "specs/cart_v1.fsl", "spec.fsl");
    std::fs::create_dir(cwd.join("adir")).expect("create directory input");
    for (args, relative) in [
        (vec!["ai", "check", "missing.fsl"], "missing.fsl"),
        (vec!["fmt", "missing.fsl"], "missing.fsl"),
        (vec!["fmt", "--check", "adir"], "adir"),
        (
            vec!["replay", "spec.fsl", "--trace", "missing.json"],
            "missing.json",
        ),
        (
            vec!["approval", "check", "spec.fsl", "--record", "missing.json"],
            "missing.json",
        ),
        (vec!["diff", "spec.fsl", "missing.fsl"], "missing.fsl"),
        (
            vec!["refine", "spec.fsl", "spec.fsl", "missing.fsl"],
            "missing.fsl",
        ),
    ] {
        assert_unreadable(&cwd, &args, relative);
    }
}

/// Route d: the causal family spelled `cannot read <path>: <error>`.
#[test]
fn causal_inputs_use_the_common_spelling() {
    let cwd = scratch();
    for args in [
        vec!["causal", "check", "missing.fsl"],
        vec!["causal", "diff", "missing.fsl", "missing.fsl"],
    ] {
        assert_unreadable(&cwd, &args, "missing.fsl");
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

/// Route c, git trees: `diff --git` reads a mapping committed in the head
/// revision from a temporary checkout that is gone when the command returns.
/// The message names the mapping as the caller named it.
#[test]
fn git_tree_mapping_is_named_as_the_caller_named_it() {
    let cwd = scratch();
    copy(&cwd, "specs/cart_v1.fsl", "spec.fsl");
    git(&cwd, &["init", "-q"]);
    git(&cwd, &["config", "user.email", "t@example.com"]);
    git(&cwd, &["config", "user.name", "T"]);
    git(&cwd, &["config", "commit.gpgsign", "false"]);
    git(&cwd, &["add", "spec.fsl"]);
    git(&cwd, &["commit", "-qm", "base"]);
    std::fs::write(cwd.join("badmap.fsl"), b"\xff\xfe mapping\n").expect("write mapping");
    git(&cwd, &["add", "badmap.fsl"]);
    git(&cwd, &["commit", "-qm", "mapping"]);
    let error = std::fs::read_to_string(cwd.join("badmap.fsl")).expect_err("not UTF-8");
    let (actual, status) = run(
        &cwd,
        &[
            "diff",
            "--git",
            "HEAD~1..HEAD",
            "spec.fsl",
            "--mapping",
            "badmap.fsl",
        ],
    );
    assert_eq!(
        (actual["kind"].as_str(), actual["message"].as_str(), status),
        (Some("io"), Some(format!("badmap.fsl: {error}").as_str()), 2),
        "{actual}"
    );
}

/// Route c, `chain` manifest: a missing manifest keeps `file not found:`;
/// any other read failure names the path with the OS error.
#[test]
fn chain_manifest_read_failures_name_the_error() {
    let cwd = scratch();
    std::fs::create_dir(cwd.join("adir")).expect("create directory input");
    std::fs::write(cwd.join("bad.toml"), b"\xff\xfe\n").expect("write manifest");
    let envelope = |message: String, manifest: &str| {
        json!({
            "fsl": "1.0",
            "result": "error",
            "kind": "io",
            "message": message,
            "manifest": manifest,
        })
    };
    for manifest in ["adir", "bad.toml"] {
        let error = std::fs::read_to_string(cwd.join(manifest)).expect_err("unreadable");
        let (mut actual, status) = run(&cwd, &["chain", manifest]);
        if let Some(object) = actual.as_object_mut() {
            object.remove("versions");
        }
        assert_eq!(
            (actual, status),
            (envelope(format!("{manifest}: {error}"), manifest), 2)
        );
    }
    // Negative control: a missing manifest keeps its spelling.
    let (mut actual, status) = run(&cwd, &["chain", "missing.toml"]);
    if let Some(object) = actual.as_object_mut() {
        object.remove("versions");
    }
    assert_eq!(
        (actual, status),
        (
            envelope("file not found: missing.toml".to_owned(), "missing.toml"),
            2
        )
    );
}

/// Route c, git trees: `approval diff` reads the baseline from a temporary
/// checkout of the recorded commit. A baseline that cannot be read is named
/// `<commit>:<path>`, never by the temporary path. Its kind stays the
/// `semantics` the digest check reported before.
#[test]
fn git_tree_approval_baseline_is_named_by_commit_and_path() {
    let cwd = scratch();
    git(&cwd, &["init", "-q"]);
    git(&cwd, &["config", "user.email", "t@example.com"]);
    git(&cwd, &["config", "user.name", "T"]);
    git(&cwd, &["config", "commit.gpgsign", "false"]);
    std::fs::write(cwd.join("spec.fsl"), b"\xff\xfe spec\n").expect("write spec");
    git(&cwd, &["add", "spec.fsl"]);
    git(&cwd, &["commit", "-qm", "unreadable baseline"]);
    let error = std::fs::read_to_string(cwd.join("spec.fsl")).expect_err("not UTF-8");
    copy(&cwd, "tests/fixtures/approval.fsl", "spec.fsl");
    git(&cwd, &["commit", "-qam", "readable"]);
    let baseline = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(&cwd)
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf-8 sha")
    .trim()
    .to_owned();
    let (ledger, status) = run(
        &cwd,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    assert_eq!(status, 0, "{ledger}");
    let (created, status) = run(
        &cwd,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "a",
            "--depth",
            "2",
            "-o",
            "approval.json",
        ],
    );
    assert_eq!(status, 0, "{created}");
    let mut record: Value =
        serde_json::from_slice(&std::fs::read(cwd.join("approval.json")).expect("read record"))
            .expect("record JSON");
    record["spec"]["git_commit"] = json!(baseline);
    std::fs::write(cwd.join("approval.json"), record.to_string()).expect("write record");
    let (actual, status) = run(
        &cwd,
        &["approval", "diff", "spec.fsl", "--record", "approval.json"],
    );
    assert_eq!(
        (actual["kind"].as_str(), actual["message"].as_str(), status),
        (
            Some("semantics"),
            Some(format!("{baseline}:spec.fsl: {error}").as_str()),
            2
        ),
        "{actual}"
    );
}
