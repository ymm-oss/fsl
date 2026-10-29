// SPDX-License-Identifier: Apache-2.0

//! #1281 PR B: stderr-only explicit-first progress; complete stdout identity.
//! Hashes pin raw stdout (including its newline) from main ddce25fe8ae0f15f031e130b362bd99de65b0c1c.
//! Capture: `FSLC_CACHE=off rust/target/debug/fslc mutate <case args below>`,
//! from the repository root, default native debug build, no field exclusions.
//! Progress tests are detectors (red before PR B); identity/error tests are
//! preservation controls and must already pass before production changes.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const RBAC: &[&str] = &[
    "examples/structural/rbac.fsl",
    "--depth",
    "8",
    "--max-mutants",
    "0",
];
const POINTER: &[&str] = &[
    "rust/fslc/tests/fixtures/issue_1283_pointer_registry.fsl",
    "--depth",
    "3",
    "--max-mutants",
    "6",
];
const BUILTIN: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1000",
];
const EXTERNAL: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "0",
    "--from",
    "rust/fslc/tests/fixtures/issue_1281_progress.jsonl",
];
const EXTERNAL_GATE: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1",
    "--from",
    "rust/fslc/tests/fixtures/issue_1281_progress.jsonl",
    "--min-kill-rate",
    "1",
];
const UNREADABLE: &[&str] = &[
    "rust/fslc/tests/fixtures/sweep_clean.fsl",
    "--depth",
    "2",
    "--max-mutants",
    "1",
    "--from",
    "does/not/exist.jsonl",
];

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("mutate")
        .args(args)
        .current_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("repository root"),
        )
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("run native mutate")
}

fn report(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON stdout");
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(value["baseline"], "verified", "{value}");
    value
}

/// Validate every raw stderr line, including finite nonnegative seconds and
/// the closed set of private route labels. Return the body and engine.
fn progress(output: &Output) -> Vec<(&str, &str)> {
    let stderr = std::str::from_utf8(&output.stderr).expect("UTF-8 stderr");
    assert!(
        !stderr.is_empty(),
        "successful mutate report must emit progress"
    );
    stderr
        .lines()
        .map(|line| {
            assert!(line.starts_with("mutate "), "{line:?}");
            let (body, timing) = line.split_once(" (engine=").expect("engine suffix");
            let (engine, seconds) = timing.split_once(", ").expect("duration separator");
            assert!(
                matches!(engine, "explicit" | "bmc" | "probe" | "none"),
                "{line}"
            );
            let seconds: f64 = seconds
                .strip_suffix("s)")
                .expect("seconds suffix")
                .parse()
                .expect("numeric seconds");
            assert!(seconds.is_finite() && seconds >= 0.0, "{line}");
            (body, engine)
        })
        .collect()
}

fn assert_report_progress(output: &Output, value: &Value) {
    let lines = progress(output);
    assert!(matches!(
        lines[0].0,
        "mutate baseline: proved" | "mutate baseline: verified"
    ));
    assert!(matches!(lines[0].1, "explicit" | "bmc"));
    let mutants = value["mutants"].as_array().expect("mutants");
    assert_eq!(
        lines.len(),
        mutants.len() + 1,
        "one line per evaluated record plus baseline"
    );
    let builtin_count = mutants
        .iter()
        .filter(|record| record["source"] == "builtin")
        .count();
    let external_count = mutants.len() - builtin_count;
    let (mut builtin, mut external) = (0, 0);
    for ((body, engine), record) in lines[1..].iter().zip(mutants) {
        let status = record["status"].as_str().expect("status");
        let killer = record["killed_by"]
            .as_str()
            .map_or_else(String::new, |name| format!(" {name}"));
        if record["source"] == "builtin" {
            builtin += 1;
            assert_eq!(
                *body,
                format!(
                    "mutate [{builtin}/{builtin_count}] {} {}: {status}{killer}",
                    record["op"].as_str().expect("op"),
                    record["target"].as_str().expect("target")
                )
            );
        } else {
            external += 1;
            assert!(
                body.starts_with(&format!("mutate external [{external}/{external_count}] ")),
                "{body}"
            );
            assert!(
                body.contains(record["id"].as_str().expect("external id"))
                    || body.contains(record["target"].as_str().expect("external target")),
                "{body}"
            );
            assert!(body.ends_with(&format!(": {status}{killer}")), "{body}");
            if status == "invalid" {
                assert_eq!(*engine, "none", "invalid records never reach the oracle");
            }
        }
    }
}

