// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn monitor_boundary_model() -> fsl_core::KernelModel {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/self/monitor_action_boundary.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read monitor boundary self-spec");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    build_model(kernel).expect("build model")
}

fn assert_success_is_sticky(
    model: &fsl_core::KernelModel,
    succeeded: &BTreeMap<String, FslValue>,
    violated: &BTreeMap<String, FslValue>,
) {
    let sticky = model
        .transitions
        .iter()
        .find(|property| property.name == "Work_SuccessSticky")
        .expect("success-sticky transition property");
    for (state, expected) in [(succeeded, true), (violated, false)] {
        let mut bindings = BTreeMap::new();
        assert_eq!(
            fsl_runtime::eval(&sticky.expr, state, &mut bindings, model, Some(succeeded),)
                .expect("evaluate success-stickiness"),
            FslValue::Bool(expected)
        );
    }
}

#[test]
fn collection_and_option_transitions_agree_across_bounded_reachable_states() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fslc/tests/fixtures/kernel_contract.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read comprehensive fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let mut queue = VecDeque::from([(initial, 0_usize)]);
    let mut seen = BTreeSet::new();
    let mut checked = 0_usize;

    while let Some((monitor, depth)) = queue.pop_front() {
        if !seen.insert(monitor.state.clone()) {
            continue;
        }
        for enabled in monitor.enabled().expect("enumerate actions") {
            let current = monitor.state.clone();
            let mut successor = monitor.clone();
            let result = successor.step(&enabled).expect("step monitor");
            if result.violation.is_some() {
                continue;
            }
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            assert!(
                block_on(fsl_verifier::transition_matches_step(
                    &model,
                    &mut solver,
                    &current,
                    &enabled.action,
                    &enabled.params,
                    &result.state,
                ))
                .expect("check transition agreement"),
                "{} disagreed from {current:?}",
                enabled.action
            );
            checked += 1;
            if depth < 3 {
                queue.push_back((successor, depth + 1));
            }
        }
    }
    assert!(checked >= 10, "fixture did not exercise enough transitions");
}

#[test]
fn symbolic_transition_accepts_monitor_successors_and_rejects_other_states() {
    let source = r"
spec Agreement {
  type Count = 0..2
  state { x: Count, flag: Bool }
  init { x = 0  flag = false }
  action advance(by in 1..1) {
    requires x < 2
    x = x + by
    flag = not flag
  }
  invariant Bound { x <= 2 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let enabled = monitor.enabled().expect("enumerate actions")[0].clone();
    let result = monitor.step(&enabled).expect("step monitor");
    assert!(result.violation.is_none());

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &current,
            &enabled.action,
            &enabled.params,
            &result.state,
        ))
        .expect("check transition agreement")
    );

    let mut wrong = result.state;
    wrong.insert("flag".to_owned(), FslValue::Bool(false));
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &current,
            &enabled.action,
            &enabled.params,
            &wrong,
        ))
        .expect("reject different successor")
    );
}

#[test]
fn explicit_effect_success_disables_retry_across_monitor_and_symbolic_semantics() {
    let source = r"
domain ExplicitEffectSuccess {
  type RequestId = 0..0
  aggregate Request {
    command Start { id: RequestId }
    event Requested { id: RequestId }
    event FailureRecovered { id: RequestId }
    decide Start { emits Requested }
    evolve Requested {}
    evolve FailureRecovered {}
  }
  effect Work {
    async
    correlation_id Requested.id
    handles Requested
    success_event FailureRecovered
    retry { max_attempts 2 }
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower domain");
    let model = build_model(kernel).expect("build model");

    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let start = monitor
        .enabled()
        .expect("enumerate start")
        .into_iter()
        .find(|action| action.action == "request_start")
        .expect("request start action");
    monitor.step(&start).expect("start effect");

    let completion = monitor
        .enabled()
        .expect("enumerate completion")
        .into_iter()
        .find(|action| action.action == "work_complete_failure_recovered")
        .expect("explicit success completion");
    let before = monitor.state.clone();
    let result = monitor.step(&completion).expect("complete effect");
    assert!(result.violation.is_none());
    assert!(
        monitor
            .enabled()
            .expect("enumerate after success")
            .iter()
            .all(|action| action.action != "work_retry")
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &before,
            &completion.action,
            &completion.params,
            &result.state,
        ))
        .expect("accept Monitor successor")
    );

    let mut wrong = result.state.clone();
    let FslValue::Map(statuses) = wrong.get_mut("work_status").expect("effect status state") else {
        panic!("effect status must be a map");
    };
    statuses.insert(
        FslValue::Int(0),
        FslValue::Enum {
            type_name: "WorkEffectStatus".to_owned(),
            member: "WorkEffectStatus_Failed".to_owned(),
        },
    );
    assert_success_is_sticky(&model, &result.state, &wrong);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create rejection solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &before,
            &completion.action,
            &completion.params,
            &wrong,
        ))
        .expect("reject name-heuristic failure successor")
    );
}

