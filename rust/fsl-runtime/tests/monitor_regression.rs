// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};

fn model(source: impl AsRef<str>) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source.as_ref(), &FsResolver::new(".")).expect("parse model"))
        .expect("build model")
}

#[test]
fn monitor_enforces_guards_and_updates_state() {
    let model = model(
        "spec Counter { type Count = 0..2 state { count: Count } init { count = 0 } ".to_owned()
            + "action add() { requires count < 2 count = count + 1 } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    for expected in [1, 2] {
        let action = monitor.enabled().expect("enabled actions")[0].clone();
        monitor.step(&action).expect("step monitor");
        assert_eq!(monitor.state["count"], FslValue::Int(expected));
    }
    assert!(monitor.enabled().expect("enabled actions").is_empty());
}

#[test]
fn concrete_arithmetic_uses_smt_euclidean_division() {
    let model = model(
        "spec Arithmetic { state { x: Int } init { x = -1 } action stay() { x = x } ".to_owned()
            + "invariant Division { x / 2 == -1 and x % 2 == 1 } }",
    );
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
    let value = fsl_runtime::eval(
        &model.invariants[0].expr,
        &monitor.state,
        &mut BTreeMap::new(),
        &model,
        None,
    )
    .expect("evaluate expression");
    assert_eq!(value, FslValue::Bool(true));
}

#[test]
fn replay_rejects_a_trace_that_is_not_enabled() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } ".to_owned()
            + "action finish() { requires not done done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
    let action = monitor.enabled().expect("enabled actions")[0].clone();
    monitor.step(&action).expect("step monitor");
    let trace_action = fsl_core::TraceAction {
        name: action.action.clone(),
        params: action.params.clone(),
    };
    let trace = vec![
        fsl_core::TraceStep {
            step: 0,
            state: BTreeMap::from([("done".to_owned(), FslValue::Bool(false))]),
            action: None,
            changes: BTreeMap::new(),
        },
        fsl_core::TraceStep {
            step: 1,
            state: monitor.state.clone(),
            action: Some(trace_action),
            changes: BTreeMap::from([(
                "done".to_owned(),
                fsl_core::TraceChange {
                    from: FslValue::Bool(false),
                    to: FslValue::Bool(true),
                },
            )]),
        },
    ];
    fsl_runtime::replay_trace(model.clone(), &trace).expect("replay valid trace");
    let mut invalid = trace;
    invalid.push(invalid[1].clone());
    assert!(fsl_runtime::replay_trace(model, &invalid).is_err());
}

#[test]
fn failed_steps_leave_the_monitor_state_unchanged() {
    for (suffix, expected_kind) in [
        ("invariant Safe { x == 0 }", "invariant"),
        ("trans Stable { x == old(x) }", "trans"),
        ("", "ensures"),
    ] {
        let ensures = if expected_kind == "ensures" {
            "ensures x == 0"
        } else {
            ""
        };
        let model = model(format!(
            "spec Rollback {{ state {{ x: Int }} init {{ x = 0 }} action break_it() {{ x = 1 {ensures} }} {suffix} }}"
        ));
        let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
        let before = monitor.state.clone();
        let action = monitor.enabled().expect("enabled actions")[0].clone();
        let result = monitor.step(&action).expect("step monitor");
        assert_eq!(
            result
                .violation
                .as_ref()
                .map(|violation| violation.kind.as_str()),
            Some(expected_kind)
        );
        assert_eq!(result.state, before);
        assert_eq!(monitor.state, before);
    }
}

#[test]
fn stale_enabled_action_is_rejected_after_state_change() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } ".to_owned()
            + "action finish() { requires not done done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let action = monitor.enabled().expect("enabled actions")[0].clone();

    monitor.step(&action).expect("first step");
    let state_after_first_step = monitor.state.clone();
    let error = monitor.step(&action).expect_err("stale action must fail");

    assert!(error.message.contains("stale"), "{error:?}");
    assert_eq!(monitor.state, state_after_first_step);
    let failed = monitor
        .attempt("finish", &BTreeMap::new())
        .expect("disabled attempt is observable");
    assert_eq!(failed.violation.expect("requires violation").step, 2);
}

#[test]
fn action_parameters_must_belong_to_their_declared_domains() {
    let model = model(
        "spec Parameters { type Small = 0..1 enum Choice { A, B } state { done: Bool } ".to_owned()
            + "init { done = false } action inline(v in 0..1) { } "
            + "action domain(v: Small) { } action choice(v: Choice) { } "
            + "action flag(v: Bool) { } }",
    );
    let valid = [
        ("inline", FslValue::Int(0)),
        ("inline", FslValue::Int(1)),
        ("domain", FslValue::Int(0)),
        ("domain", FslValue::Int(1)),
        (
            "choice",
            FslValue::Enum {
                type_name: "Choice".to_owned(),
                member: "A".to_owned(),
            },
        ),
        ("flag", FslValue::Bool(false)),
        ("flag", FslValue::Bool(true)),
    ];
    for (action, value) in valid {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
        let result = monitor
            .attempt(action, &BTreeMap::from([("v".to_owned(), value)]))
            .unwrap_or_else(|error| panic!("valid {action} parameter failed: {error:?}"));
        assert!(result.violation.is_none(), "{action}: {result:?}");
    }

    let invalid = [
        ("inline", FslValue::Int(-1)),
        ("inline", FslValue::Int(2)),
        ("domain", FslValue::Int(-1)),
        ("domain", FslValue::Int(2)),
        ("choice", FslValue::Bool(false)),
        (
            "choice",
            FslValue::Enum {
                type_name: "Other".to_owned(),
                member: "A".to_owned(),
            },
        ),
        (
            "choice",
            FslValue::Enum {
                type_name: "Choice".to_owned(),
                member: "Missing".to_owned(),
            },
        ),
        ("flag", FslValue::Int(0)),
    ];
    for (action, value) in invalid {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
        let state_before_attempt = monitor.state.clone();
        let error = monitor
            .attempt(action, &BTreeMap::from([("v".to_owned(), value)]))
            .expect_err("out-of-domain parameter must fail");
        assert!(error.message.contains("parameter"), "{error:?}");
        assert_eq!(monitor.state, state_before_attempt);
    }
}

#[test]
fn wide_named_domain_parameter_validation_does_not_enumerate_for_direct_attempts() {
    let model = model(
        "spec Wide { type Value = 0..1000000000 state { done: Bool } init { done = false } action accept(v: Value) { done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");

    monitor
        .attempt(
            "accept",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(1_000_000_000))]),
        )
        .expect("upper bound validates without domain enumeration");
}

#[test]
fn invalid_parameters_do_not_consume_a_logical_step() {
    let model = model(
        "spec Steps { type Small = 0..1 state { done: Bool } init { done = false } action blocked(v: Small) { requires false } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    monitor
        .attempt(
            "blocked",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(2))]),
        )
        .expect_err("invalid parameter must fail before attempting a step");
    let failed = monitor
        .attempt(
            "blocked",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(1))]),
        )
        .expect("valid disabled attempt is observable");
    assert_eq!(failed.violation.expect("requires violation").step, 1);
}
