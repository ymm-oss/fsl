// SPDX-License-Identifier: Apache-2.0

//! Native FSL self-conformance anchors for issue #537 C7.
//!
//! This test intentionally does not import `fslc_rust::outcome`. The production
//! classifier is one side of the contract under test, so reusing it here would
//! let the implementation and its oracle repeat the same defect.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

const SESSION_SPEC: &str = "examples/self/fslc_session.fsl";
const MONITOR_SPEC: &str = "examples/self/fslc_monitor.fsl";
const FOLD_SPEC: &str = "examples/self/fslc_fold.fsl";
const CART_SPEC: &str = "specs/cart_v1.fsl";

#[derive(Debug)]
struct CliObservation {
    subcommand: &'static str,
    output: Value,
    exit_code: i32,
}

#[derive(Debug)]
struct RawCliOutput {
    output: Value,
    exit_code: i32,
}

#[derive(Clone, Copy, Debug)]
enum CompoundCommand {
    Sweep,
    Chain,
    AnalyzeBatch,
}

#[derive(Clone, Copy)]
struct CorpusCase {
    id: &'static str,
    path: &'static str,
    verify_args: &'static [&'static str],
    induction_args: &'static [&'static str],
    run_verify: bool,
    run_induction: bool,
}

// Transcribed from tests/test_self_conformance.py:39-67. Keep paths,
// subcommands, and arguments aligned with the frozen compatibility anchor.
const CORPUS: &[CorpusCase] = &[
    CorpusCase {
        id: "happy_pipeline",
        path: "examples/self/fslc_session.fsl",
        verify_args: &["--depth", "8", "--deadlock", "warn"],
        induction_args: &[
            "--depth",
            "8",
            "--deadlock",
            "warn",
            "--engine",
            "induction",
        ],
        run_verify: true,
        run_induction: true,
    },
    CorpusCase {
        id: "violated",
        path: "examples/gallery/errors/violated_invariant_counter.fsl",
        verify_args: &["--depth", "2"],
        induction_args: &["--depth", "8", "--engine", "induction"],
        run_verify: true,
        run_induction: false,
    },
    CorpusCase {
        id: "reachable_failed",
        path: "examples/gallery/injected/bank__over_strengthened_guard.fsl",
        verify_args: &["--depth", "8"],
        induction_args: &["--depth", "8", "--engine", "induction"],
        run_verify: true,
        run_induction: false,
    },
    CorpusCase {
        id: "check_parse_error",
        path: "examples/gallery/errors/parse_missing_expression.fsl",
        verify_args: &[],
        induction_args: &[],
        run_verify: false,
        run_induction: false,
    },
    CorpusCase {
        id: "check_type_error",
        path: "examples/gallery/errors/type_undeclared_type.fsl",
        verify_args: &[],
        induction_args: &[],
        run_verify: false,
        run_induction: false,
    },
];

#[derive(Clone, Copy)]
enum ReplayFixture {
    Conformant,
    Nonconformant,
}

#[derive(Clone, Copy)]
struct SubcommandAnchorCase {
    id: &'static str,
    check_path: &'static str,
    subcommand: &'static str,
    argv: &'static [&'static str],
    refine_abs: Option<&'static str>,
    refine_mapping: Option<&'static str>,
    replay_fixture: Option<ReplayFixture>,
}

// Transcribed from tests/test_self_conformance.py:85-152.
const SUBCOMMAND_CORPUS: &[SubcommandAnchorCase] = &[
    SubcommandAnchorCase {
        id: "verify_user_error",
        check_path: "examples/self/no_actions.fsl",
        subcommand: "verify",
        argv: &["--depth", "1"],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "scenarios_ok",
        check_path: CART_SPEC,
        subcommand: "scenarios",
        argv: &["--depth", "8"],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "explained_ok",
        check_path: CART_SPEC,
        subcommand: "explain",
        argv: &["--depth", "4"],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "mutated_ok",
        check_path: CART_SPEC,
        subcommand: "mutate",
        argv: &["--depth", "4"],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "typestate_ok",
        check_path: "specs/order_workflow.fsl",
        subcommand: "typestate",
        argv: &[],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "refines_ok",
        check_path: "examples/refinement_chain/bot.fsl",
        subcommand: "refine",
        argv: &["--depth", "6"],
        refine_abs: Some("examples/refinement_chain/mid.fsl"),
        refine_mapping: Some("examples/refinement_chain/bot_refines_mid.fsl"),
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "refine_failed",
        check_path: "examples/gallery/errors/refinement_failed_impl.fsl",
        subcommand: "refine",
        argv: &["--depth", "3"],
        refine_abs: Some("examples/gallery/errors/refinement_failed_abs.fsl"),
        refine_mapping: Some("examples/gallery/errors/refinement_failed_map.fsl"),
        replay_fixture: None,
    },
    SubcommandAnchorCase {
        id: "replay_conformant",
        check_path: CART_SPEC,
        subcommand: "replay",
        argv: &[],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: Some(ReplayFixture::Conformant),
    },
    SubcommandAnchorCase {
        id: "replay_nonconformant",
        check_path: CART_SPEC,
        subcommand: "replay",
        argv: &[],
        refine_abs: None,
        refine_mapping: None,
        replay_fixture: Some(ReplayFixture::Nonconformant),
    },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let directory = root().join("rust/target").join(format!(
        "self-conformance-{name}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create self-conformance scratch directory");
    directory
}

fn parse_cli_output(arguments: &[String], output: &Output) -> RawCliOutput {
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "native fslc emitted invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let exit_code = output.status.code().unwrap_or_else(|| {
        panic!(
            "native fslc terminated without an exit code; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    RawCliOutput {
        output: value,
        exit_code,
    }
}

fn run_cli_at(cwd: &Path, arguments: &[String]) -> RawCliOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run native fslc");
    parse_cli_output(arguments, &output)
}

