// SPDX-License-Identifier: Apache-2.0

//! Issue #1171 (candidate 5): the argv contract of every command `command()`
//! dispatches, pinned before its hand-written option loops became typed
//! option structs with one parser per command.
//!
//! For each command: no arguments, `--help`, an unknown flag, a missing value,
//! an invalid value, a repeated flag (which occurrence wins, and that an
//! invalid occurrence fails wherever it appears), and a valid run. Messages and
//! exit codes are compared exactly; a usage error prints only its envelope.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const SPEC: &str = "specs/cart_v1.fsl";
const CAUSAL: &str = "examples/causal/incident_response.fsl";
const DB: &str = "examples/db/safe_add_nullable_column.fsl";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {}", self.stdout))
    }
}

fn run(args: &[&str]) -> Run {
    let cache = std::env::temp_dir().join(format!("fslc-issue-1171-argv-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .env("FSLC_CACHE_DIR", &cache)
        .output()
        .expect("run native CLI");
    Run {
        status: output.status.code().expect("exit status"),
        stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
        stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
    }
}

/// `args` is a usage error with exactly `message`: exit 2, the bare error
/// envelope on stdout, nothing on stderr.
fn usage(args: &[&str], message: &str) {
    let run = run(args);
    assert_eq!(run.status, 2, "{args:?}: {}", run.stdout);
    assert_eq!(
        run.json(),
        json!({"fsl":"1.0","result":"error","kind":"usage","message":message}),
        "{args:?}"
    );
    assert_eq!(run.stderr, "", "{args:?}");
}

/// `args` fails with a non-usage error envelope of `kind`.
fn error_kind(args: &[&str], status: i32, kind: &str, message: &str) {
    let run = run(args);
    assert_eq!(run.status, status, "{args:?}: {}", run.stdout);
    let value = run.json();
    assert_eq!(value["result"], "error", "{args:?}: {value}");
    assert_eq!(value["kind"], kind, "{args:?}: {value}");
    assert_eq!(value["message"], message, "{args:?}: {value}");
}

/// `args` runs to a JSON envelope; returns it after checking the exit code.
fn ran(args: &[&str], status: i32) -> Value {
    let run = run(args);
    assert_eq!(run.status, status, "{args:?}: {}", run.stdout);
    let value = run.json();
    assert_ne!(value["kind"], "usage", "{args:?}: {value}");
    value
}

/// `--help` / `-h` anywhere after the command path prints that command's help
/// from the embedded CLI contract and exits 0, before any option is parsed.
fn help(path: &[&str], extra: &[&str]) {
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("CLI contract");
    let mut node = &contract["root"];
    for segment in path {
        node = node["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find(|command| {
                    command["path"].as_array().and_then(|p| p.last()) == Some(&json!(segment))
                })
            })
            .unwrap_or_else(|| panic!("no contract node for {path:?}"));
    }
    let expected = node["help"].as_str().expect("help text");
    for flag in ["--help", "-h"] {
        let mut args = path.to_vec();
        args.extend_from_slice(extra);
        args.push(flag);
        let run = run(&args);
        assert_eq!(run.status, 0, "{args:?}");
        assert_eq!(run.stdout, expected, "{args:?}");
        assert_eq!(run.stderr, "", "{args:?}");
    }
}

#[test]
fn no_command_unknown_command_and_version() {
    usage(&[], "usage: fslc <check|verify> SPEC [options]");
    usage(&["bogus"], "unknown command 'bogus'");
    for flag in ["version", "-V", "--version"] {
        let run = run(&[flag]);
        assert_eq!(run.status, 0, "{flag}");
        assert_eq!(run.stdout, format!("fslc {}\n", env!("CARGO_PKG_VERSION")));
        assert_eq!(run.stderr, "");
    }
}

