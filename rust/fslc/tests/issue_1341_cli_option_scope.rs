// SPDX-License-Identifier: Apache-2.0

//! Issue #1341: shared parsers must reject options owned by another command.
//! The twelve rejection tests are detectors; supported command options are
//! preservation controls. Publication of --allow-unwired is a separate decision.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

const SPEC: &str = "rust/fslc/tests/fixtures/sweep_clean.fsl";

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

fn rejects_option(command: &str, option: &str, value: Option<&str>) {
    let mut option_args = vec![option];
    option_args.extend(value);
    let mut first = vec![command];
    first.extend_from_slice(&option_args);
    first.push(SPEC);
    let mut last = vec![command, SPEC];
    last.extend_from_slice(&option_args);
    // An unowned value-taking option is unknown even without a value: reject
    // ownership before trying to consume/validate the owning command's value.
    for args in [first, last, vec![command, SPEC, option]] {
        let output = run(&args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let prefix = stdout.chars().take(100).collect::<String>();
        assert!(
            output.status.code() == Some(2) && stdout.trim_start().starts_with('{'),
            "{args:?}: expected a usage JSON envelope (exit 2, stdout starts with '{{'); \
             produced exit {:?}, stdout prefix {prefix:?}",
            output.status.code(),
        );
        assert_eq!(
            (output.status.code(), json_output(&output)),
            (
                Some(2),
                json!({
                    "fsl": "1.0",
                    "result": "error",
                    "kind": "usage",
                    "message": format!("unknown {command} option '{option}'"),
                }),
            ),
            "{args:?}"
        );
    }
}

macro_rules! rejection_tests {
    ($($name:ident: $command:literal, $option:literal, $value:expr;)*) => {
        $(
            #[test]
            fn $name() {
                rejects_option($command, $option, $value);
            }
        )*
    };
}

rejection_tests! {
    explain_rejects_max_mutants: "explain", "--max-mutants", Some("0");
    explain_rejects_by_requirement: "explain", "--by-requirement", None;
    explain_rejects_oracle_attribution: "explain", "--oracle-attribution", None;
    explain_rejects_from: "explain", "--from", Some("missing-mutants.jsonl");
    explain_rejects_ts: "explain", "--ts", None;
    typestate_rejects_depth: "typestate", "--depth", Some("0");
    typestate_rejects_readable: "typestate", "--readable", None;
    typestate_rejects_max_mutants: "typestate", "--max-mutants", Some("0");
    typestate_rejects_by_requirement: "typestate", "--by-requirement", None;
    typestate_rejects_oracle_attribution: "typestate", "--oracle-attribution", None;
    typestate_rejects_from: "typestate", "--from", Some("missing-mutants.jsonl");
}

#[test]
fn testgen_rejects_engine() {
    for engine in ["bmc", "induction", "explicit"] {
        rejects_option("testgen", "--engine", Some(engine));
    }
}

/// Preservation: the options actually used by each command keep working.
#[test]
fn supported_options_remain_command_local() {
    let explained = run(&["explain", "--depth", "1", "--readable", SPEC]);
    assert_eq!(explained.status.code(), Some(0));
    assert!(explained.stderr.is_empty());
    assert!(String::from_utf8_lossy(&explained.stdout).starts_with("Spec: SweepClean (depth 1)"));

    let mutated = run(&[
        "mutate",
        "specs/cart_v1.fsl",
        "--depth",
        "8",
        "--max-mutants",
        "0",
        "--from",
        "rust/fslc/tests/fixtures/issue_1237_killed.jsonl",
        "--by-requirement",
        "--oracle-attribution",
    ]);
    // Only this successful mutation report may carry stderr progress.
    let value: Value = serde_json::from_slice(&mutated.stdout).expect("mutate JSON");
    assert_eq!(mutated.status.code(), Some(0), "{value}");
    assert_eq!(value["result"], "mutated", "{value}");
    let stderr = std::str::from_utf8(&mutated.stderr).expect("UTF-8 progress");
    assert!(!stderr.is_empty(), "successful mutate must report progress");
    assert!(
        stderr.lines().all(|line| line.starts_with("mutate ")
            && line.contains(" (engine=")
            && line.ends_with("s)")),
        "{stderr}"
    );
    assert_eq!(value["summary"]["total"], 3, "{value}");
    assert_eq!(value["summary"]["killed"], 3, "{value}");
    assert_eq!(value["attribution"]["mode"], "all_killers", "{value}");

    let typestate = run(&["typestate", "--ts", "specs/cart_v1.fsl"]);
    assert_eq!(typestate.status.code(), Some(0));
    assert!(typestate.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&typestate.stdout).starts_with("// Typestate skeleton for `Cart`")
    );

    let generated = run(&[
        "testgen", "--depth", "1", "--strict", "--target", "vitest", SPEC,
    ]);
    assert_eq!(generated.status.code(), Some(0));
    assert!(generated.stderr.is_empty());
    assert!(String::from_utf8_lossy(&generated.stdout).contains("conformance tests (Vitest)"));
}

#[test]
fn testgen_output_option_remains_supported() {
    let directory = root().join(format!(
        "rust/target/issue-1341-output-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("worktree-local output directory");
    let path = directory.join("generated.test.ts");
    let output = run(&[
        "testgen",
        "--depth",
        "1",
        "--target",
        "vitest",
        "-o",
        path.to_str().expect("UTF-8 output path"),
        SPEC,
    ]);
    let value = json_output(&output);
    assert_eq!(output.status.code(), Some(0), "{value}");
    assert_eq!(value["result"], "generated", "{value}");
    assert_eq!(value["target"], "vitest", "{value}");
    assert_eq!(value["output"], path.to_string_lossy().as_ref(), "{value}");
    let content = std::fs::read_to_string(path).expect("generated scaffold");
    assert!(content.contains("conformance tests (Vitest)"));
}
