// SPDX-License-Identifier: Apache-2.0

//! Issue #1961: `verify --engine explicit` and `--engine auto` evaluate the
//! terminal on every dead state of the level where BMC asks for a partial
//! terminal, so all three engines report the same `partial_op`. The same
//! holds for a `reachable` that one state of a level witnesses and another
//! state of that level cannot evaluate.
//!
//! Before #1961 the first dead non-terminal state of a level recorded the
//! deadlock and later dead states of that level skipped the terminal, so
//! `issue_1961_terminal_partial_second.fsl` closed as `proved` with a
//! deadlock warning under explicit and auto while BMC reported `violated`.
//!
//! The terminal failure of a level is reported only after the level's
//! stepping has found no action partial operation, which BMC checks first
//! (#1962 review): `issue_1961_action_partial_before_terminal.fsl`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn verify(name: &str, engine: &str, depth: Option<&str>) -> (Value, i32) {
    let mut command = fslc_cli::command();
    command
        .arg("verify")
        .arg(fixture(name))
        .args(["--engine", engine]);
    if let Some(depth) = depth {
        command.args(["--depth", depth]);
    }
    let output = command.output().expect("run fslc verify");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("{name} {engine}: not JSON ({error}): {stdout}"));
    (value, output.status.code().expect("exit code"))
}

/// Each engine reports the same verdict, kind, name, step and state.
fn assert_partial_terminal(name: &str, engine: &str, last_state: &Value) {
    assert_partial(name, engine, "_partial_property_terminal", last_state);
}

fn assert_partial(name: &str, engine: &str, property: &str, last_state: &Value) {
    assert_partial_at(name, None, engine, (property, 1), last_state);
}

/// [`assert_partial`] at `depth` (the default when `None`), for the
/// `partial_op` `property` reported at `step`.
fn assert_partial_at(
    name: &str,
    depth: Option<&str>,
    engine: &str,
    (property, step): (&str, usize),
    last_state: &Value,
) {
    let (output, status) = verify(name, engine, depth);
    assert_eq!(status, 1, "{name} {engine}: {output:#}");
    assert_eq!(output["result"], "violated", "{name} {engine}: {output:#}");
    assert_eq!(
        output["violation_kind"], "partial_op",
        "{name} {engine}: {output:#}"
    );
    assert_eq!(output["invariant"], property, "{name} {engine}: {output:#}");
    assert_eq!(
        output["violated_at_step"], step,
        "{name} {engine}: {output:#}"
    );
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

const REACHABLE: &str = "issue_1961_reachable_partial_second.fsl";

fn reachable_state() -> Value {
    json!({"a_first": true, "done": true, "i": 1, "q": [1]})
}

#[test]
fn reachable_witnessed_earlier_in_the_level_explicit() {
    assert_partial(
        REACHABLE,
        "explicit",
        "_partial_property_R",
        &reachable_state(),
    );
}

#[test]
fn reachable_witnessed_earlier_in_the_level_auto() {
    assert_partial(REACHABLE, "auto", "_partial_property_R", &reachable_state());
}

#[test]
fn reachable_witnessed_earlier_in_the_level_bmc() {
    assert_partial(REACHABLE, "bmc", "_partial_property_R", &reachable_state());
}

const ACTION_BEFORE_TERMINAL: &str = "issue_1961_action_partial_before_terminal.fsl";

fn crash_state() -> Value {
    json!({"done": true, "k": 3, "q": []})
}

#[test]
fn action_partial_before_terminal_explicit() {
    assert_partial_at(
        ACTION_BEFORE_TERMINAL,
        None,
        "explicit",
        ("_partial_crash", 2),
        &crash_state(),
    );
}

#[test]
fn action_partial_before_terminal_auto() {
    assert_partial_at(
        ACTION_BEFORE_TERMINAL,
        None,
        "auto",
        ("_partial_crash", 2),
        &crash_state(),
    );
}

#[test]
fn action_partial_before_terminal_bmc() {
    assert_partial_at(
        ACTION_BEFORE_TERMINAL,
        None,
        "bmc",
        ("_partial_crash", 2),
        &crash_state(),
    );
}

/// At the depth no action of the last level is checked, so the terminal is.
#[test]
fn action_partial_before_terminal_at_depth_one() {
    for engine in ["explicit", "auto", "bmc"] {
        assert_partial_at(
            ACTION_BEFORE_TERMINAL,
            Some("1"),
            engine,
            ("_partial_property_terminal", 1),
            &json!({"done": true, "k": 2, "q": []}),
        );
    }
}

const ACTION_BEFORE_INVARIANT: &str = "issue_1961_action_partial_before_invariant.fsl";

#[test]
fn action_partial_before_a_later_invariant_violation() {
    for engine in ["explicit", "auto", "bmc"] {
        assert_partial_at(
            ACTION_BEFORE_INVARIANT,
            None,
            engine,
            ("_partial_crash", 2),
            &json!({"done": true, "k": 2, "q": []}),
        );
    }
}

const PARTIAL_GUARD_TERMINAL: &str = "issue_1961_partial_guard_terminal.fsl";

/// A state whose only candidate is a partial guard is dead for its terminal
/// at the depth, where BMC checks no action partial operation.
#[test]
fn partial_guard_state_terminal_at_depth_one() {
    for engine in ["explicit", "auto", "bmc"] {
        assert_partial_at(
            PARTIAL_GUARD_TERMINAL,
            Some("1"),
            engine,
            ("_partial_property_terminal", 1),
            &json!({"done": true, "k": 3, "q": []}),
        );
    }
}

/// Below the depth the guard's own partial operation comes first.
#[test]
fn partial_guard_state_terminal_at_depth_two() {
    for engine in ["explicit", "auto", "bmc"] {
        assert_partial_at(
            PARTIAL_GUARD_TERMINAL,
            Some("2"),
            engine,
            ("_partial_g", 2),
            &json!({"done": true, "k": 3, "q": []}),
        );
    }
}

/// The same state is a deadlock at the depth.
#[test]
fn partial_guard_state_is_a_deadlock_at_depth_one() {
    let name = "issue_1961_partial_guard_deadlock.fsl";
    for engine in ["explicit", "auto", "bmc"] {
        let output = fslc_cli::command()
            .arg("verify")
            .arg(fixture(name))
            .args(["--engine", engine, "--depth", "1", "--deadlock", "error"])
            .output()
            .expect("run fslc verify");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("{name} {engine}: not JSON ({error}): {stdout}"));
        assert_eq!(output.status.code(), Some(1), "{engine}: {value:#}");
        assert_eq!(value["result"], "violated", "{engine}: {value:#}");
        assert_eq!(value["violation_kind"], "deadlock", "{engine}: {value:#}");
        assert_eq!(value["violated_at_step"], 1, "{engine}: {value:#}");
    }
}