#[test]
fn check_argv() {
    help(&["check"], &[SPEC, "--bogus"]);
    usage(&["check"], "usage: fslc check SPEC [options]");
    usage(
        &["check", SPEC, "--bogus"],
        "unknown check option '--bogus'",
    );
    usage(
        &["check", "--bogus", SPEC],
        "unknown check option '--bogus'",
    );
    usage(&["check", SPEC, "extra"], "unknown check option 'extra'");
    usage(
        &["check", SPEC, "--requirements"],
        "--requirements requires a value",
    );
    usage(&["check", SPEC, "--edition"], "--edition requires a value");
    usage(
        &["check", "--edition", "bogus", SPEC],
        "--edition must be current or next",
    );
    usage(
        &["check", "--edition", "bogus", "--edition", "current", SPEC],
        "--edition must be current or next",
    );
    usage(
        &["check", "--edition", "current", "--edition", "bogus", SPEC],
        "--edition must be current or next",
    );
    assert_eq!(ran(&["check", SPEC], 0)["result"], "ok");
    assert_eq!(
        ran(
            &[
                "check",
                "--strict-tags",
                "--strict-tags",
                "--edition",
                "next",
                "--edition",
                "current",
                SPEC
            ],
            0
        )["result"],
        "ok"
    );
    assert_eq!(ran(&["check", "--", SPEC], 0)["result"], "ok");
    usage(
        &["check", "--strict-tags", CAUSAL],
        "fslc check on a causal model accepts no options; use fslc causal check",
    );
    usage(
        &["check", "--", CAUSAL],
        "fslc check on a causal model accepts no options; use fslc causal check",
    );
    assert_ne!(ran(&["check", CAUSAL], 0)["result"], "error");
}

#[test]
fn kernel_argv() {
    help(&["kernel"], &[]);
    usage(
        &["kernel"],
        "usage: fslc kernel SPEC [--kernel-version MAJOR]",
    );
    usage(
        &["kernel", SPEC, "--bogus"],
        "unknown kernel option '--bogus'",
    );
    usage(
        &["kernel", SPEC, "extra"],
        "unexpected kernel argument 'extra'",
    );
    usage(
        &["kernel", SPEC, "--kernel-version"],
        "--kernel-version requires a value",
    );
    // An unsupported version is reported where it is parsed, ahead of a later
    // unknown option or a stray argument.
    for args in [
        &["kernel", "--kernel-version", "9", "--bogus", SPEC][..],
        &["kernel", "--kernel-version", "9", SPEC, "extra"][..],
    ] {
        let run = run(args);
        assert_eq!(run.status, 2, "{args:?}");
        assert_eq!(run.json()["kind"], "semantics", "{args:?}");
    }
    let schema = |value: &Value| value["$schema"].as_str().unwrap_or_default().to_owned();
    assert!(
        schema(&ran(
            &[
                "kernel",
                "--kernel-version",
                "2",
                "--kernel-version",
                "1",
                SPEC
            ],
            0
        ))
        .ends_with("kernel.v1.schema.json")
    );
    assert!(
        schema(&ran(
            &[
                "kernel",
                "--kernel-version",
                "1",
                "--kernel-version",
                "2",
                SPEC
            ],
            0
        ))
        .ends_with("kernel.v2.schema.json")
    );
}

#[test]
fn conformance_and_testplan_argv() {
    help(&["conformance"], &[]);
    usage(
        &["conformance"],
        "usage: fslc conformance SPEC [--depth N] [--kernel-version MAJOR]",
    );
    usage(
        &["conformance", SPEC, "--bogus"],
        "unknown conformance option '--bogus'",
    );
    usage(
        &["conformance", SPEC, "extra"],
        "unexpected conformance argument 'extra'",
    );
    usage(
        &["conformance", SPEC, "--depth"],
        "--depth requires a value",
    );
    usage(
        &["conformance", "--depth", "x", SPEC],
        "--depth must be a non-negative integer",
    );
    usage(
        &["conformance", SPEC, "--kernel-version"],
        "--kernel-version requires a value",
    );
    let run_bad = run(&["conformance", "--kernel-version", "9", "--bogus", SPEC]);
    assert_eq!(
        (run_bad.status, run_bad.json()["kind"].clone()),
        (2, json!("semantics"))
    );
    assert_eq!(
        ran(&["conformance", "--depth", "2", "--depth", "1", SPEC], 0)["depth"],
        1
    );

    help(&["testplan"], &[]);
    usage(&["testplan"], "usage: fslc testplan SPEC [--depth N]");
    usage(
        &["testplan", SPEC, "--bogus"],
        "unknown testplan option '--bogus'",
    );
    usage(
        &["testplan", SPEC, "extra"],
        "unexpected testplan argument 'extra'",
    );
    usage(&["testplan", SPEC, "--depth"], "--depth requires a value");
    usage(
        &["testplan", "--depth", "-1", SPEC],
        "--depth must be a non-negative integer",
    );
    assert_eq!(
        ran(&["testplan", "--depth", "2", "--depth", "1", SPEC], 0)["depth"],
        1
    );
}

