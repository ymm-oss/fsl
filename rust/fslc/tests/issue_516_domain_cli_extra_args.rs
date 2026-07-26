// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #516: `fslc domain replay` and `fslc domain
//! analyze` must reject an unrecognized trailing argument as
//! `result:"error"`/`kind:"usage"`/exit 2, matching every other `domain`
//! subcommand (`check`/`expand`/`generate`/`testgen`) and the rest of the
//! CLI, instead of silently discarding it and returning a result computed
//! without it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

const SPEC: &str = "examples/domain/order_async_effect.fsl";
const LOGS: &str = "examples/domain/order_async_effect_replay.jsonl";

#[test]
fn domain_replay_rejects_an_unknown_trailing_flag() {
    let (output, status) = run(&[
        "domain",
        "replay",
        SPEC,
        "--logs",
        LOGS,
        "--no-such-flag",
        "--depth",
        "99",
        "zzz",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "usage");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("--no-such-flag")),
        "{output:#}"
    );
}

#[test]
fn domain_replay_rejects_a_bare_unknown_flag() {
    let (output, status) = run(&["domain", "replay", SPEC, "--logs", LOGS, "--bogus"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "usage");
}

#[test]
fn domain_replay_rejects_an_extra_positional_argument() {
    let (output, status) = run(&["domain", "replay", SPEC, "--logs", LOGS, "EXTRA"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "usage");
}

#[test]
fn domain_analyze_rejects_an_unknown_trailing_flag() {
    let (output, status) = run(&["domain", "analyze", SPEC, "--bogus"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "usage");
}

/// Regression control: the correct, documented invocations of both
/// subcommands must keep succeeding, so rejecting residual arguments does
/// not over-trigger on well-formed calls.
#[test]
fn domain_replay_and_analyze_still_succeed_without_extra_arguments() {
    let (replay, replay_status) = run(&["domain", "replay", SPEC, "--logs", LOGS]);
    assert_eq!(replay_status, 0, "{replay:#}");

    let (analyze, analyze_status) = run(&["domain", "analyze", SPEC]);
    assert_eq!(analyze_status, 0, "{analyze:#}");
    assert_eq!(analyze["result"], "analyzed");
}
