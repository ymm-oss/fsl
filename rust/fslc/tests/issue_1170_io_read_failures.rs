// SPDX-License-Identifier: Apache-2.0

//! Issue #1170 phase 2: an input file that cannot be read is reported the same
//! way on every command, `kind:"io"`, exit 2, message `<path>: <error>`.
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