#[test]
fn compat_check_argv() {
    help(&["compat", "check"], &[]);
    usage(
        &["compat"],
        "usage: fslc compat check DBSYSTEM [--include-ai]",
    );
    usage(
        &["compat", "bogus"],
        "usage: fslc compat check DBSYSTEM [--include-ai]",
    );
    usage(&["compat", "check"], "compat check requires a dbsystem");
    usage(
        &["compat", "check", DB, "--bogus"],
        "unknown compat check option '--bogus'",
    );
    usage(
        &["compat", "check", DB, "extra"],
        "unknown compat check option 'extra'",
    );
    let value = ran(&["compat", "check", "--include-ai", "--include-ai", DB], 0);
    assert_eq!(value["compat"]["include_ai"], true, "{value}");
    assert!(ran(&["compat", "check", DB], 0).get("compat").is_none());
}

#[test]
fn explain_mutate_typestate_argv() {
    for command in ["explain", "mutate", "typestate"] {
        help(&[command], &[]);
        usage(&[command], &format!("fslc {command} requires a spec"));
        usage(
            &[command, SPEC, "--bogus"],
            &format!("unknown {command} option '--bogus'"),
        );
        usage(
            &[command, SPEC, "extra"],
            &format!("unknown {command} option 'extra'"),
        );
        if command == "typestate" {
            usage(
                &[command, SPEC, "--depth"],
                "unknown typestate option '--depth'",
            );
            usage(
                &[command, "--depth", "x", SPEC],
                "unknown typestate option '--depth'",
            );
        } else {
            usage(&[command, SPEC, "--depth"], "--depth requires a value");
            usage(
                &[command, "--depth", "x", SPEC],
                "--depth must be a non-negative integer",
            );
        }
        if command == "mutate" {
            usage(
                &[command, SPEC, "--max-mutants"],
                "--max-mutants requires a value",
            );
            usage(
                &[command, "--max-mutants", "-1", SPEC],
                "--max-mutants must be an integer",
            );
            usage(&[command, SPEC, "--from"], "--from requires a JSONL path");
        } else {
            let message = format!("unknown {command} option '--max-mutants'");
            usage(&[command, SPEC, "--max-mutants"], &message);
            usage(&[command, "--max-mutants", "-1", SPEC], &message);
            usage(
                &[command, SPEC, "--from"],
                &format!("unknown {command} option '--from'"),
            );
        }
    }
    // The mutation gate belongs to `mutate` alone (#1237).
    for command in ["explain", "typestate"] {
        for flag in ["--fail-on-survivors", "--min-kill-rate"] {
            usage(
                &[command, flag, "0.5", SPEC],
                &format!("unknown {command} option '{flag}'"),
            );
        }
    }
    usage(
        &["mutate", SPEC, "--min-kill-rate"],
        "--min-kill-rate requires a value",
    );
    for rate in ["2", "-0.1", "NaN", "x"] {
        usage(
            &["mutate", "--min-kill-rate", rate, SPEC],
            "--min-kill-rate must be a number between 0 and 1",
        );
    }
    let explained = ran(&["explain", "--depth", "2", "--depth", "1", SPEC], 0);
    assert_eq!(
        (explained["result"].clone(), explained["depth"].clone()),
        (json!("explained"), json!(1))
    );
    let readable = run(&["explain", "--depth", "1", "--readable", SPEC]);
    assert_eq!(readable.status, 0);
    assert!(
        serde_json::from_str::<Value>(&readable.stdout).is_err(),
        "--readable prints text"
    );
    let skeleton = run(&["typestate", "--ts", SPEC]);
    assert_eq!(skeleton.status, 0);
    assert!(
        skeleton
            .stdout
            .starts_with("// Typestate skeleton for `Cart`"),
        "--ts prints the TypeScript skeleton: {}",
        skeleton.stdout
    );
    assert_eq!(ran(&["typestate", SPEC], 0)["result"], "typestate");
}

