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
