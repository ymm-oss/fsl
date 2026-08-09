// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for the #465 residual: the three solver-dependent
//! `docs/DESIGN-vacuity.md` §2 lanes — `always_true_requires`,
//! `tautology_over_frozen`, `urgency_freeze` — were entirely unimplemented in
//! native `fslc`, so a hollow spec whose only emptiness was one of them came
//! back `result:"verified"`/exit 0 even under `--vacuity error`. Each lane is
//! pinned three ways: it fires (exit 2, `kind`, `trace_type:"vacuity"`), it
//! routes through `--vacuity warn|ignore`, and it stays silent on a legitimate
//! spec of the same shape.
//!
//! The lanes are proved over the declared type space rather than over states
//! witnessed within `--depth`, so `depth_does_not_change_the_verdict` and
//! `a_capacity_guard_is_never_reported_as_dead` pin the accepted resolution of
//! issue #465: a warning that would disappear at a larger `--depth` is never
//! emitted.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn verify(fixture: &str, depth: &str, vacuity: &str) -> (Value, i32) {
    run_cli(&[
        "verify",
        fixture,
        "--depth",
        depth,
        "--deadlock",
        "ignore",
        "--vacuity",
        vacuity,
        "--no-cache",
    ])
}

fn warning_kinds(output: &Value) -> Vec<String> {
    output["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter_map(|warning| warning["kind"].as_str())
        .map(str::to_owned)
        .collect()
}

const FROZEN_GHOST: &str = "rust/fslc/tests/fixtures/vacuity_frozen_ghost.fsl";
const FROZEN_PLUS_DYNAMIC: &str = "rust/fslc/tests/fixtures/vacuity_frozen_plus_dynamic.fsl";
const URGENCY_FREEZE: &str = "rust/fslc/tests/fixtures/vacuity_urgency_freeze.fsl";
const STATE_CHANGING_DEADLINE: &str =
    "rust/fslc/tests/fixtures/vacuity_state_changing_deadline.fsl";
const DEADLINE_PATTERN: &str = "rust/fslc/tests/fixtures/vacuity_deadline_urgency_pattern.fsl";
const REDUNDANT_REQUIRES: &str = "rust/fslc/tests/fixtures/vacuity_redundant_requires.fsl";
const CAPACITY_GUARD: &str = "rust/fslc/tests/fixtures/vacuity_capacity_guard.fsl";
const COVERAGE_FALSE_REQUIRES: &str =
    "rust/fslc/tests/fixtures/vacuity_coverage_false_requires.fsl";

/// Issue #465's headline reproduction: this spec checks nothing and used to
/// pass `--vacuity error` with exit 0.
#[test]
fn tautology_over_frozen_fails_closed_under_vacuity_error() {
    let (output, status) = verify(FROZEN_GHOST, "3", "error");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "tautology_over_frozen");
    assert_eq!(output["trace_type"], "vacuity");
    let finding = &output["findings"][0];
    assert_eq!(finding["name"], "FrozenGhost");
    assert_eq!(finding["faithfulness_class"], "frozen_only_invariant");
    assert_eq!(
        finding["recommended_action"],
        "run mutate to check kill-rate"
    );
    assert!(
        finding["message"]
            .as_str()
            .is_some_and(|message| message.contains("ghost")),
        "the message must name the frozen state: {output:#}"
    );
}

#[test]
fn tautology_over_frozen_routes_through_warn_and_ignore() {
    let (warned, status) = verify(FROZEN_GHOST, "3", "warn");
    assert_eq!(status, 0, "{warned:#}");
    assert_eq!(warned["result"], "verified");
    assert!(
        warning_kinds(&warned).contains(&"tautology_over_frozen".to_owned()),
        "{warned:#}"
    );

    let (ignored, status) = verify(FROZEN_GHOST, "3", "ignore");
    assert_eq!(status, 0, "{ignored:#}");
    assert!(
        !warning_kinds(&ignored).contains(&"tautology_over_frozen".to_owned()),
        "{ignored:#}"
    );
}

/// Non-firing control: an invariant that reads frozen state but whose truth
/// still depends on a dynamic variable is a real invariant.
#[test]
fn an_invariant_that_depends_on_dynamic_state_is_not_a_frozen_tautology() {
    let (output, status) = verify(FROZEN_PLUS_DYNAMIC, "3", "error");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
}