#[test]
fn testgen_html_ledger_argv() {
    for command in ["testgen", "html", "ledger"] {
        help(&[command], &[]);
        usage(&[command], &format!("fslc {command} requires a spec"));
        usage(
            &[command, SPEC, "--bogus"],
            &format!("unknown {command} option '--bogus'"),
        );
        usage(
            &[command, SPEC, "extra"],
            &format!("unknown {command} option 'extra'"),
        );
        usage(&[command, SPEC, "--depth"], "--depth requires a value");
        usage(
            &[command, "--depth", "x", SPEC],
            "--depth must be a non-negative integer",
        );
        usage(&[command, SPEC, "-o"], "--output requires a path");
        usage(&[command, SPEC, "--output"], "--output requires a path");
        usage(&[command, SPEC, "--target"], "--target requires a value");
        let engine_missing = if command == "testgen" {
            "unknown testgen option '--engine'"
        } else {
            "--engine requires a value"
        };
        let engine_invalid = if command == "testgen" {
            "unknown testgen option '--engine'"
        } else {
            "--engine must be bmc or induction"
        };
        usage(&[command, SPEC, "--engine"], engine_missing);
        usage(&[command, "--engine", "explicit", SPEC], engine_invalid);
        usage(
            &[command, SPEC, "--deadlock"],
            "--deadlock requires a value",
        );
        usage(
            &[command, "--deadlock", "bogus", SPEC],
            "--deadlock must be warn, error, or ignore",
        );
        usage(
            &[command, "--engine", "bmc", "--engine", "bogus", SPEC],
            engine_invalid,
        );
    }
    for command in ["testgen", "html"] {
        for flag in ["--impl-log", "--evidence", "--approval", "--trust-key"] {
            usage(
                &[command, flag, "x", SPEC],
                &format!("unknown {command} option '{flag}'"),
            );
        }
    }
    for command in ["html", "ledger"] {
        usage(
            &[command, "--allow-unwired", SPEC],
            &format!("unknown {command} option '--allow-unwired'"),
        );
    }
    for flag in ["--impl-log", "--evidence", "--approval", "--trust-key"] {
        usage(&["ledger", SPEC, flag], &format!("{flag} requires a value"));
    }
    let generated = run(&[
        "testgen", "--depth", "1", "--target", "vitest", "--target", "pytest", SPEC,
    ]);
    assert_eq!(generated.status, 0);
    assert!(
        generated.stdout.contains("import pytest") || generated.stdout.contains("def test_"),
        "the last --target wins: {}",
        &generated.stdout[..generated.stdout.len().min(300)]
    );
}

#[test]
fn analyze_argv() {
    help(&["analyze"], &[]);
    usage(&["analyze"], "fslc analyze requires a spec");
    usage(
        &["analyze", SPEC, "--bogus"],
        "unknown analyze option '--bogus'",
    );
    for flag in ["--projection", "--format"] {
        usage(
            &["analyze", SPEC, flag],
            &format!("{flag} requires a value"),
        );
    }
    for flag in ["--focus", "--profile", "--export", "--code"] {
        usage(
            &["analyze", SPEC, flag],
            &format!("{flag} requires a value"),
        );
    }
    let value = ran(
        &[
            "analyze",
            "--projection",
            "tsg",
            "--projection",
            "action_dependency_graph",
            SPEC,
        ],
        0,
    );
    assert_eq!(value["projection"], "action_dependency_graph");
    assert_eq!(
        ran(&["analyze", SPEC, "specs/bank.fsl"], 0)["result"],
        ran(&["analyze", "specs/bank.fsl", SPEC], 0)["result"]
    );
}

#[test]
fn diff_and_chain_argv() {
    help(&["diff"], &[]);
    usage(
        &["diff"],
        "diff requires OLD NEW or --git BASE..HEAD [SPEC]",
    );
    usage(
        &["diff", SPEC],
        "diff requires OLD NEW or --git BASE..HEAD [SPEC]",
    );
    usage(
        &["diff", SPEC, SPEC, SPEC],
        "diff requires OLD NEW or --git BASE..HEAD [SPEC]",
    );
    usage(
        &["diff", SPEC, SPEC, "--bogus"],
        "unknown diff option '--bogus'",
    );
    usage(&["diff", SPEC, SPEC, "--depth"], "--depth requires a value");
    usage(
        &["diff", "--depth", "x", SPEC, SPEC],
        "--depth must be an integer",
    );
    for flag in ["--git", "--mapping", "--forbid"] {
        usage(
            &["diff", SPEC, SPEC, flag],
            &format!("{flag} requires a value"),
        );
    }
    usage(
        &["diff", "--git", "HEAD", SPEC, SPEC],
        "--git accepts at most one spec path",
    );

    help(&["chain"], &[]);
    usage(&["chain", "--bogus"], "unknown chain option '--bogus'");
    usage(
        &["chain", "a.toml", "b.toml"],
        "unexpected chain argument 'b.toml'",
    );
    usage(&["chain", "--jobs"], "--jobs requires a value");
    for jobs in ["0", "x"] {
        usage(
            &["chain", "--jobs", jobs],
            "--jobs must be a positive integer",
        );
    }
    let missing = run(&[
        "chain",
        "--keep-going",
        "--jobs",
        "2",
        "missing-project.toml",
    ]);
    assert_eq!(missing.status, 2);
    assert_eq!(missing.json()["kind"], "io");
    assert!(
        missing.stderr.starts_with("Layer"),
        "the chain table goes to stderr: {}",
        missing.stderr
    );
}

