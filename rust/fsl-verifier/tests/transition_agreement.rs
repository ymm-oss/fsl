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

fn assert_post_failure_kind_rejections(
    model: &fsl_core::KernelModel,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    result: &fsl_runtime::StepResult,
    actual_kind: &str,
) {
    let Some(attempted) = result.attempted_state.as_ref() else {
        return;
    };
    for substituted_kind in ["type_bound", "invariant", "trans", "ensures"] {
        if substituted_kind == actual_kind {
            continue;
        }
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create substitution solver");
        let substituted = block_on(fsl_verifier::transition_outcome_matches_step(
            model,
            &mut solver,
            current,
            action,
            params,
            &result.state,
            Some(attempted),
            substituted_kind,
        ));
        assert_ne!(
            substituted,
            Ok(true),
            "{action} {actual_kind} accepted as {substituted_kind}"
        );
    }

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create success mutant solver");
    assert_ne!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            model,
            &mut solver,
            current,
            action,
            params,
            attempted,
            None,
            "ok",
        )),
        Ok(true),
        "{action} {actual_kind} accepted as ok"
    );
}

fn assert_failure_outcome_agreement(
    model: &fsl_core::KernelModel,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    result: &fsl_runtime::StepResult,
    kind: &str,
) {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create failure solver");
    let agreement = block_on(fsl_verifier::transition_outcome_matches_step(
        model,
        &mut solver,
        current,
        action,
        params,
        &result.state,
        result.attempted_state.as_ref(),
        kind,
    ));
    if kind == "partial_op" {
        assert!(
            agreement
                .expect_err("partial outcome must fail closed without an exact oracle")
                .message
                .contains("partial-operation")
        );
    } else if kind == "type_bound" {
        match agreement {
            Ok(true) => {}
            Err(error) if error.message.contains("over-capacity sequence") => {}
            other => panic!("{action} {kind} disagreed: {other:?}"),
        }
    } else {
        assert!(
            agreement.expect("check failure agreement"),
            "{action} {kind} disagreed"
        );
        assert_post_failure_kind_rejections(model, current, action, params, result, kind);
    }
}

fn assert_attempted_presence_corruption_rejected(
    model: &fsl_core::KernelModel,
    current: &BTreeMap<String, FslValue>,
    action: &str,
    params: &BTreeMap<String, FslValue>,
    result: &fsl_runtime::StepResult,
    kind: &str,
) {
    let corrupted_attempted = if result.attempted_state.is_some() {
        None
    } else {
        Some(current)
    };
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create corruption solver");
    let corrupted = block_on(fsl_verifier::transition_outcome_matches_step(
        model,
        &mut solver,
        current,
        action,
        params,
        &result.state,
        corrupted_attempted,
        kind,
    ));
    if kind == "partial_op" {
        assert!(
            corrupted
                .expect_err("partial corruption must fail closed")
                .message
                .contains("partial-operation")
        );
    } else {
        assert!(
            !corrupted.expect("reject corrupted failure evidence"),
            "{action} {kind} accepted the wrong attempted-state presence"
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
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create outcome solver");
            assert!(
                block_on(fsl_verifier::transition_outcome_matches_step(
                    &model,
                    &mut solver,
                    &current,
                    &enabled.action,
                    &enabled.params,
                    &result.state,
                    None,
                    "ok",
                ))
                .expect("check successful outcome agreement"),
                "{} outcome disagreed from {current:?}",
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
            assert_failure_outcome_agreement(
                &model,
                &current,
                &action.name,
                &params,
                &result,
                &violation.kind,
            );
            assert_attempted_presence_corruption_rejected(
                &model,
                &current,
                &action.name,
                &params,
                &result,
                &violation.kind,
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
            if action.name == "flip" {
                let mut solver = fsl_solver_z3::Z3Solver::new().expect("create outcome solver");
                assert!(
                    block_on(fsl_verifier::transition_outcome_matches_step(
                        &model,
                        &mut solver,
                        &current,
                        &action.name,
                        &params,
                        &result.state,
                        None,
                        "ok",
                    ))
                    .expect("check successful outcome agreement")
                );
            }
        }
    }
}

#[test]
fn enabled_non_stuttering_success_cannot_be_relabelled_as_a_failure() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fslc/tests/fixtures/conformance_failures.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read failure fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("flip", &BTreeMap::new())
        .expect("execute enabled success");
    assert!(result.violation.is_none());
    assert_ne!(result.state, current, "negative control must not stutter");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create rejection solver");
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "flip",
            &BTreeMap::new(),
            &current,
            None,
            "requires_failed",
        ))
        .expect("reject fabricated requires failure")
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create fail-closed solver");
    let error = block_on(fsl_verifier::transition_outcome_matches_step(
        &model,
        &mut solver,
        &current,
        "flip",
        &BTreeMap::new(),
        &current,
        None,
        "partial_op",
    ))
    .expect_err("partial-operation evidence must not pass without an exact oracle");
    assert!(error.message.contains("partial-operation agreement"));
}

