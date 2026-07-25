// SPDX-License-Identifier: Apache-2.0

//! Negative controls for `fslc scenarios` reporting-surface defects
//! (issues #522, #523, #526): the verdict machinery is correct, but what
//! gets reported is wrong, missing, or actively misleading. Every test here
//! fails if the corresponding fix is reverted.

use std::process::Command;

fn run_cli(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

const DEADLOCK_FIXTURE: &str = "rust/fslc/tests/fixtures/explicit_deadlock.fsl";
const BLOCKED_ACTION_FIXTURE: &str = "rust/fslc/tests/fixtures/scenario_blocked_action.fsl";
const PARTIAL_BINDING_FIXTURE: &str =
    "rust/fslc/tests/fixtures/scenario_leadsto_partial_binding.fsl";
const DELAYED_BINDING_FIXTURE: &str =
    "rust/fslc/tests/fixtures/scenario_leadsto_delayed_binding.fsl";

// ---------------------------------------------------------------------
// #522 — `--deadlock error` must keep the verify verdict and exit code.
// ---------------------------------------------------------------------

#[test]
fn scenarios_deadlock_error_reports_violated_and_exits_one() {
    let (value, status) = run_cli(&[
        "scenarios",
        DEADLOCK_FIXTURE,
        "--depth",
        "4",
        "--deadlock",
        "error",
    ]);
    assert_eq!(status, 1, "{value}");
    assert_eq!(value["result"], "violated", "{value}");
    assert_eq!(value["violation_kind"], "deadlock", "{value}");
}

#[test]
fn scenarios_deadlock_warn_still_generates_with_a_note() {
    let (value, status) = run_cli(&[
        "scenarios",
        DEADLOCK_FIXTURE,
        "--depth",
        "4",
        "--deadlock",
        "warn",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "scenarios", "{value}");
    let scenarios = value["scenarios"].as_array().expect("scenarios array");
    let deadlock = scenarios
        .iter()
        .find(|scenario| scenario["kind"] == "deadlock")
        .unwrap_or_else(|| panic!("deadlock scenario present in {value}"));
    assert_eq!(
        deadlock["note"], "after these steps no action is enabled",
        "{deadlock}"
    );
}

#[test]
fn scenarios_terminal_state_still_succeeds() {
    // Positive control: an intentional terminal state (no `--deadlock`
    // promotion involved) must remain unaffected by the #522 fix.
    let (value, status) = run_cli(&[
        "scenarios",
        "rust/fslc/tests/fixtures/scenario_bool_guard_terminal.fsl",
        "--depth",
        "8",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "scenarios", "{value}");
}

// ---------------------------------------------------------------------
// #523 — a never-enabled action must not be described as "was enabled".
// ---------------------------------------------------------------------

#[test]
fn scenarios_never_enabled_action_is_not_reported_as_enabled() {
    let (value, status) = run_cli(&["scenarios", BLOCKED_ACTION_FIXTURE, "--depth", "4"]);
    assert_eq!(status, 0, "{value}");
    let warnings = value["warnings"].as_array().expect("warnings array");
    let warning = warnings
        .iter()
        .find(|warning| {
            warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("'bad'"))
        })
        .unwrap_or_else(|| panic!("a warning naming 'bad' in {value}"));
    let message = warning["message"].as_str().expect("message string");
    assert!(
        message.contains("is never enabled"),
        "must say the action was never enabled: {message}"
    );
    assert!(
        !message.contains("was enabled"),
        "must not claim the blocked action was enabled: {message}"
    );
    assert!(warning.get("blocking_requires").is_some(), "{warning}");
}

#[test]
fn scenarios_genuinely_covered_action_gets_no_blocked_warning() {
    // Positive control: `ok` is unconditionally enabled and covered, so it
    // must carry neither wording.
    let (value, status) = run_cli(&["scenarios", BLOCKED_ACTION_FIXTURE, "--depth", "4"]);
    assert_eq!(status, 0, "{value}");
    let warnings = value["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().all(|warning| !warning["message"]
            .as_str()
            .is_some_and(|message| message.contains("'ok'"))),
        "{value}"
    );
}

// ---------------------------------------------------------------------
// #526 — quantified leadsTo completeness must warn per binding.
// ---------------------------------------------------------------------

#[test]
fn scenarios_leadsto_witnessed_binding_does_not_hide_the_other_bindings_warning() {
    let (value, status) = run_cli(&["scenarios", PARTIAL_BINDING_FIXTURE, "--depth", "4"]);
    assert_eq!(status, 0, "{value}");
    let scenarios = value["scenarios"].as_array().expect("scenarios array");
    let responded = scenarios
        .iter()
        .filter(|scenario| scenario["kind"] == "leadsTo")
        .map(|scenario| scenario["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(responded, vec!["respond_MaybePending_p0"], "{value}");

    let warnings = value["warnings"].as_array().expect("warnings array");
    let warning = warnings
        .iter()
        .find(|warning| {
            warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("MaybePending"))
        })
        .unwrap_or_else(|| panic!("a MaybePending completeness warning in {value}"));
    let message = warning["message"].as_str().expect("message string");
    assert!(message.contains("p=1"), "{message}");
    assert!(
        message.contains("never holds"),
        "binding 1's antecedent never holds and must say so: {message}"
    );
}

#[test]
fn scenarios_leadsto_delayed_binding_warns_distinctly_from_an_impossible_one() {
    let (value, status) = run_cli(&["scenarios", DELAYED_BINDING_FIXTURE, "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    let scenarios = value["scenarios"].as_array().expect("scenarios array");
    let responded = scenarios
        .iter()
        .filter(|scenario| scenario["kind"] == "leadsTo")
        .map(|scenario| scenario["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(responded, vec!["respond_Responds_p0"], "{value}");

    let warnings = value["warnings"].as_array().expect("warnings array");
    let warning = warnings
        .iter()
        .find(|warning| {
            warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("Responds"))
        })
        .unwrap_or_else(|| panic!("a Responds completeness warning in {value}"));
    let message = warning["message"].as_str().expect("message string");
    assert!(message.contains("p=1"), "{message}");
    assert!(
        message.contains("no response scenario"),
        "binding 1 triggered but never closed and must say so, not 'never holds': {message}"
    );
    assert!(
        !message.contains("never holds"),
        "a triggered-but-incomplete binding must not read like an impossible one: {message}"
    );
}

#[test]
fn scenarios_leadsto_both_bindings_witnessed_removes_both_warnings() {
    // Positive control: raising depth far enough for both bindings to
    // respond must clear every leadsTo completeness warning.
    let (value, status) = run_cli(&["scenarios", DELAYED_BINDING_FIXTURE, "--depth", "10"]);
    assert_eq!(status, 0, "{value}");
    let scenarios = value["scenarios"].as_array().expect("scenarios array");
    let mut responded = scenarios
        .iter()
        .filter(|scenario| scenario["kind"] == "leadsTo")
        .map(|scenario| scenario["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    responded.sort_unstable();
    assert_eq!(
        responded,
        vec!["respond_Responds_p0", "respond_Responds_p1"],
        "{value}"
    );
    let warnings = value["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().all(|warning| !warning["message"]
            .as_str()
            .is_some_and(|message| message.contains("Responds"))),
        "{value}"
    );
}