#[test]
fn refine_and_replay_argv() {
    help(&["refine"], &[]);
    usage(&["refine"], "refine requires IMPL ABS MAPPING");
    usage(&["refine", "a", "b"], "refine requires IMPL ABS MAPPING");
    usage(
        &["refine", "a", "b", "c", "--bogus"],
        "unknown refine option '--bogus'",
    );
    usage(
        &["refine", "a", "b", "c", "--depth"],
        "--depth requires a value",
    );
    usage(
        &["refine", "--depth", "x", "a", "b", "c"],
        "--depth must be a non-negative integer",
    );
    error_kind(
        &[
            "refine",
            "specs/cart_impl.fsl",
            SPEC,
            "specs/cart_refines.fsl",
            SPEC,
        ],
        2,
        "usage",
        "refine chain must list (abs map) pairs after the first mapping",
    );

    help(&["replay"], &[]);
    usage(&["replay"], "usage: fslc replay SPEC --trace TRACE.json");
    usage(
        &["replay", SPEC, "--bogus"],
        "unknown replay option '--bogus'",
    );
    for flag in ["--trace", "--from-log", "--mapping"] {
        usage(&["replay", SPEC, flag], &format!("{flag} requires a value"));
    }
    for (args, message) in [
        (
            &["replay", SPEC, "--trace", "a", "--from-log", "b"][..],
            "--trace and --from-log are mutually exclusive",
        ),
        (
            &["replay", SPEC, "--from-log", "b"][..],
            "--mapping is required with --from-log",
        ),
        (
            &["replay", SPEC, "--trace", "a", "--mapping", "m"][..],
            "--mapping is only valid with --from-log",
        ),
        (
            &["replay", SPEC][..],
            "one of --trace or --from-log is required",
        ),
    ] {
        usage(args, message);
    }
}

#[test]
fn sweep_argv() {
    help(&["sweep"], &[]);
    usage(&["sweep"], "usage: fslc sweep SPEC [options]");
    usage(
        &["sweep", SPEC, "--bogus"],
        "unknown sweep option '--bogus'",
    );
    usage(&["sweep", SPEC, "extra"], "unknown sweep option 'extra'");
    for flag in [
        "--depth",
        "--deadlock",
        "--engine",
        "--k",
        "--instances",
        "--values",
        "--property",
        "--vacuity",
        "--requirements",
    ] {
        usage(&["sweep", SPEC, flag], &format!("{flag} requires a value"));
    }
    usage(
        &["sweep", "--deadlock", "x", SPEC],
        "--deadlock must be warn, error, or ignore",
    );
    usage(
        &["sweep", "--engine", "explicit", SPEC],
        "--engine must be bmc or induction",
    );
    usage(
        &["sweep", "--k", "0", SPEC],
        "--k must be a positive integer",
    );
    usage(
        &["sweep", "--vacuity", "x", SPEC],
        "--vacuity must be warn, error, or ignore",
    );
    // The last range wins: 0..3 alone is inconclusive (exit 1) on this spec.
    let value = ran(&["sweep", "--depth", "0..3", "--depth", "2..4", SPEC], 0);
    assert_eq!(value["sweep"]["ranges"]["depth"], json!([2, 4]), "{value}");
}

