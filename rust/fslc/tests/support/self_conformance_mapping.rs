// SPDX-License-Identifier: Apache-2.0

//! Shared test-only P1 observation and independent outcome mapping (#537 C7, #670).
//!
//! This module deliberately does not import `fslc_rust::outcome`. Both the C7
//! self-conformance suite and the Triangulated Assurance registry execute this
//! exact seam, so the common-mode fault operator can replace the registered
//! observer rather than an ancillary proxy.

use serde_json::Value;

#[derive(Debug)]
pub struct CliObservation {
    pub subcommand: &'static str,
    pub stdout_bytes: Vec<u8>,
    pub stderr_bytes: Vec<u8>,
    pub output: Value,
    pub exit_code: i32,
    pub binary_revision: &'static str,
}

fn result(observation: &CliObservation) -> Result<&str, String> {
    let reparsed: Value = serde_json::from_slice(&observation.stdout_bytes)
        .map_err(|error| format!("raw stdout stopped being JSON: {error}"))?;
    if reparsed != observation.output {
        return Err("parsed output does not match retained raw stdout".to_owned());
    }
    let _retained_stderr = observation.stderr_bytes.as_slice();
    if observation.binary_revision.is_empty() {
        return Err("missing binary revision".to_owned());
    }
    observation
        .output
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "missing string result: subcommand={} output={}",
                observation.subcommand, observation.output
            )
        })
}

fn mapped_session_action(
    observation: &CliObservation,
    action: &'static str,
    expected_exit: i32,
) -> Result<&'static str, String> {
    if observation.exit_code != expected_exit {
        return Err(format!(
            "result/exit contradiction: subcommand={} result={:?} kind={:?} exit={} expected={expected_exit}",
            observation.subcommand,
            observation.output.get("result"),
            observation.output.get("kind"),
            observation.exit_code,
        ));
    }
    Ok(action)
}

fn mapped_violated_session_action(observation: &CliObservation) -> Result<&'static str, String> {
    let action = mapped_session_action(observation, "verify_violated", 1)?;
    if observation
        .output
        .get("trace")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(format!(
            "violated result missing nonempty trace: {}",
            observation.output
        ));
    }
    if !observation.output["loc"]["line"].is_u64() || !observation.output["loc"]["column"].is_u64()
    {
        return Err(format!(
            "violated result missing source location: {}",
            observation.output
        ));
    }
    if !observation.output["violated_at_step"].is_u64() {
        return Err(format!(
            "violated result missing first violated step: {}",
            observation.output
        ));
    }
    Ok(action)
}

/// Independent native port of
/// `tests/test_self_conformance.py:314-395::cli_result_to_session_action`.
///
/// The tuple includes the directly observed process exit code. An unlisted
/// tuple is an error, never a fallback to a success or failure action.
pub fn cli_result_to_session_action(observation: &CliObservation) -> Result<&'static str, String> {
    let result = result(observation)?;
    let kind = observation.output.get("kind").and_then(Value::as_str);
    let user_error = matches!(kind, Some("parse" | "semantics" | "io" | "usage" | "type"));

    match observation.subcommand {
        "check" => match (result, kind) {
            ("ok", _) => mapped_session_action(observation, "check_ok", 0),
            ("error", _) if user_error => mapped_session_action(observation, "check_err", 2),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "verify" => match (result, kind) {
            ("verified", _) => mapped_session_action(observation, "verify_ok", 0),
            ("violated", _) => mapped_violated_session_action(observation),
            ("reachable_failed", _) => {
                mapped_session_action(observation, "verify_reachable_failed", 1)
            }
            ("error", _) if user_error => {
                mapped_session_action(observation, "verify_user_error", 2)
            }
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "induction" => match (result, kind) {
            ("proved", _) => mapped_session_action(observation, "induction_proved", 0),
            ("unknown_cti", _) => mapped_session_action(observation, "induction_cti", 1),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "scenarios" => match (result, kind) {
            ("scenarios", _) => mapped_session_action(observation, "scenarios_ok", 0),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "explain" => match (result, kind) {
            ("explained", _) => mapped_session_action(observation, "explained_ok", 0),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "mutate" => match (result, kind) {
            ("mutated", _) => mapped_session_action(observation, "mutated_ok", 0),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "typestate" => match (result, kind) {
            ("typestate", _) => mapped_session_action(observation, "typestate_ok", 0),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "refine" => match (result, kind) {
            ("refines", _) => mapped_session_action(observation, "refines_ok", 0),
            ("refinement_failed", _) => mapped_session_action(observation, "refine_failed", 1),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        "replay" => match (result, kind) {
            ("conformant", _) => mapped_session_action(observation, "replay_conformant", 0),
            ("nonconformant", _) => mapped_session_action(observation, "replay_nonconformant", 1),
            ("error", Some("internal")) => mapped_session_action(observation, "tool_fault", 3),
            _ => Err(unmapped_session_tuple(observation)),
        },
        _ => Err(unmapped_session_tuple(observation)),
    }
}

fn unmapped_session_tuple(observation: &CliObservation) -> String {
    format!(
        "unmapped CLI observation: subcommand={} result={:?} kind={:?} exit={}",
        observation.subcommand,
        observation.output.get("result"),
        observation.output.get("kind"),
        observation.exit_code
    )
}