fn run_cli(arguments: &[String]) -> RawCliOutput {
    run_cli_at(&root(), arguments)
}

fn strings(arguments: &[&str]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

fn write_json_fixture(name: &str, value: &Value) -> PathBuf {
    let directory = scratch_dir(name);
    let path = directory.join("trace.json");
    fs::write(
        &path,
        serde_json::to_vec(value).expect("serialize trace fixture"),
    )
    .expect("write trace fixture");
    path
}

fn observe(subcommand: &'static str, arguments: &[String]) -> CliObservation {
    let observed = run_cli(arguments);
    CliObservation {
        subcommand,
        output: observed.output,
        exit_code: observed.exit_code,
    }
}

fn result(observation: &CliObservation) -> Result<&str, String> {
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
fn cli_result_to_session_action(observation: &CliObservation) -> Result<&'static str, String> {
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

fn run_model_pipeline(case: CorpusCase) -> Vec<CliObservation> {
    let mut observations = Vec::new();
    observations.push(observe("check", &strings(&["check", case.path])));
    if observations[0].output["result"] != "ok" {
        return observations;
    }
    assert!(
        case.run_verify,
        "{} unexpectedly checked ok; the transcribed corpus marks it check-only because check must fail",
        case.id
    );

    let mut verify = strings(&["verify", case.path]);
    verify.extend(
        case.verify_args
            .iter()
            .map(|argument| (*argument).to_owned()),
    );
    observations.push(observe("verify", &verify));
    if observations[1].output["result"] != "verified" || !case.run_induction {
        return observations;
    }

    let mut induction = strings(&["verify", case.path]);
    induction.extend(
        case.induction_args
            .iter()
            .map(|argument| (*argument).to_owned()),
    );
    observations.push(observe("induction", &induction));
    observations
}

fn cart_events(fixture: ReplayFixture) -> Vec<Value> {
    let conformant = vec![
        json!({"action":"add_to_cart","params":{"u":0,"i":0}}),
        json!({"action":"checkout","params":{"u":0}}),
    ];
    match fixture {
        ReplayFixture::Conformant => conformant,
        ReplayFixture::Nonconformant => vec![
            json!({"action":"add_to_cart","params":{"u":0,"i":0}}),
            json!({"action":"add_to_cart","params":{"u":0,"i":1}}),
            json!({"action":"checkout","params":{"u":0}}),
        ],
    }
}

fn run_subcommand_anchor(case: SubcommandAnchorCase) -> Vec<CliObservation> {
    let check = observe("check", &strings(&["check", case.check_path]));
    assert_eq!(
        check.output["result"], "ok",
        "{} prerequisite check failed: {}",
        case.id, check.output
    );
    let mut observations = vec![check];

    let arguments = if case.subcommand == "refine" {
        let mut arguments = strings(&[
            "refine",
            case.check_path,
            case.refine_abs.expect("refine abs"),
            case.refine_mapping.expect("refine mapping"),
        ]);
        arguments.extend(case.argv.iter().map(|argument| (*argument).to_owned()));
        arguments
    } else if case.subcommand == "replay" {
        let events = cart_events(case.replay_fixture.expect("replay fixture"));
        let path = write_json_fixture(case.id, &Value::Array(events));
        let mut arguments = strings(&["replay", CART_SPEC, "--trace"]);
        arguments.push(path.to_string_lossy().into_owned());
        arguments.extend(case.argv.iter().map(|argument| (*argument).to_owned()));
        arguments
    } else {
        let mut arguments = strings(&[case.subcommand, case.check_path]);
        arguments.extend(case.argv.iter().map(|argument| (*argument).to_owned()));
        arguments
    };
    observations.push(observe(case.subcommand, &arguments));
    observations
}

fn replay_actions(spec: &str, actions: &[Value]) -> RawCliOutput {
    let path = write_json_fixture("model-replay", &Value::Array(actions.to_vec()));
    let mut arguments = strings(&["replay", spec, "--trace"]);
    arguments.push(path.to_string_lossy().into_owned());
    run_cli(&arguments)
}

fn session_trace(observations: &[CliObservation]) -> Vec<Value> {
    observations
        .iter()
        .map(|observation| {
            let action = cli_result_to_session_action(observation)
                .unwrap_or_else(|error| panic!("{error}; output={}", observation.output));
            json!({"action":action})
        })
        .collect()
}

fn assert_conformant(spec: &str, trace: &[Value], context: &str) -> RawCliOutput {
    let replay = replay_actions(spec, trace);
    assert_eq!(
        replay.output["result"], "conformant",
        "{context}: trace={trace:?}; replay={}",
        replay.output
    );
    assert_eq!(replay.exit_code, 0, "{context}: {}", replay.output);
    replay
}

fn assert_nonconformant(spec: &str, trace: &[Value], context: &str) {
    let replay = replay_actions(spec, trace);
    assert_eq!(
        replay.output["result"], "nonconformant",
        "{context}: trace={trace:?}; replay={}",
        replay.output
    );
    assert_eq!(replay.exit_code, 1, "{context}: {}", replay.output);
}

#[test]
fn native_session_corpus_observations_replay_conformantly() {
    for case in CORPUS {
        let observations = run_model_pipeline(*case);
        let trace = session_trace(&observations);
        assert_conformant(SESSION_SPEC, &trace, case.id);
    }
    for case in SUBCOMMAND_CORPUS {
        let observations = run_subcommand_anchor(*case);
        let trace = session_trace(&observations);
        assert_conformant(SESSION_SPEC, &trace, case.id);
    }
}

#[test]
fn session_contract_violations_are_rejected() {
    // First two traces are transcribed from
    // tests/test_self_conformance.py:488-504. The third is C7's explicit
    // failure-to-success promotion negative control.
    let negative_traces = [
        (
            "verify_ok_without_check_ok",
            vec![json!({"action":"verify_ok"})],
        ),
        (
            "induction_proved_without_verify_ok",
            vec![
                json!({"action":"check_ok"}),
                json!({"action":"induction_proved"}),
            ],
        ),
        (
            "failure_promoted_without_repair",
            vec![
                json!({"action":"check_ok"}),
                json!({"action":"verify_violated"}),
                json!({"action":"verify_ok"}),
            ],
        ),
    ];
    for (name, trace) in negative_traces {
        assert_nonconformant(SESSION_SPEC, &trace, name);
    }
}

#[test]
fn session_mapping_rejects_result_exit_contradictions() {
    let contradictory = CliObservation {
        subcommand: "verify",
        output: json!({"result":"violated","kind":"invariant"}),
        exit_code: 0,
    };
    let error =
        cli_result_to_session_action(&contradictory).expect_err("violated/exit-0 must fail closed");
    assert!(
        error.contains("result/exit contradiction"),
        "unexpected error: {error}"
    );

    let actual_arguments = strings(&[
        "verify",
        "examples/gallery/errors/violated_invariant_counter.fsl",
        "--depth",
        "2",
    ]);
    let actual = observe("verify", &actual_arguments);
    assert_eq!(actual.output["result"], "violated", "{}", actual.output);
    assert!(
        actual.output["trace"]
            .as_array()
            .is_some_and(|trace| !trace.is_empty()),
        "failure lost its replayable trace: {}",
        actual.output
    );
    assert!(
        actual.output["loc"]["line"].is_u64() && actual.output["loc"]["column"].is_u64(),
        "failure lost its source location: {}",
        actual.output
    );
    assert!(
        actual.output["violated_at_step"].is_u64(),
        "failure lost its first-divergence evidence: {}",
        actual.output
    );
    assert_eq!(
        cli_result_to_session_action(&actual).expect("real failure tuple"),
        "verify_violated"
    );

    for (field, replacement) in [
        ("trace", Value::Array(Vec::new())),
        ("loc", json!({"line":8,"column":"missing"})),
        ("violated_at_step", Value::Null),
    ] {
        let mut missing = actual.output.clone();
        missing
            .as_object_mut()
            .expect("CLI output object")
            .insert(field.to_owned(), replacement);
        let missing = CliObservation {
            subcommand: "verify",
            output: missing,
            exit_code: 1,
        };
        assert!(
            cli_result_to_session_action(&missing).is_err(),
            "missing/corrupt {field} must fail closed"
        );
    }
}

#[test]
fn mutate_failure_verdict_cannot_exit_zero() {
    // This is the native sensitivity point for
    // fault_operators/failure-verdict-exits-zero.patch. It deliberately uses
    // a non-clean mutate baseline, the path issue #554 inverted.
    let observed = run_cli(&strings(&[
        "mutate",
        "examples/gallery/errors/violated_invariant_counter.fsl",
        "--depth",
        "2",
    ]));
    assert_eq!(observed.output["result"], "violated", "{}", observed.output);
    assert_eq!(
        observed.exit_code, 1,
        "failure verdict must exit 1, never zero: {}",
        observed.output
    );
}

#[test]
fn semantics_error_input_never_maps_to_verified() {
    let check = observe(
        "check",
        &strings(&["check", "examples/self/no_actions.fsl"]),
    );
    assert_eq!(check.output["result"], "ok", "{}", check.output);
    let verification = observe(
        "verify",
        &strings(&["verify", "examples/self/no_actions.fsl", "--depth", "1"]),
    );
    assert_eq!(
        verification.output["result"], "error",
        "{}",
        verification.output
    );
    assert_eq!(
        verification.output["kind"], "semantics",
        "{}",
        verification.output
    );
    assert_ne!(verification.output["result"], "verified");
    assert_eq!(
        cli_result_to_session_action(&verification).expect("mapped user error"),
        "verify_user_error"
    );
    assert_conformant(
        SESSION_SPEC,
        &session_trace(&[check, verification]),
        "no-actions user error",
    );
}

fn replay_cart(events: &[Value]) -> RawCliOutput {
    let path = write_json_fixture("cart-replay", &Value::Array(events.to_vec()));
    let mut arguments = strings(&["replay", CART_SPEC, "--trace"]);
    arguments.push(path.to_string_lossy().into_owned());
    run_cli(&arguments)
}

/// Independent native port of
/// `tests/test_self_conformance.py:426-445::replay_out_to_monitor_actions`.
fn replay_out_to_monitor_actions(
    replay: &RawCliOutput,
    events: &[Value],
) -> Result<Vec<Value>, String> {
    match replay.output.get("result").and_then(Value::as_str) {
        Some("conformant") => {
            if replay.exit_code != 0 {
                return Err(format!(
                    "conformant replay exited {}: {}",
                    replay.exit_code, replay.output
                ));
            }
            let mut actions = events
                .iter()
                .map(|_| json!({"action":"step_ok"}))
                .collect::<Vec<_>>();
            actions.push(json!({"action":"finish"}));
            Ok(actions)
        }
        Some("nonconformant") => {
            if replay.exit_code != 1 {
                return Err(format!(
                    "nonconformant replay exited {}: {}",
                    replay.exit_code, replay.output
                ));
            }
            if !replay.output["violation"].is_object() || !replay.output["state_before"].is_object()
            {
                return Err(format!(
                    "nonconformant replay is missing violation/state evidence: {}",
                    replay.output
                ));
            }
            let failed_at = replay
                .output
                .get("failed_at_event")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    format!(
                        "nonconformant replay has no failed_at_event: {}",
                        replay.output
                    )
                })?;
            let failed_at = usize::try_from(failed_at).map_err(|_| {
                format!(
                    "failed_at_event does not fit usize: {}",
                    replay.output["failed_at_event"]
                )
            })?;
            if failed_at >= events.len() {
                return Err(format!(
                    "failed_at_event={failed_at} outside {} events: {}",
                    events.len(),
                    replay.output
                ));
            }
            let mut actions = (0..failed_at)
                .map(|_| json!({"action":"step_ok"}))
                .collect::<Vec<_>>();
            actions.push(json!({"action":"step_reject"}));
            Ok(actions)
        }
        _ => Err(format!("unmapped replay observation: {}", replay.output)),
    }
}