#[test]
fn verify_argv() {
    help(&["verify"], &[SPEC]);
    usage(&["verify"], "usage: fslc verify SPEC [options]");
    usage(
        &["verify", SPEC, "--bogus"],
        "unknown verify option '--bogus'",
    );
    usage(&["verify", SPEC, "extra"], "unknown verify option 'extra'");
    for flag in [
        "--depth",
        "--deadlock",
        "--engine",
        "--explicit-budget",
        "--k",
        "--vacuity",
        "--property",
        "--exclude-property",
        "--instances",
        "--values",
        "--requirements",
        "--lemma",
        "--from-state",
        "--edition",
    ] {
        usage(&["verify", SPEC, flag], &format!("{flag} requires a value"));
    }
    for (flag, value, message) in [
        ("--depth", "x", "--depth must be a non-negative integer"),
        (
            "--deadlock",
            "x",
            "--deadlock must be warn, error, or ignore",
        ),
        (
            "--engine",
            "x",
            "--engine must be bmc, induction, explicit, or auto",
        ),
        (
            "--explicit-budget",
            "0",
            "--explicit-budget must be a positive integer",
        ),
        ("--k", "0", "--k must be a positive integer"),
        ("--vacuity", "x", "--vacuity must be warn, error, or ignore"),
        (
            "--instances",
            "Cart",
            "invalid --instances value 'Cart': expected NAME=N",
        ),
        (
            "--instances",
            "Cart=0",
            "invalid --instances value 'Cart=0': expected a non-empty NAME and N >= 1",
        ),
        ("--edition", "x", "--edition must be current or next"),
    ] {
        usage(&["verify", flag, value, SPEC], message);
        usage(&["verify", flag, value, "--depth", "1", SPEC], message);
    }
    let value = ran(
        &[
            "verify",
            "--depth",
            "1",
            "--depth",
            "4",
            "--no-cache",
            "--no-cache",
            SPEC,
        ],
        0,
    );
    assert_eq!(value["checked_to_depth"], 4, "{value}");
    let value = ran(&["verify", "--no-cache", "--depth", "4", "--", SPEC], 0);
    assert_eq!(value["checked_to_depth"], 4, "{value}");
}

#[test]
fn scenarios_argv() {
    help(&["scenarios"], &[]);
    usage(&["scenarios"], "usage: fslc scenarios SPEC [options]");
    // `scenarios` shares `verify`'s option parser and its wording.
    usage(
        &["scenarios", SPEC, "--bogus"],
        "unknown verify option '--bogus'",
    );
    usage(&["scenarios", SPEC, "--depth"], "--depth requires a value");
    usage(
        &["scenarios", "--engine", "x", SPEC],
        "--engine must be bmc, induction, explicit, or auto",
    );
    for extra in [
        &["--engine", "induction"][..],
        &["--explicit-budget", "5"][..],
        &["--k", "2"][..],
        &["--vacuity", "error"][..],
        &["--property", "P"][..],
        &["--exclude-property", "P"][..],
        &["--instances", "Cart=1"][..],
        &["--values", "qty=0..1"][..],
        &["--strict-tags"][..],
        &["--requirements", "r.fsl"][..],
        &["--no-cache"][..],
        &["--lemma", "L"][..],
        &["--from-state", "s.json"][..],
        &["--edition", "next"][..],
    ] {
        let mut args = vec!["scenarios", "--depth", "1"];
        args.extend_from_slice(extra);
        args.push(SPEC);
        usage(&args, "scenarios accepts only --depth and --deadlock");
    }
    // Options left at their defaults are not "other options".
    for extra in [
        &["--engine", "bmc"][..],
        &["--edition", "current"][..],
        &["--k", "1"][..],
    ] {
        let mut args = vec!["scenarios", "--depth", "4"];
        args.extend_from_slice(extra);
        args.push(SPEC);
        assert_ne!(ran(&args, 0)["result"], "error", "{args:?}");
    }
    let value = ran(
        &[
            "scenarios",
            "--depth",
            "1",
            "--depth",
            "4",
            "--deadlock",
            "ignore",
            SPEC,
        ],
        0,
    );
    assert_eq!(value["depth"], 4, "{value}");
}

#[test]
fn lint_and_migrate_argv() {
    for command in ["lint", "migrate"] {
        help(&[command], &[]);
        usage(
            &[command, SPEC, "--bogus"],
            &format!("unknown {command} option '--bogus'"),
        );
        usage(&[command, SPEC, "--edition"], "--edition requires a value");
    }
    usage(
        &["lint"],
        "usage: fslc lint FILE... [--edition current|next] [--project fsl-project.toml]",
    );
    usage(
        &["migrate"],
        "usage: fslc migrate FILE... [--edition current|next] [--write]",
    );
    usage(&["lint", "--write", SPEC], "unknown lint option '--write'");
    usage(
        &["migrate", "--project", "p.toml", SPEC],
        "unknown migrate option '--project'",
    );
    usage(&["lint", SPEC, "--project"], "--project requires a value");
    usage(
        &["migrate", SPEC, SPEC],
        "the same migration path cannot be supplied twice",
    );
    assert_eq!(
        ran(
            &["lint", "--edition", "next", "--edition", "current", SPEC],
            0
        )["edition"],
        "current"
    );
    assert_eq!(ran(&["migrate", SPEC], 0)["edition"], "next");
}