#[test]
fn rbac_baseline_uses_explicit_and_keeps_legacy_verified_literal() {
    let output = run(RBAC);
    let value = report(&output, 0);
    let lines = progress(&output);
    assert_eq!(lines.len(), 1, "cap zero evaluates no mutants");
    assert!(matches!(
        lines[0].0,
        "mutate baseline: proved" | "mutate baseline: verified"
    ));
    assert_eq!(lines[0].1, "explicit");
    assert_eq!(value["baseline"], "verified");
}

#[test]
fn builtin_progress_uses_evaluated_count_not_the_larger_cap() {
    let output = run(BUILTIN);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 14);
    assert_report_progress(&output, &value);
}

#[test]
fn pointer_registry_has_a_per_mutant_explicit_route() {
    let output = run(POINTER);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 6);
    assert!(
        progress(&output)
            .iter()
            .any(|(body, engine)| body.starts_with("mutate [") && *engine == "explicit"),
        "baseline alone does not establish an explicit mutant shortcut"
    );
    assert_report_progress(&output, &value);
}

#[test]
fn external_only_progress_includes_invalid_and_duplicate_records() {
    let output = run(EXTERNAL);
    let value = report(&output, 0);
    assert_eq!(value["summary"]["total"], 4);
    assert_eq!(value["summary"]["invalid"], 2);
    // Duplicate IDs are invalid records, not command-level errors on main.
    assert!(
        value["mutants"][3]["invalid"]["message"]
            .as_str()
            .expect("duplicate diagnostic")
            .contains("duplicate external mutant id")
    );
    assert_report_progress(&output, &value);
}

#[test]
fn failing_gate_is_still_a_report_with_builtin_and_external_progress() {
    let output = run(EXTERNAL_GATE);
    let value = report(&output, 1);
    assert_eq!(value["summary"]["total"], 5);
    assert_eq!(value["summary"]["by_source"]["builtin"]["total"], 1);
    assert_eq!(value["summary"]["by_source"]["external"]["total"], 4);
    assert_report_progress(&output, &value);
}

#[test]
fn successful_report_stdout_is_byte_identical_to_main() {
    for (args, exit, expected) in [
        (
            RBAC,
            0,
            "0cf2ab9e73ebf5070f6be5ce4963fdc40d165cd4a3a83f46a8f05cd45e890494",
        ),
        (
            POINTER,
            0,
            "2b2a659698d848bf2bfa374916177015425bcfc6f5b534ebf613ce1ee148b43f",
        ),
        (
            BUILTIN,
            0,
            "0398133ba8110fe9ac57f5bd60f46e13b370a98f6d9cc5f1415a8c5b1caa09c4",
        ),
        (
            EXTERNAL,
            0,
            "9be842f189b8dc798d137baeb40ca36c852e78193acfad4b6ebd81196e4756b6",
        ),
        (
            EXTERNAL_GATE,
            1,
            "924572cefbd289bf5ac3a670b518cdd1a82d65ff73672efec77273b29f8d8aa6",
        ),
    ] {
        let output = run(args);
        report(&output, exit);
        assert_eq!(
            format!("{:x}", Sha256::digest(&output.stdout)),
            expected,
            "complete stdout drifted for {args:?}; no public engine fields or exclusions"
        );
    }
}

#[test]
fn unreadable_catalog_after_a_clean_baseline_is_quiet_and_byte_identical() {
    // Cap one matters: preloading must fail before even a built-in progress
    // line, not merely happen to stay quiet in an external-only invocation.
    let output = run(UNREADABLE);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("error JSON");
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "io");
    assert_eq!(
        format!("{:x}", Sha256::digest(&output.stdout)),
        "0378857af9ede852462b991c4c66b1c8013673f8ac882187cc1739a5b515b6f8"
    );
}

#[test]
fn refused_baseline_has_no_progress() {
    let output = run(&[
        "examples/gallery/errors/violated_invariant_counter.fsl",
        "--depth",
        "3",
        "--max-mutants",
        "1",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("baseline JSON");
    assert_eq!(value["result"], "violated");
    assert!(value.get("mutants").is_none());
}
