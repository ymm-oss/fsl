// SPDX-License-Identifier: Apache-2.0

//! Actual symbolic/concrete truth, independently of collision admission.
#[path = "support/pattern_lifetime.rs"]
mod fixtures;
#[path = "support/observation.rs"]
mod observations;

use fixtures::{CELLS, block_on, build, model};
use fsl_core::{FslValue as Value, KernelModel};

fn symbolic(model: &KernelModel, induction: bool, expected: bool, context: &str) {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("solver");
    let result = block_on(fsl_verifier::verify_bounded(model, &mut solver, 4)).expect("bmc");
    observations::capture(context, "bmc-full", &result);
    assert!(result.leadsto_violation.is_none());
    assert_eq!(
        result.violation.is_none(),
        expected,
        "{context}: {result:?}"
    );
    if let Some(v) = result.violation {
        assert_eq!((&*v.kind, &*v.name, v.step), ("invariant", "Q", 0));
    } else if induction {
        // Like the CLI, prove only after checking the bounded base.
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("solver");
        let result =
            block_on(fsl_verifier::prove_induction(model, &mut solver, 4)).expect("induction");
        observations::capture(context, "induction-full", &result);
        assert!(
            result.cti.is_none(),
            "{context}: unknown is not proved: {result:?}"
        );
    }
}

fn matrix(consumer: &str) {
    for cell in CELLS {
        for fresh in [true, false] {
            let model = model(cell, fresh);
            for repeat in 1..=2 {
                let context = format!(
                    "{} fresh={fresh} consumer={consumer} repeat={repeat}",
                    cell.name
                );
                observations::capture(&context, "model-full", &model);
                match consumer {
                    "bmc" | "induction" => {
                        symbolic(&model, consumer == "induction", cell.expected, &context);
                    }
                    "explicit" => {
                        let result = fsl_runtime::verify_explicit(model.clone(), 4, 10_000)
                            .expect("explicit");
                        observations::capture(&context, "explicit-full", &result);
                        assert_eq!(
                            result.violation.is_none(),
                            cell.expected,
                            "{context}: {result:?}"
                        );
                        if let Some(v) = result.violation {
                            assert_eq!(
                                (&*v.violation.kind, &*v.violation.name, v.violation.step),
                                ("invariant", "Q", 0)
                            );
                        } else {
                            assert!(result.closure, "{context}: no closure");
                        }
                    }
                    "Monitor" => {
                        let mut monitor =
                            fsl_runtime::Monitor::new(model.clone()).expect("Monitor");
                        let before = monitor.state.clone();
                        let property = model.invariants.iter().find(|p| p.name == "Q").unwrap();
                        let mut bindings = fsl_runtime::Bindings::new();
                        let value = fsl_runtime::eval(
                            &property.expr,
                            &monitor.state,
                            &mut bindings,
                            &model,
                            None,
                        )
                        .expect("eval");
                        assert_eq!(value, Value::Bool(cell.expected), "{context}");
                        assert!(bindings.is_empty(), "{context}: {bindings:?}");
                        assert_eq!(monitor.state, before);
                        let initial = monitor.current_violation().expect("initial violation");
                        assert_eq!(initial.is_none(), cell.expected, "{context}: {initial:?}");
                        let action = monitor.enabled().expect("enabled").remove(0);
                        let step = monitor.step(&action).expect("tick");
                        observations::capture(
                            &context,
                            "monitor-full",
                            &(
                                &property.expr,
                                &before,
                                &bindings,
                                &value,
                                &initial,
                                &action,
                                &step,
                                &monitor.state,
                            ),
                        );
                        assert_eq!(
                            step.violation.is_none(),
                            cell.expected,
                            "{context}: {step:?}"
                        );
                        assert_eq!(monitor.state, before);
                    }
                    _ => panic!("unknown consumer"),
                }
                eprintln!(
                    "OBS {context} truth={} (asserted, not engine-agreement-only)",
                    cell.expected
                );
            }
        }
    }
}

#[test]
fn bmc_lifetime() {
    matrix("bmc");
}
#[test]
fn induction_lifetime() {
    matrix("induction");
}
#[test]
fn explicit_lifetime() {
    matrix("explicit");
}
#[test]
fn monitor_lifetime() {
    matrix("Monitor");
}

#[test]
fn requires_keeps_its_legal_payload_continuation() {
    for wrong in [false, true] {
        let write = if wrong { 2 } else { 1 };
        let model = build(&format!(
            "spec Continuation {{type V=0..3 state {{o:Option<V> = some(1),x:V=0}}
             action go() {{let matched = some(3) is some(a)
             requires o is some(v) requires v == 1 x=v+{}}}
             invariant Q {{x != 2}} }}",
            write - 1
        ));
        for repeat in 1..=2 {
            let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("Monitor");
            let action = monitor.enabled().expect("enabled").remove(0);
            let step = monitor.step(&action).expect("go");
            assert_eq!(step.violation.is_some(), wrong);
            let state = if wrong {
                step.attempted_state.expect("attempted state")
            } else {
                step.state
            };
            assert_eq!(state["x"], Value::Int(write));
            let result = fsl_runtime::verify_explicit(model.clone(), 4, 10_000).expect("explicit");
            assert_eq!(result.violation.is_some(), wrong);
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("solver");
            let result =
                block_on(fsl_verifier::verify_bounded(&model, &mut solver, 4)).expect("bmc");
            assert_eq!(result.violation.is_some(), wrong);
            if wrong {
                let violation = result.violation.expect("negative control");
                assert_eq!(
                    (&*violation.kind, &*violation.name, violation.step),
                    ("invariant", "Q", 1)
                );
            } else {
                symbolic(&model, true, true, "requires");
            }
            eprintln!("OBS requires wrong={wrong} repeat={repeat} binding=1 x={write}");
        }
    }
}