#[test]
fn native_monitor_observations_replay_conformantly() {
    let cases = [
        ("all_accepted", cart_events(ReplayFixture::Conformant)),
        ("empty_log", Vec::new()),
        ("first_reject", cart_events(ReplayFixture::Nonconformant)),
    ];
    for (name, events) in cases {
        let observed = replay_cart(&events);
        let trace = replay_out_to_monitor_actions(&observed, &events)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        if name == "first_reject" {
            assert_eq!(observed.output["failed_at_event"], 1, "{}", observed.output);
            assert!(
                observed.output["violation"].is_object()
                    && observed.output["state_before"].is_object(),
                "replay failure lost its violation/state evidence: {}",
                observed.output
            );
            assert_eq!(
                trace,
                vec![json!({"action":"step_ok"}), json!({"action":"step_reject"})]
            );
        }
        assert_conformant(MONITOR_SPEC, &trace, name);
    }
}

#[test]
fn monitor_contract_violations_are_rejected() {
    // Transcribed from tests/test_self_conformance.py:620-636.
    let negative_traces = [
        (
            "step_ok_after_reject",
            vec![
                json!({"action":"step_ok"}),
                json!({"action":"step_reject"}),
                json!({"action":"step_ok"}),
            ],
        ),
        (
            "step_ok_after_finish",
            vec![json!({"action":"finish"}), json!({"action":"step_ok"})],
        ),
    ];
    for (name, trace) in negative_traces {
        assert_nonconformant(MONITOR_SPEC, &trace, name);
    }

    let events = cart_events(ReplayFixture::Nonconformant);
    for (name, output) in [
        (
            "missing_failed_at",
            json!({"result":"nonconformant","violation":{},"state_before":{}}),
        ),
        (
            "noninteger_failed_at",
            json!({"result":"nonconformant","failed_at_event":"1","violation":{},"state_before":{}}),
        ),
        (
            "out_of_range_failed_at",
            json!({"result":"nonconformant","failed_at_event":events.len(),"violation":{},"state_before":{}}),
        ),
        (
            "missing_violation",
            json!({"result":"nonconformant","failed_at_event":1,"state_before":{}}),
        ),
        (
            "missing_state",
            json!({"result":"nonconformant","failed_at_event":1,"violation":{}}),
        ),
    ] {
        let synthetic = RawCliOutput {
            output,
            exit_code: 1,
        };
        assert!(
            replay_out_to_monitor_actions(&synthetic, &events).is_err(),
            "{name} must fail closed"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoldClass {
    Success,
    Failure,
    Skipped,
}

/// Independent fold registry. The 65 registered result literals are
/// transcribed from `rust/fslc/src/outcome.rs:82-216`; sibling-field semantics
/// are documented at `docs/LANGUAGE.md:940-961`. This function deliberately
/// does not call the production classifier.
#[allow(clippy::too_many_lines)]
fn fold_result_class(output: &Value) -> Result<FoldClass, String> {
    let result = output
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("fold item has no string result: {output}"))?;
    match result {
        "approval_check" => match output.get("status").and_then(Value::as_str) {
            Some("approved" | "drifted") => Ok(FoldClass::Success),
            Some("signature-invalid") => Ok(FoldClass::Failure),
            status => Err(format!(
                "unmapped approval_check status {status:?}: {output}"
            )),
        },
        "format_check" => match output.get("changed").and_then(Value::as_bool) {
            Some(false) => Ok(FoldClass::Success),
            Some(true) => Ok(FoldClass::Failure),
            None => Err(format!("format_check missing boolean changed: {output}")),
        },
        "lint" => match output.get("finding_count").and_then(Value::as_u64) {
            Some(0) => Ok(FoldClass::Success),
            Some(_) => Ok(FoldClass::Failure),
            None => Err(format!("lint missing integer finding_count: {output}")),
        },
        "semantic_diff" => {
            let violations = output
                .get("violations")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("semantic_diff missing violations array: {output}"))?;
            Ok(if violations.is_empty() {
                FoldClass::Success
            } else {
                FoldClass::Failure
            })
        }
        "semantic_diff_batch" => match output
            .get("gate")
            .and_then(|gate| gate.get("passed"))
            .and_then(Value::as_bool)
        {
            Some(true) => Ok(FoldClass::Success),
            Some(false) => Ok(FoldClass::Failure),
            None => Err(format!("semantic_diff_batch missing gate.passed: {output}")),
        },
        "skipped" => Ok(FoldClass::Skipped),
        "ok"
        | "verified"
        | "proved"
        | "refines"
        | "conformant"
        | "sweep_passed"
        | "generated"
        | "created"
        | "imported"
        | "imported_with_warnings"
        | "analyzed"
        | "expanded"
        | "explained"
        | "kernel"
        | "typestate"
        | "scenarios"
        | "mutated"
        | "migrated"
        | "compared"
        | "compat_profile_generated"
        | "conformance"
        | "conformance_coverage"
        | "testgen_trace"
        | "conformance_checked"
        | "document_conformant"
        | "observed_conformant"
        | "replay_conformant"
        | "observed_supported"
        | "verified_under_assumptions"
        | "agent_analyzed"
        | "ai_project_analyzed"
        | "causal_analyzed"
        | "causal_model_checked"
        | "causal_diffed"
        | "causal_ledger"
        | "causal_expectations_checked"
        | "causal_expectations_observed"
        | "statistically_supported" => Ok(FoldClass::Success),
        "error"
        | "violated"
        | "reachable_failed"
        | "unknown_cti"
        | "unknown_budget"
        | "refinement_failed"
        | "nonconformant"
        | "impl_violated"
        | "sweep_failed"
        | "observed_mismatch"
        | "replay_nonconformant"
        | "document_drifted"
        | "migration_refused"
        | "approval_diff"
        | "statistically_unsupported"
        | "dataset_invalid"
        | "evaluator_untrusted"
        | "slice_missing"
        | "insufficient_samples"
        | "inconclusive"
        | "unknown" => Ok(FoldClass::Failure),
        _ => Err(format!("unmapped fold result {result:?}: {output}")),
    }
}

fn fold_action(output: &Value) -> Result<Value, String> {
    Ok(match fold_result_class(output)? {
        FoldClass::Success => json!({"action":"fold_sub_success"}),
        FoldClass::Failure => json!({"action":"fold_sub_failure"}),
        FoldClass::Skipped => json!({"action":"fold_skipped"}),
    })
}

/// Exact compound result/exit pairs follow `docs/LANGUAGE.md:940-961` and the
/// command contracts at `main.rs:3533-3589,4000-4051,13309-13338`.
fn finalize_action(command: CompoundCommand, top: &RawCliOutput) -> Result<Value, String> {
    let result = top
        .output
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{command:?} result is not a string: {}", top.output))?;
    let action = match (command, result, top.exit_code) {
        (CompoundCommand::Sweep, "sweep_passed", 0)
        | (CompoundCommand::Chain, "verified", 0)
        | (CompoundCommand::AnalyzeBatch, "analyzed", 0) => "finalize_pass",
        (CompoundCommand::Sweep, "sweep_failed", 1)
        | (CompoundCommand::Chain, "violated", 1)
        | (CompoundCommand::Chain | CompoundCommand::AnalyzeBatch, "error", 2) => "finalize_fail",
        _ => {
            return Err(format!(
                "{command:?} result/exit contradiction: result={result:?} exit={} output={}",
                top.exit_code, top.output
            ));
        }
    };
    Ok(json!({"action":action}))
}

fn fold_trace(
    command: CompoundCommand,
    items: &[Value],
    top: &RawCliOutput,
) -> Result<Vec<Value>, String> {
    let mut trace = items
        .iter()
        .map(fold_action)
        .collect::<Result<Vec<_>, _>>()?;
    trace.push(finalize_action(command, top)?);
    Ok(trace)
}

fn rejected_finalize_pass(trace: &[Value]) -> Vec<Value> {
    let mut invalid = trace.to_vec();
    let last = invalid.last_mut().expect("fold trace has finalize action");
    *last = json!({"action":"finalize_pass"});
    invalid
}

#[test]
fn fold_spec_has_native_proof_vacuity_and_mutation_evidence() {
    let check = run_cli(&strings(&["check", FOLD_SPEC]));
    assert_eq!(
        (check.output["result"].as_str(), check.exit_code),
        (Some("ok"), 0)
    );

    let bounded = run_cli(&strings(&["verify", FOLD_SPEC, "--depth", "8"]));
    assert_eq!(
        (bounded.output["result"].as_str(), bounded.exit_code),
        (Some("verified"), 0)
    );

    let induction = run_cli(&strings(&[
        "verify",
        FOLD_SPEC,
        "--depth",
        "8",
        "--engine",
        "induction",
    ]));
    assert_eq!(
        (induction.output["result"].as_str(), induction.exit_code),
        (Some("proved"), 0)
    );

    let vacuity = run_cli(&strings(&[
        "verify",
        FOLD_SPEC,
        "--depth",
        "8",
        "--vacuity",
        "error",
    ]));
    assert_eq!(
        (vacuity.output["result"].as_str(), vacuity.exit_code),
        (Some("verified"), 0)
    );
    assert!(
        vacuity
            .output
            .get("warnings")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "fold spec has vacuity warnings: {}",
        vacuity.output
    );

    let mutation = run_cli(&strings(&["mutate", FOLD_SPEC, "--depth", "8"]));
    assert_eq!(
        (mutation.output["result"].as_str(), mutation.exit_code),
        (Some("mutated"), 0)
    );
    assert_eq!(
        mutation.output["summary"]["invalid"], 0,
        "{}",
        mutation.output
    );
    assert!(
        mutation.output["summary"]["kill_rate"]
            .as_f64()
            .is_some_and(|rate| rate >= 0.65),
        "fold mutation kill rate is too weak: {}",
        mutation.output
    );
    for operator in ["requires_remove", "requires_negate"] {
        assert!(
            mutation.output["mutants"]
                .as_array()
                .expect("mutation rows")
                .iter()
                .any(|mutant| {
                    mutant["op"] == operator
                        && mutant["target"] == "finalize_pass requires #2"
                        && mutant["status"] == "killed"
                        && mutant["killed_by"] == "FailureIsSticky"
                }),
            "{operator} of the failure-sticky finalize guard survived: {}",
            mutation.output
        );
    }
}

#[test]
fn fold_classifier_is_fail_closed() {
    assert_eq!(
        fold_result_class(&json!({"result":"verified"})).expect("registered success"),
        FoldClass::Success
    );
    assert_eq!(
        fold_result_class(&json!({"result":"violated"})).expect("registered failure"),
        FoldClass::Failure
    );
    assert!(
        fold_result_class(&json!({"result":"verified_typo"}))
            .expect_err("unknown result must fail closed")
            .contains("unmapped fold result")
    );

    for (command, result, wrong_exit) in [
        (CompoundCommand::Sweep, "sweep_failed", 2),
        (CompoundCommand::Chain, "violated", 3),
        (CompoundCommand::AnalyzeBatch, "error", 1),
    ] {
        let contradictory = RawCliOutput {
            output: json!({"result":result}),
            exit_code: wrong_exit,
        };
        assert!(
            finalize_action(command, &contradictory).is_err(),
            "{command:?} {result}/exit-{wrong_exit} must fail closed"
        );
    }
}

#[test]
fn sweep_subverdicts_conform_to_the_fold_model() {
    let passed = run_cli(&strings(&[
        "sweep",
        "rust/fslc/tests/fixtures/sweep_clean.fsl",
        "--depth",
        "0..3",
    ]));
    let passed_items = passed.output["sweep"]["results"]
        .as_array()
        .expect("clean sweep results")
        .iter()
        .map(|entry| entry["summary"].clone())
        .collect::<Vec<_>>();
    let passed_trace =
        fold_trace(CompoundCommand::Sweep, &passed_items, &passed).expect("map clean sweep");
    assert_conformant(FOLD_SPEC, &passed_trace, "clean sweep fold");

    let failed = run_cli(&strings(&[
        "sweep",
        "rust/fslc/tests/fixtures/sweep_violating.fsl",
        "--depth",
        "0..3",
    ]));
    assert_eq!(failed.output["result"], "sweep_failed", "{}", failed.output);
    let failed_items = failed.output["sweep"]["results"]
        .as_array()
        .expect("failed sweep results")
        .iter()
        .map(|entry| entry["summary"].clone())
        .collect::<Vec<_>>();
    assert!(
        failed_items
            .iter()
            .any(|item| fold_result_class(item) == Ok(FoldClass::Failure)),
        "failed sweep must expose a failure item: {}",
        failed.output
    );
    let failed_trace =
        fold_trace(CompoundCommand::Sweep, &failed_items, &failed).expect("map failed sweep");
    assert_conformant(FOLD_SPEC, &failed_trace, "failed sweep fold");
    assert_nonconformant(
        FOLD_SPEC,
        &rejected_finalize_pass(&failed_trace),
        "failed sweep cannot finalize pass",
    );
}

const CHAIN_PYTHON_COMMAND: &str = "command = \"python -c \\\"print('impl ok')\\\"\"";

fn portable_chain_command(arguments: &str) -> String {
    let binary = env!("CARGO_BIN_EXE_fslc").replace('\\', "\\\\");
    format!("command = \"{binary} {arguments}\"")
}

fn copy_chain_fixtures(destination: &Path) {
    let fixture_directory = root().join("tests/fixtures/chain");
    for entry in fs::read_dir(&fixture_directory).expect("read chain fixture directory") {
        let entry = entry.expect("chain fixture entry");
        let source = entry.path();
        if !source.is_file() {
            continue;
        }
        let target = destination.join(source.file_name().expect("chain fixture name"));
        if source
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let manifest = fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()))
                .replace("\r\n", "\n");
            assert!(
                manifest.contains(CHAIN_PYTHON_COMMAND),
                "{} has an unexpected impl command",
                source.display()
            );
            fs::write(
                target,
                manifest.replace(
                    CHAIN_PYTHON_COMMAND,
                    &portable_chain_command("check business.fsl"),
                ),
            )
            .expect("write portable chain manifest");
        } else {
            fs::copy(&source, target)
                .unwrap_or_else(|error| panic!("copy {}: {error}", source.display()));
        }
    }
}

