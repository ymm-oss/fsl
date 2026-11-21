// SPDX-License-Identifier: Apache-2.0

//! Issue #1961: `verify --engine explicit` and `--engine auto` evaluate the
//! terminal on every dead state of the level where BMC asks for a partial
//! terminal, so all three engines report the same `partial_op`.
//!
//! Before #1961 the first dead non-terminal state of a level recorded the
//! deadlock and later dead states of that level skipped the terminal, so
//! `issue_1961_terminal_partial_second.fsl` closed as `proved` with a
//! deadlock warning under explicit and auto while BMC reported `violated`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn verify(name: &str, engine: &str) -> (Value, i32) {
    let output = fslc_cli::command()
        .arg("verify")
        .arg(fixture(name))
        .args(["--engine", engine])
        .output()
        .expect("run fslc verify");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("{name} {engine}: not JSON ({error}): {stdout}"));
    (value, output.status.code().expect("exit code"))
}

/// Each engine reports the same verdict, kind, name, step and state.
fn assert_partial_terminal(name: &str, engine: &str, last_state: &Value) {
    let (output, status) = verify(name, engine);
    assert_eq!(status, 1, "{name} {engine}: {output:#}");
    assert_eq!(output["result"], "violated", "{name} {engine}: {output:#}");
    assert_eq!(
        output["violation_kind"], "partial_op",
        "{name} {engine}: {output:#}"
    );
    assert_eq!(
        output["invariant"], "_partial_property_terminal",
        "{name} {engine}: {output:#}"
    );
    assert_eq!(output["violated_at_step"], 1, "{name} {engine}: {output:#}");
    let trace = output["trace"].as_array().expect("trace");
    assert_eq!(
        &trace.last().expect("trace step")["state"],
        last_state,
        "{name} {engine}: {output:#}"
    );
    if engine == "auto" {
        // The verdict is explicit's own, not a BMC fallback.
        assert_eq!(output["engine"], "explicit", "{name} {engine}: {output:#}");
    }
}

const SECOND: &str = "issue_1961_terminal_partial_second.fsl";
const FIRST: &str = "issue_1961_terminal_partial_first.fsl";

fn second_state() -> Value {
    json!({"a_first": true, "done": true, "q": []})
}

fn first_state() -> Value {
    json!({"a_first": false, "done": true, "q": []})
}

#[test]
fn second_dead_state_explicit() {
    assert_partial_terminal(SECOND, "explicit", &second_state());
}

#[test]
fn second_dead_state_auto() {
    assert_partial_terminal(SECOND, "auto", &second_state());
}

#[test]
fn second_dead_state_bmc() {
    assert_partial_terminal(SECOND, "bmc", &second_state());
}

#[test]
fn first_dead_state_explicit() {
    assert_partial_terminal(FIRST, "explicit", &first_state());
}

#[test]
fn first_dead_state_auto() {
    assert_partial_terminal(FIRST, "auto", &first_state());
}

#[test]
fn first_dead_state_bmc() {
    assert_partial_terminal(FIRST, "bmc", &first_state());
}