#[test]
fn urgency_freeze_fails_closed_under_vacuity_error() {
    let (output, status) = verify(URGENCY_FREEZE, "3", "error");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "urgency_freeze");
    assert_eq!(output["trace_type"], "vacuity");
    let finding = &output["findings"][0];
    assert_eq!(finding["name"], "tick");
    assert_eq!(finding["requirement"]["id"], "NFR-1");
    let message = finding["message"].as_str().expect("urgency_freeze message");
    assert!(
        message.contains("'spin'"),
        "the message must name the urgent action: {output:#}"
    );
    assert!(
        message.contains("generated action 'tick' is never enabled"),
        "{output:#}"
    );
}

#[test]
fn urgency_freeze_routes_through_warn_and_ignore() {
    let (warned, status) = verify(URGENCY_FREEZE, "3", "warn");
    assert_eq!(status, 0, "{warned:#}");
    assert!(
        warning_kinds(&warned).contains(&"urgency_freeze".to_owned()),
        "{warned:#}"
    );

    let (ignored, status) = verify(URGENCY_FREEZE, "3", "ignore");
    assert_eq!(status, 0, "{ignored:#}");
    assert!(
        !warning_kinds(&ignored).contains(&"urgency_freeze".to_owned()),
        "{ignored:#}"
    );
}

#[test]
fn state_changing_urgency_cannot_hide_a_deadline_that_never_advances() {
    let (output, status) = verify(STATE_CHANGING_DEADLINE, "4", "error");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "vacuous_deadline");
    let finding = &output["findings"][0];
    assert_eq!(finding["requirement"]["id"], "NFR-STATEFUL");
    assert!(
        finding["message"]
            .as_str()
            .is_some_and(|message| message.contains("every transition preserves zero")),
        "{output:#}"
    );
}

/// Non-firing control: the documented deadline-urgency pattern must stay
/// clean, otherwise the lane punishes the shape `docs/LANGUAGE.md` recommends.
#[test]
fn the_deadline_urgency_pattern_is_not_reported_as_a_freeze() {
    let (output, status) = verify(DEADLINE_PATTERN, "4", "error");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["action_coverage"]["tick"], true);
}