/// Independent adapter for `main.rs:3638-3657::chain_layer_passes` and the
/// layer envelopes produced at `main.rs:3812-3968`. It deliberately does not
/// call that function or the production outcome classifier.
fn chain_layer_fold_class(layer: &Value) -> Result<FoldClass, String> {
    let status = layer
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("chain layer missing status: {layer}"))?;
    let result = layer
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("chain layer missing result: {layer}"))?;
    let exit_code = layer
        .get("exit_code")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("chain layer missing integer exit_code: {layer}"))?;
    let kind = layer
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("chain layer missing string kind: {layer}"))?;

    if status == "skipped" {
        return if result == "skipped"
            && exit_code == 0
            && matches!(kind, "spec" | "refine" | "impl")
        {
            Ok(FoldClass::Skipped)
        } else {
            Err(format!("contradictory skipped chain layer: {layer}"))
        };
    }

    let detail = layer
        .get("detail")
        .ok_or_else(|| format!("non-skipped chain layer missing detail: {layer}"))?;
    if kind == "command" {
        let return_code = detail
            .get("returncode")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("command layer missing returncode: {layer}"))?;
        return match (status, result, exit_code, detail["result"].as_str()) {
            ("passed", "passed", 0, Some("passed")) if return_code == 0 => Ok(FoldClass::Success),
            ("failed", "failed", 1, Some("failed")) if return_code != 0 => Ok(FoldClass::Failure),
            _ => Err(format!("contradictory command chain layer: {layer}")),
        };
    }
    if !matches!(kind, "verify" | "check" | "refine") {
        return Err(format!(
            "unmapped non-command chain layer kind {kind:?}: {layer}"
        ));
    }

    if layer.get("result") != detail.get("result") {
        return Err(format!("chain layer/detail results disagree: {layer}"));
    }
    let detail_class = fold_result_class(detail)?;
    let implements_failed = match detail.get("implements") {
        None => false,
        Some(Value::Object(implements)) => match implements.get("result").and_then(Value::as_str) {
            Some("refines") => false,
            Some("refinement_failed" | "impl_violated") => true,
            _ => {
                return Err(format!(
                    "unmapped implements result in chain layer: {layer}"
                ));
            }
        },
        Some(_) => return Err(format!("chain layer implements is not an object: {layer}")),
    };
    let expected_exit = if implements_failed {
        1
    } else {
        match detail_class {
            FoldClass::Success => 0,
            FoldClass::Failure if result == "error" && detail["kind"] == "internal" => 3,
            FoldClass::Failure if result == "error" => 2,
            FoldClass::Failure => 1,
            FoldClass::Skipped => {
                return Err(format!("non-skipped layer has skipped detail: {layer}"));
            }
        }
    };
    let expected_status =
        if detail_class == FoldClass::Success && !implements_failed && expected_exit == 0 {
            "passed"
        } else {
            "failed"
        };
    if status != expected_status || exit_code != expected_exit {
        return Err(format!(
            "chain layer status/exit contradiction: expected {expected_status}/exit-{expected_exit}: {layer}"
        ));
    }
    Ok(if expected_status == "passed" {
        FoldClass::Success
    } else {
        FoldClass::Failure
    })
}

