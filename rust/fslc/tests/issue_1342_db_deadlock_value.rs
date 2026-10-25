// SPDX-License-Identifier: Apache-2.0

//! Issue #1342: db check validates --deadlock before database findings or I/O.
//! Unsafe input is essential: clean input already reaches a later validator.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

const UNSAFE: &str = "examples/db/unsafe_not_null_before_backfill.fsl";
const SAFE: &str = "examples/db/safe_add_nullable_column.fsl";
const INVALID_MESSAGE: &str = "--deadlock must be warn, error, or ignore";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("run native CLI")
}

fn json_output(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn usage(args: &[&str], message: &str) {
    let output = run(args);
    assert_eq!(
        (output.status.code(), json_output(&output)),
        (
            Some(2),
            json!({"fsl":"1.0", "result":"error", "kind":"usage", "message":message}),
        ),
        "{args:?}"
    );
}

#[test]
fn invalid_deadlock_is_usage_even_for_violated_db() {
    for invalid in ["bogus", "", "WARN", "false"] {
        usage(
            &["db", "check", UNSAFE, "--deadlock", invalid],
            INVALID_MESSAGE,
        );
        usage(
            &["db", "check", "--deadlock", invalid, UNSAFE],
            INVALID_MESSAGE,
        );
    }
}

#[test]
fn invalid_deadlock_precedes_source_loading() {
    let missing = format!("rust/target/issue-1342-missing-{}.fsl", std::process::id());
    assert!(!root().join(&missing).exists());
    usage(
        &["db", "check", "--deadlock", "bogus", &missing],
        INVALID_MESSAGE,
    );
    usage(
        &["db", "check", &missing, "--deadlock", "bogus"],
        INVALID_MESSAGE,
    );
}

#[test]
fn invalid_first_deadlock_cannot_be_overwritten() {
    usage(
        &[
            "db",
            "check",
            "--deadlock",
            "bogus",
            UNSAFE,
            "--deadlock",
            "warn",
        ],
        INVALID_MESSAGE,
    );
}

#[test]
fn invalid_last_deadlock_is_rejected() {
    usage(
        &[
            "db",
            "check",
            "--deadlock",
            "warn",
            UNSAFE,
            "--deadlock",
            "bogus",
        ],
        INVALID_MESSAGE,
    );
}

/// Preservation: the existing missing-value diagnostic stays a usage error.
#[test]
fn missing_deadlock_value_is_usage() {
    usage(
        &["db", "check", UNSAFE, "--deadlock"],
        "--deadlock requires a value",
    );
}

/// Preservation: valid values/default still run the database checker, and an
/// actual unsafe database remains a violation rather than an argument error.
#[test]
fn valid_deadlock_values_and_default_are_preserved() {
    for mode in [None, Some("warn"), Some("error"), Some("ignore")] {
        let mut args = vec!["db", "check", SAFE, "--depth", "1"];
        if let Some(mode) = mode {
            args.extend(["--deadlock", mode]);
        }
        let output = run(&args);
        let value = json_output(&output);
        assert_eq!(output.status.code(), Some(0), "{args:?}: {value}");
        assert_eq!(value["result"], "verified_under_assumptions", "{value}");

        args[2] = UNSAFE;
        let output = run(&args);
        let value = json_output(&output);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {value}");
        assert_eq!(value["result"], "violated", "{value}");
    }
}