#[test]
fn worked_sla_consumes_slack_and_its_lowered_boundary_bites() {
    let (output, status) = verify("examples/nfr/sla_worker.fsl", "10", "error");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["action_coverage"]["tick"], true);
    assert!(!warning_kinds(&output).contains(&"vacuous_deadline".to_owned()));

    let source = std::fs::read_to_string(repository_root().join("examples/nfr/sla_worker.fsl"))
        .expect("read worked SLA")
        .replace("deadline age <= 4", "deadline age <= 3");
    let path = std::env::temp_dir().join(format!(
        "fslc-sla-boundary-{}-{:?}.fsl",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write lowered deadline fixture");
    let (lowered, status) = run_cli(&[
        "verify",
        path.to_str().expect("UTF-8 fixture path"),
        "--depth",
        "10",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
        "--no-cache",
    ]);
    std::fs::remove_file(path).expect("remove lowered deadline fixture");
    assert_eq!(status, 1, "{lowered:#}");
    assert_eq!(lowered["result"], "violated");
    assert_eq!(lowered["invariant"], "_deadline_NFR_1_age_1");
    assert_eq!(lowered["requirement"]["id"], "NFR-1");
}

#[test]
fn always_true_requires_fails_closed_under_vacuity_error() {
    let (output, status) = verify(REDUNDANT_REQUIRES, "2", "error");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "always_true_requires");
    assert_eq!(output["trace_type"], "vacuity");
    let findings = output["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["name"], "pay");
    assert_eq!(findings[0]["requirement"]["id"], "REQ-A");
    assert!(
        findings[0]["message"]
            .as_str()
            .is_some_and(|message| !message.contains("depth")),
        "the judgment is depth-independent, so the message must not cite a depth: {output:#}"
    );
}

#[test]
fn always_true_requires_routes_through_warn_and_ignore() {
    let (warned, status) = verify(REDUNDANT_REQUIRES, "2", "warn");
    assert_eq!(status, 0, "{warned:#}");
    assert!(
        warning_kinds(&warned).contains(&"always_true_requires".to_owned()),
        "{warned:#}"
    );

    let (ignored, status) = verify(REDUNDANT_REQUIRES, "2", "ignore");
    assert_eq!(status, 0, "{ignored:#}");
    assert!(
        !warning_kinds(&ignored).contains(&"always_true_requires".to_owned()),
        "{ignored:#}"
    );
}

/// The accepted resolution of issue #465, in miniature: `slots < 3` is only
/// falsifiable three steps in, so a depth-derived judgment would report it as
/// dead at `--depth 2`. The declared bound `0..3` says it is a real guard, and
/// that answer must not move with the bound.
#[test]
fn a_capacity_guard_is_never_reported_as_dead() {
    for depth in ["1", "2", "8"] {
        let (output, status) = verify(CAPACITY_GUARD, depth, "error");
        assert_eq!(status, 0, "depth {depth}: {output:#}");
        assert_eq!(output["result"], "verified", "depth {depth}");
    }
}

/// `examples/causal/funnel.fsl` is the case the issue names: `requires
/// visits < 100` with `visits: 0..100` reaches only `visits == 8` at depth 8,
/// which the frozen Python reference reports as `always_true_requires`. A
/// warning that disappears when `--depth` rises is not evidence of a hollow
/// spec, so native must stay silent at every depth.
#[test]
fn depth_does_not_change_the_verdict() {
    for depth in ["3", "8", "16"] {
        let (output, status) = run_cli(&[
            "verify",
            "examples/causal/funnel.fsl",
            "--depth",
            depth,
            "--deadlock",
            "ignore",
            "--vacuity",
            "error",
            "--no-cache",
        ]);
        assert_eq!(status, 0, "depth {depth}: {output:#}");
        assert!(
            !warning_kinds(&output).contains(&"always_true_requires".to_owned()),
            "depth {depth}: {output:#}"
        );
    }
}

/// Non-firing control: the second clause of `impossible` is trivially implied
/// by the first, but the action is never enabled and already carries its own
/// never-enabled warning.
#[test]
fn a_coverage_false_action_does_not_produce_always_true_requires() {
    let (output, status) = verify(COVERAGE_FALSE_REQUIRES, "2", "error");
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "never_enabled_action", "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "never_enabled_action"),
        "coverage-false action must retain its own finding: {output:#}"
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding["kind"] != "always_true_requires"),
        "coverage-false action must not add an always-true-requires finding: {output:#}"
    );

    let (warned, warned_status) = verify(COVERAGE_FALSE_REQUIRES, "2", "warn");
    assert_eq!(warned_status, 0, "{warned:#}");
    assert_eq!(warned["result"], "verified", "{warned:#}");
    assert_eq!(
        warning_kinds(&warned),
        vec!["never_enabled_action"],
        "coverage-false action must emit only its bounded coverage finding: {warned:#}"
    );
}

/// Non-firing control: a compose-synchronized action inherits `a > 0` from
/// both `bank.submit_deposit` and `audit.deposit`. That duplication is the
/// intended "each component defends its own contract" design, not removable
/// redundancy (`docs/DESIGN-vacuity.md` §2), and every clause is checked in
/// the right context when the component spec is verified on its own.
#[test]
fn a_synchronized_compose_action_is_not_flagged_for_duplicate_guards() {
    let (output, status) = run_cli(&[
        "verify",
        "specs/bank_system.fsl",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["action_coverage"]["deposit_audited"], true);
}

/// The lanes describe the model, not the exploration, so `--engine explicit`
/// (solver-free BFS) and `--engine induction` must report the same kind as
/// bounded model checking. An engine-dependent vacuity verdict would be
/// exactly the exit-code divergence `docs/DESIGN-rust-port.md` forbids.
#[test]
fn every_engine_reports_the_same_vacuity_kind() {
    for (fixture, depth, kind) in [
        (FROZEN_GHOST, "3", "tautology_over_frozen"),
        (URGENCY_FREEZE, "3", "urgency_freeze"),
        (STATE_CHANGING_DEADLINE, "4", "vacuous_deadline"),
        (REDUNDANT_REQUIRES, "2", "always_true_requires"),
    ] {
        for engine in ["bmc", "explicit", "induction"] {
            let (output, status) = run_cli(&[
                "verify",
                fixture,
                "--depth",
                depth,
                "--deadlock",
                "ignore",
                "--vacuity",
                "error",
                "--engine",
                engine,
                "--no-cache",
            ]);
            assert_eq!(status, 2, "{engine}: {output:#}");
            assert_eq!(output["kind"], kind, "{engine}: {output:#}");
            assert_eq!(output["trace_type"], "vacuity", "{engine}: {output:#}");
        }
    }
}