#[test]
fn agreement_queries_do_not_contaminate_a_reused_solver() {
    let source = r"
spec ReusedAgreementSolver {
  type Bit = 0..1
  state { x: Bit }
  init { x = 0 }
  action flip() { x = 1 }
  action blocked() { requires false  x = 1 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = initial.state.clone();
    let params = BTreeMap::new();
    let mut flip = initial.clone();
    let success = flip.attempt("flip", &params).expect("execute flip");
    let mut blocked = initial.clone();
    let rejected = blocked
        .attempt("blocked", &params)
        .expect("execute blocked");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create reused solver");
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "flip",
            &params,
            &current,
            None,
            "requires_failed",
        ))
        .expect("reject fabricated guard failure")
    );
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "blocked",
            &params,
            &rejected.state,
            None,
            "requires_failed",
        ))
        .expect("accept genuine guard failure")
    );
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "flip",
            &params,
            &success.state,
            None,
            "ok",
        ))
        .expect("accept success after failure queries")
    );
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "flip",
            &params,
            &current,
            None,
            "ok",
        ))
        .expect("reject wrong successor after successful query")
    );
}

fn reached_partial_paths_model() -> fsl_core::KernelModel {
    let source = r"
spec ReachedPartialPaths {
  type Bit = 0..1
  state {
    x: Bit,
    queue: Seq<Bit, 0>,
    values: Map<Bit, Bit>
  }
  init {
    x = 0
    queue = Seq {}
    forall i: Bit { values[i] = i }
  }
  action skipped_guard() {
    requires false
    requires queue.head() == 0
    x = 1
  }
  action skipped_quantified_guard() {
    requires false
    requires forall i in 0..0 { queue.head() == i }
    x = 1
  }
  action reached_guard() {
    requires queue.head() == 0
    x = 1
  }
  action safe_branch() {
    if false { x = queue.head() } else { x = 1 }
  }
  action reached_branch() {
    if true { x = queue.head() } else { x = 1 }
  }
  action map_read() { x = values[1] }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    build_model(kernel).expect("build model")
}

#[test]
fn agreement_accepts_unreached_partial_operation_paths() {
    let model = reached_partial_paths_model();
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let params = BTreeMap::new();

    for action in ["skipped_guard", "skipped_quantified_guard"] {
        let mut skipped = initial.clone();
        let rejected = skipped
            .attempt(action, &params)
            .expect("short-circuit later guard");
        assert_eq!(
            rejected
                .violation
                .as_ref()
                .map(|violation| violation.kind.as_str()),
            Some("requires_failed")
        );
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create guard solver");
        assert!(
            block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &initial.state,
                action,
                &params,
                &rejected.state,
                None,
                "requires_failed",
            ))
            .expect("accept short-circuited guard failure")
        );
    }

    for action in ["safe_branch", "map_read"] {
        let mut monitor = initial.clone();
        let result = monitor
            .attempt(action, &params)
            .expect("execute defined operation path");
        assert!(result.violation.is_none(), "{action} must succeed");
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create success solver");
        assert!(
            block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &initial.state,
                action,
                &params,
                &result.state,
                None,
                "ok",
            ))
            .expect("accept reached defined operation path"),
            "{action} outcome disagreed"
        );
    }
}

#[test]
fn disabled_guard_does_not_evaluate_an_unreachable_body() {
    let source = r"
spec DisabledPartialBody {
  type Bit = 0..1
  state { x: Bit, dead: Seq<Bit, 0> }
  init { x = 0  dead = Seq {} }
  action disabled() {
    requires false
    x = dead.head()
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let current = fsl_runtime::Monitor::new(model.clone())
        .expect("create monitor")
        .state;
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create disabled solver");
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "disabled",
            &BTreeMap::new(),
            &current,
            None,
            "requires_failed",
        ))
        .expect("accept disabled guard without evaluating its body")
    );
}