#[test]
fn disabled_and_failed_monitor_outcomes_agree_and_rollback() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fslc/tests/fixtures/conformance_failures.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read failure fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");

    // Most actions in this fixture are deliberate failures (its purpose is
    // to fix v1 failure semantics), but a few (e.g. `euclid_divide`,
    // `flip`) are genuine successful transitions added by issue #223 as
    // coverage-matrix evidence. Check each outcome with the agreement path
    // that matches what actually happened, exactly like
    // `collection_and_option_transitions_agree_across_bounded_reachable_states`
    // does for `kernel_contract.fsl`.
    for action in &model.actions {
        let current = initial.state.clone();
        let mut monitor = initial.clone();
        let params = std::collections::BTreeMap::new();
        let result = monitor
            .attempt(&action.name, &params)
            .expect("evaluate action outcome");

        if let Some(violation) = result.violation.as_ref() {
            assert_eq!(
                result.state, current,
                "{} committed a failed step",
                action.name
            );
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            assert!(
                block_on(fsl_verifier::transition_outcome_matches_step(
                    &model,
                    &mut solver,
                    &current,
                    &action.name,
                    &params,
                    &result.state,
                    result.attempted_state.as_ref(),
                    &violation.kind,
                ))
                .expect("check failure agreement"),
                "{} {} disagreed",
                action.name,
                violation.kind
            );

            let mut rejection_solver =
                fsl_solver_z3::Z3Solver::new().expect("create rejection solver");
            let corrupted_attempted = if result.attempted_state.is_some() {
                None
            } else {
                Some(&current)
            };
            assert!(
                !block_on(fsl_verifier::transition_outcome_matches_step(
                    &model,
                    &mut rejection_solver,
                    &current,
                    &action.name,
                    &params,
                    &result.state,
                    corrupted_attempted,
                    &violation.kind,
                ))
                .expect("reject corrupted failure evidence"),
                "{} {} accepted the wrong attempted-state presence",
                action.name,
                violation.kind
            );
        } else {
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            assert!(
                block_on(fsl_verifier::transition_matches_step(
                    &model,
                    &mut solver,
                    &current,
                    &action.name,
                    &params,
                    &result.state,
                ))
                .expect("check success agreement"),
                "{} disagreed from {current:?}",
                action.name
            );
        }
    }
}

#[test]
fn stale_and_out_of_domain_calls_are_absent_from_the_symbolic_relation() {
    let model = monitor_boundary_model();
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let selected = monitor
        .enabled()
        .expect("enumerate selections")
        .into_iter()
        .find(|action| {
            action.action == "select" && action.params.get("v") == Some(&FslValue::Int(0))
        })
        .expect("select lower bound");
    monitor.step(&selected).expect("select value");
    let selected_state = monitor.state.clone();
    let enabled = monitor
        .enabled()
        .expect("enumerate executions")
        .into_iter()
        .find(|action| {
            action.action == "execute" && action.params.get("raw") == Some(&FslValue::Int(0))
        })
        .expect("execute selected value");
    let executed = monitor.step(&enabled).expect("execute value");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &selected_state,
            &enabled.action,
            &enabled.params,
            &executed.state,
        ))
        .expect("accept legal transition")
    );
    let current = monitor.state.clone();

    assert!(
        monitor
            .step(&enabled)
            .expect_err("stale action must fail")
            .message
            .contains("stale")
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &current,
            &enabled.action,
            &enabled.params,
            &current,
        ))
        .expect("reject stale transition")
    );

    let rejected = BTreeMap::from([("raw".to_owned(), FslValue::Int(2))]);
    let mut attempted = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    attempted.step(&selected).expect("select value");
    let result = attempted
        .attempt("execute", &rejected)
        .expect("raw input belongs to its API domain");
    assert_eq!(
        result
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("requires_failed")
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &selected_state,
            "execute",
            &rejected,
            &selected_state,
        ))
        .expect("reject raw Count violation")
    );

    let invalid = BTreeMap::from([("raw".to_owned(), FslValue::Int(3))]);
    let state_before_attempt = attempted.state.clone();
    assert!(
        attempted
            .attempt("execute", &invalid)
            .expect_err("out-of-domain parameter must fail")
            .message
            .contains("parameter")
    );
    assert_eq!(attempted.state, state_before_attempt);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        !block_on(fsl_verifier::transition_matches_step(
            &model,
            &mut solver,
            &selected_state,
            "execute",
            &invalid,
            &selected_state,
        ))
        .expect("reject out-of-domain transition")
    );
}

#[test]
fn bool_action_parameters_agree_between_symbolic_and_concrete_execution() {
    let model = build_model(
        parse_kernel_source(
            "spec BoolParameter { state { value: Bool } init { value = false } action set(v: Bool) { value = v } }",
            &FsResolver::new("."),
        )
        .expect("parse Bool action model"),
    )
    .expect("build Bool action model");
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    for value in [false, true] {
        let params = BTreeMap::from([("v".to_owned(), FslValue::Bool(value))]);
        let mut concrete = monitor.clone();
        let result = concrete
            .attempt("set", &params)
            .expect("execute Bool action");
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        assert!(
            block_on(fsl_verifier::transition_matches_step(
                &model,
                &mut solver,
                &monitor.state,
                "set",
                &params,
                &result.state,
            ))
            .expect("check Bool transition")
        );
    }
}