fn chain_fold_trace(items: &[Value], top: &RawCliOutput) -> Result<Vec<Value>, String> {
    let mut trace = items
        .iter()
        .map(|item| {
            Ok(match chain_layer_fold_class(item)? {
                FoldClass::Success => json!({"action":"fold_sub_success"}),
                FoldClass::Failure => json!({"action":"fold_sub_failure"}),
                FoldClass::Skipped => json!({"action":"fold_skipped"}),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    trace.push(finalize_action(CompoundCommand::Chain, top)?);
    Ok(trace)
}

#[test]
fn chain_layer_adapter_is_fail_closed() {
    let valid = json!({
        "layer":"requirements",
        "kind":"check",
        "status":"passed",
        "result":"ok",
        "exit_code":0,
        "detail":{"result":"ok"}
    });
    assert_eq!(
        chain_layer_fold_class(&valid).expect("valid chain layer"),
        FoldClass::Success
    );

    for implements in [
        json!("not-an-object"),
        json!({}),
        json!({"result":7}),
        json!({"result":"maybe"}),
    ] {
        let mut malformed = valid.clone();
        malformed["detail"]["implements"] = implements;
        assert!(
            chain_layer_fold_class(&malformed).is_err(),
            "malformed/unknown implements must fail closed: {malformed}"
        );
    }

    let mut missing_kind = valid;
    missing_kind
        .as_object_mut()
        .expect("chain layer object")
        .remove("kind");
    assert!(
        chain_layer_fold_class(&missing_kind).is_err(),
        "missing layer kind must fail closed"
    );
}

#[test]
fn chain_layer_verdicts_conform_to_the_fold_model() {
    let directory = scratch_dir("chain");
    copy_chain_fixtures(&directory);

    let passed = run_cli_at(&directory, &strings(&["chain", "fsl-project.toml"]));
    let passed_items = passed.output["layers"]
        .as_array()
        .expect("clean chain layers")
        .clone();
    assert!(
        passed_items.iter().any(|item| item["kind"] == "command"
            && chain_layer_fold_class(item) == Ok(FoldClass::Success)),
        "clean chain must execute its implementation command: {}",
        passed.output
    );
    let passed_trace = chain_fold_trace(&passed_items, &passed).expect("map clean chain");
    assert_conformant(FOLD_SPEC, &passed_trace, "clean chain fold");

    let failed = run_cli_at(
        &directory,
        &strings(&["chain", "fsl-project-broken-implements.toml"]),
    );
    assert_eq!(failed.output["result"], "violated", "{}", failed.output);
    let failed_items = failed.output["layers"]
        .as_array()
        .expect("failed chain layers")
        .clone();
    assert!(
        failed_items
            .iter()
            .any(
                |item| item["detail"]["implements"]["result"] == "refinement_failed"
                    && chain_layer_fold_class(item) == Ok(FoldClass::Failure)
            ),
        "failed chain must expose the nested implements failure: {}",
        failed.output
    );
    assert!(
        failed_items
            .iter()
            .any(|item| chain_layer_fold_class(item) == Ok(FoldClass::Skipped)),
        "stop-on-failure chain must expose a skipped layer: {}",
        failed.output
    );
    let failed_trace = chain_fold_trace(&failed_items, &failed).expect("map failed chain");
    assert_conformant(FOLD_SPEC, &failed_trace, "failed chain fold");
    assert_nonconformant(
        FOLD_SPEC,
        &rejected_finalize_pass(&failed_trace),
        "failed chain cannot finalize pass",
    );

    let manifest_path = directory.join("fsl-project-command-fails.toml");
    let manifest = fs::read_to_string(directory.join("fsl-project.toml"))
        .expect("read portable clean manifest")
        .replace(
            &portable_chain_command("check business.fsl"),
            &portable_chain_command("check missing.fsl"),
        );
    fs::write(&manifest_path, manifest).expect("write failing command manifest");
    let command_failed = run_cli_at(
        &directory,
        &strings(&["chain", "fsl-project-command-fails.toml"]),
    );
    let command_failed_items = command_failed.output["layers"]
        .as_array()
        .expect("command-failed chain layers")
        .clone();
    assert!(
        command_failed_items
            .iter()
            .any(|item| item["kind"] == "command"
                && chain_layer_fold_class(item) == Ok(FoldClass::Failure)),
        "failing implementation command must fold as failure: {}",
        command_failed.output
    );
    let command_failed_trace =
        chain_fold_trace(&command_failed_items, &command_failed).expect("map command-failed chain");
    assert_conformant(
        FOLD_SPEC,
        &command_failed_trace,
        "command-failed chain fold",
    );
}

#[test]
fn analyze_batch_items_conform_to_the_fold_model() {
    let empty_directory = scratch_dir("analyze-empty");
    let empty = run_cli(&[
        "analyze".to_owned(),
        empty_directory.to_string_lossy().into_owned(),
    ]);
    assert_eq!(empty.output["result"], "analyzed", "{}", empty.output);
    assert_eq!(empty.exit_code, 0, "{}", empty.output);
    let empty_items = empty.output["files"]
        .as_array()
        .expect("empty analyze files");
    assert!(empty_items.is_empty(), "{}", empty.output);
    let empty_trace = fold_trace(CompoundCommand::AnalyzeBatch, empty_items, &empty)
        .expect("map empty analyze batch");
    assert_eq!(empty_trace, vec![json!({"action":"finalize_pass"})]);
    assert_conformant(FOLD_SPEC, &empty_trace, "empty analyze batch fold");

    let directory = scratch_dir("analyze-batch");
    fs::write(
        directory.join("valid-a.fsl"),
        "spec ValidA { state { x: Bool } init { x = false } action set() { x = true } }\n",
    )
    .expect("write valid analysis fixture");
    fs::write(
        directory.join("valid-b.fsl"),
        "spec ValidB { state { x: Bool } init { x = true } action clear() { x = false } }\n",
    )
    .expect("write second valid analysis fixture");

    let passed = run_cli(&[
        "analyze".to_owned(),
        directory.join("valid-a.fsl").to_string_lossy().into_owned(),
        directory.join("valid-b.fsl").to_string_lossy().into_owned(),
    ]);
    assert_eq!(passed.output["result"], "analyzed", "{}", passed.output);
    let passed_items = passed.output["files"]
        .as_array()
        .expect("clean analyze files")
        .clone();
    let passed_trace = fold_trace(CompoundCommand::AnalyzeBatch, &passed_items, &passed)
        .expect("map clean analyze batch");
    assert_conformant(FOLD_SPEC, &passed_trace, "clean analyze batch fold");

    fs::write(
        directory.join("invalid.fsl"),
        "spec Invalid { state { x: } }\n",
    )
    .expect("write invalid analysis fixture");
    let failed = run_cli(&[
        "analyze".to_owned(),
        directory.join("valid-a.fsl").to_string_lossy().into_owned(),
        directory.join("invalid.fsl").to_string_lossy().into_owned(),
    ]);
    assert_eq!(failed.output["result"], "error", "{}", failed.output);
    let failed_items = failed.output["files"]
        .as_array()
        .expect("failed analyze files")
        .clone();
    assert!(
        failed_items
            .iter()
            .any(|item| fold_result_class(item) == Ok(FoldClass::Failure)),
        "failed analyze batch must expose an error item: {}",
        failed.output
    );
    assert!(
        failed_items
            .iter()
            .any(|item| fold_result_class(item) == Ok(FoldClass::Success)),
        "failed analyze batch must preserve its successful item: {}",
        failed.output
    );
    let failed_trace = fold_trace(CompoundCommand::AnalyzeBatch, &failed_items, &failed)
        .expect("map failed analyze batch");
    assert_conformant(FOLD_SPEC, &failed_trace, "failed analyze batch fold");
    assert_nonconformant(
        FOLD_SPEC,
        &rejected_finalize_pass(&failed_trace),
        "failed analyze batch cannot finalize pass",
    );
}