/// A partial guard that BMC's encoding reads as `true` (division totalized)
/// keeps its state alive at the depth: no terminal evaluation, no deadlock.
#[test]
fn totalized_partial_guard_keeps_the_state_alive_at_the_depth() {
    let name = "issue_1961_partial_guard_totalized.fsl";
    for engine in ["explicit", "auto", "bmc"] {
        let output = fslc_cli::command()
            .arg("verify")
            .arg(fixture(name))
            .args(["--engine", engine, "--depth", "0", "--deadlock", "error"])
            .output()
            .expect("run fslc verify");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("{name} {engine}: not JSON ({error}): {stdout}"));
        assert_eq!(output.status.code(), Some(0), "{engine}: {value:#}");
        assert_eq!(value["result"], "verified", "{engine}: {value:#}");
        assert_eq!(value["deadlock"]["found"], false, "{engine}: {value:#}");
    }
    for engine in ["explicit", "auto", "bmc"] {
        assert_partial_at(
            name,
            Some("1"),
            engine,
            ("_partial_a", 1),
            &json!({"x": 0, "q": []}),
        );
    }
}

fn verify_args(name: &str, engine: &str, args: &[&str]) -> (Value, i32) {
    let output = fslc_cli::command()
        .arg("verify")
        .arg(fixture(name))
        .args(["--engine", engine])
        .args(args)
        .output()
        .expect("run fslc verify");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("{name} {engine}: not JSON ({error}): {stdout}"));
    (value, output.status.code().expect("exit code"))
}

/// A state-budget stop below the depth does not record a state whose only
/// candidate is a partial guard as the deadlock: that guard's `partial_op`
/// would come first.
#[test]
fn budget_stop_records_no_deadlock_for_a_partial_guard_state() {
    let name = "issue_1961_budget_partial_guard.fsl";
    let args = ["--explicit-budget", "3", "--deadlock", "error"];
    let (output, _) = verify_args(name, "explicit", &args);
    assert_eq!(output["result"], "unknown_budget", "explicit: {output:#}");
    for engine in ["auto", "bmc"] {
        let (output, status) = verify_args(name, engine, &args);
        assert_eq!(status, 1, "{engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "{engine}: {output:#}"
        );
        assert_eq!(output["invariant"], "_partial_g", "{engine}: {output:#}");
        assert_eq!(output["violated_at_step"], 2, "{engine}: {output:#}");
    }
}