#[test]
fn ordinary_bmc_remains_fail_closed_for_reached_zero_capacity_access() {
    let source = r"
spec ZeroCapacityBmc {
  type Bit = 0..1
  state { x: Bit, queue: Seq<Bit, 0> }
  init { x = 0  queue = Seq {} }
  action bad() { x = queue.head() }
  invariant Stay { x == 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let concrete = monitor
        .attempt("bad", &BTreeMap::new())
        .expect("classify concrete partial operation");
    assert_eq!(
        concrete
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("partial_op")
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create BMC solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect_err("ordinary BMC must not consume agreement-only fallback values");
    assert!(
        error.message.contains("zero-capacity sequence"),
        "{error:?}"
    );
}

#[test]
fn agreement_rejects_reached_partial_operation_paths() {
    let model = reached_partial_paths_model();
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let params = BTreeMap::new();

    let mut reached_guard = initial.clone();
    let partial_guard = reached_guard
        .attempt("reached_guard", &params)
        .expect("classify reached partial guard");
    assert_eq!(
        partial_guard
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("partial_op")
    );
    let mut fabricated_success = initial.state.clone();
    fabricated_success.insert("x".to_owned(), FslValue::Int(1));
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create partial-guard solver");
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &initial.state,
            "reached_guard",
            &params,
            &fabricated_success,
            None,
            "ok",
        ))
        .expect("reject reached partial guard as success")
    );

    let mut reached_branch = initial.clone();
    let partial_branch = reached_branch
        .attempt("reached_branch", &params)
        .expect("classify reached partial branch");
    assert_eq!(
        partial_branch
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("partial_op")
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create partial-branch solver");
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &initial.state,
            "reached_branch",
            &params,
            &initial.state,
            None,
            "ok",
        ))
        .expect("reject reached partial branch as success")
    );
}

#[test]
fn earlier_post_phase_failure_dominates_later_partial_syntax() {
    let source = r"
spec OrderedPostFailure {
  type Bit = 0..1
  state { x: Bit, queue: Seq<Bit, 0> }
  init { x = 0  queue = Seq {} }
  action overflow() { x = 2 }
  action fail_invariant() { x = 1 }
  invariant FirstFailure { x == 0 }
  invariant LaterPartial { queue.head() == 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("overflow", &BTreeMap::new())
        .expect("execute bound failure before invariant");
    assert_eq!(
        result
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("type_bound")
    );
    let attempted = result
        .attempted_state
        .as_ref()
        .expect("bound failure preserves attempted state");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create ordered-phase solver");
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "overflow",
            &BTreeMap::new(),
            &result.state,
            Some(attempted),
            "type_bound",
        ))
        .expect("accept first reached post failure")
    );

    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("fail_invariant", &BTreeMap::new())
        .expect("execute invariant failure before later partial invariant");
    assert_eq!(
        result
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("invariant")
    );
    let attempted = result
        .attempted_state
        .as_ref()
        .expect("invariant failure preserves attempted state");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create invariant-phase solver");
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "fail_invariant",
            &BTreeMap::new(),
            &result.state,
            Some(attempted),
            "invariant",
        ))
        .expect("accept earlier invariant failure")
    );
}

#[test]
fn old_expression_definedness_uses_the_pre_state() {
    let source = r"
spec OldDefinedness {
  type Bit = 0..1
  state { queue: Seq<Bit, 1> }
  init { queue = Seq {} }
  action fill() { queue = queue.push(0) }
  trans OldHead { old(queue.head()) == 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("fill", &BTreeMap::new())
        .expect("classify partial old-state expression");
    assert_eq!(
        result
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("partial_op")
    );
    let attempted = result
        .attempted_state
        .as_ref()
        .expect("post-update partial preserves attempted state");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create old-state solver");
    assert!(
        !block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "fill",
            &BTreeMap::new(),
            attempted,
            None,
            "ok",
        ))
        .expect("reject partial old-state expression as success")
    );
}

#[test]
fn checked_integer_overflow_cannot_be_relabelled_as_success() {
    let source = r"
spec CheckedIntegerAgreement {
  state { minimum: Int, maximum: Int, values: Seq<Int, 3> }
  init {
    minimum = -9223372036854775807 - 1
    maximum = 9223372036854775807
    values = Seq { 9223372036854775807, 1, -1 }
  }
  action abs_overflow() {
    minimum = minimum
    values = values
    ensures abs(minimum) > 0
  }
  action sum_overflow() {
    minimum = minimum
    values = values
    ensures sum(item in values of item) > 0
  }
  action negation_overflow() {
    ensures -minimum > 0
  }
  action addition_overflow() {
    ensures maximum + 1 > maximum
  }
  action subtraction_overflow() {
    ensures minimum - 1 < minimum
  }
  action multiplication_overflow() {
    ensures maximum * 2 > maximum
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");

    for (action, expected_error) in [
        ("abs_overflow", "integer overflow in abs"),
        ("sum_overflow", "integer overflow in sum"),
        ("negation_overflow", "integer overflow in negation"),
        ("addition_overflow", "integer overflow in addition"),
        ("subtraction_overflow", "integer overflow in subtraction"),
        (
            "multiplication_overflow",
            "integer overflow in multiplication",
        ),
    ] {
        let mut monitor = initial.clone();
        let error = monitor
            .attempt(action, &BTreeMap::new())
            .expect_err("native checked arithmetic must reject the action");
        assert!(error.message.contains(expected_error), "{error:?}");

        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create overflow solver");
        assert!(
            !block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &initial.state,
                action,
                &BTreeMap::new(),
                &initial.state,
                None,
                "ok",
            ))
            .expect("reject fabricated checked-arithmetic success"),
            "{action} overflow was accepted as success"
        );
    }
}

#[test]
fn representable_scalar_type_bound_outcome_agrees() {
    let source = r"
spec ScalarTypeBoundAgreement {
  type Bit = 0..1
  state { x: Bit }
  init { x = 0 }
  action overflow() { x = 2 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("overflow", &BTreeMap::new())
        .expect("execute overflowing assignment");
    assert_eq!(
        result
            .violation
            .as_ref()
            .map(|violation| violation.kind.as_str()),
        Some("type_bound")
    );
    let attempted = result
        .attempted_state
        .as_ref()
        .expect("type-bound failure preserves attempted state");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create type-bound solver");
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "overflow",
            &BTreeMap::new(),
            &result.state,
            Some(attempted),
            "type_bound",
        ))
        .expect("check representable type-bound agreement")
    );
}

#[test]
fn over_capacity_sequence_suffix_cannot_pass_exact_agreement() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fslc/tests/fixtures/conformance_failures.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read failure fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(fixture.parent().expect("fixture directory")),
    )
    .expect("parse model");
    let model = build_model(kernel).expect("build model");
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("create monitor");
    let current = monitor.state.clone();
    let result = monitor
        .attempt("type_bound", &BTreeMap::new())
        .expect("execute sequence overflow");
    let mut altered = result
        .attempted_state
        .expect("sequence overflow preserves attempted state");
    let FslValue::Seq(values) = altered.get_mut("queue").expect("queue state") else {
        panic!("queue must be a sequence");
    };
    assert!(
        values.len() > 1,
        "negative control needs an overflow suffix"
    );
    values[1] = FslValue::Int(1);

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create overflow-suffix solver");
    let error = block_on(fsl_verifier::transition_outcome_matches_step(
        &model,
        &mut solver,
        &current,
        "type_bound",
        &BTreeMap::new(),
        &result.state,
        Some(&altered),
        "type_bound",
    ))
    .expect_err("unrepresentable overflow suffix must fail closed");
    assert!(error.message.contains("over-capacity sequence"));
}

#[test]
fn outcome_agreement_rejects_malformed_calls_and_state_shapes() {
    let source = r"
spec MalformedAgreement {
  type Bit = 0..1
  struct Pair { value: Bit }
  state { x: Bit, pair: Pair }
  init { x = 0  pair = Pair { value: 0 } }
  action set(v: Bit) { requires false  x = v }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let current = fsl_runtime::Monitor::new(model.clone())
        .expect("create monitor")
        .state;

    for malformed_params in [
        BTreeMap::new(),
        BTreeMap::from([("v".to_owned(), FslValue::Int(2))]),
        BTreeMap::from([("wrong".to_owned(), FslValue::Int(0))]),
    ] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create malformed-call solver");
        assert!(
            block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &current,
                "set",
                &malformed_params,
                &current,
                None,
                "requires_failed",
            ))
            .is_err()
        );
    }

    let good_params = BTreeMap::from([("v".to_owned(), FslValue::Int(0))]);
    let mut missing_state = current.clone();
    missing_state.remove("x");
    let mut extra_state = current.clone();
    extra_state.insert("extra".to_owned(), FslValue::Bool(false));
    let mut wrong_state_type = current.clone();
    wrong_state_type.insert("x".to_owned(), FslValue::Bool(false));
    let mut out_of_bound_state = current.clone();
    out_of_bound_state.insert("x".to_owned(), FslValue::Int(2));
    let mut extra_struct_field = current.clone();
    let FslValue::Struct { fields, .. } = extra_struct_field.get_mut("pair").expect("pair state")
    else {
        panic!("pair must be a struct");
    };
    fields.insert("extra".to_owned(), FslValue::Int(0));

    for malformed_state in [
        missing_state,
        extra_state,
        wrong_state_type,
        out_of_bound_state,
        extra_struct_field,
    ] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create malformed-state solver");
        assert!(
            block_on(fsl_verifier::transition_outcome_matches_step(
                &model,
                &mut solver,
                &malformed_state,
                "set",
                &good_params,
                &malformed_state,
                None,
                "requires_failed",
            ))
            .is_err()
        );
    }

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create unknown-action solver");
    assert!(
        block_on(fsl_verifier::transition_outcome_matches_step(
            &model,
            &mut solver,
            &current,
            "missing",
            &BTreeMap::new(),
            &current,
            None,
            "partial_op",
        ))
        .is_err()
    );
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
