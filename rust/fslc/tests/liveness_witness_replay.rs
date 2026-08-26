// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, KernelModel, TraceAction, TraceChange, TraceStep};
use fsl_verifier::BmcResult;

const DEPTH: usize = 5;

#[derive(Debug)]
struct LassoCase {
    name: &'static str,
    model: KernelModel,
    trace: Vec<TraceStep>,
    loop_start: usize,
    corrupted_loop_start: usize,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn load_model(relative: &str) -> KernelModel {
    let path = repository_root().join(relative);
    let source = std::fs::read_to_string(&path).expect("read leadsTo case");
    let resolver = FsResolver::new(path.parent().expect("case parent"));
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse leadsTo case");
    fsl_core::build_model(kernel).expect("build leadsTo model")
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn verify(model: &KernelModel) -> BmcResult {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create native solver");
    block_on(fsl_verifier::verify_bounded(model, &mut solver, DEPTH)).expect("verify leadsTo case")
}

fn state_changes(
    before: &BTreeMap<String, FslValue>,
    after: &BTreeMap<String, FslValue>,
) -> BTreeMap<String, TraceChange> {
    after
        .iter()
        .filter_map(|(name, value)| {
            let old = &before[name];
            (old != value).then(|| {
                (
                    name.clone(),
                    TraceChange {
                        from: old.clone(),
                        to: value.clone(),
                    },
                )
            })
        })
        .collect()
}

fn native_cycle(
    name: &'static str,
    relative: &str,
    actions: &[&str],
    loop_start: usize,
    corrupted_loop_start: usize,
) -> LassoCase {
    let model = load_model(relative);
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize native cycle");
    let mut trace = vec![TraceStep {
        step: 0,
        state: monitor.state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    for (index, action_name) in actions.iter().enumerate() {
        let enabled = monitor
            .enabled()
            .expect("enumerate enabled actions")
            .into_iter()
            .find(|action| action.action == *action_name)
            .unwrap_or_else(|| panic!("{name}: action {action_name} must be enabled"));
        let before = monitor.state.clone();
        let stepped = monitor.step(&enabled).expect("execute native cycle action");
        assert_eq!(stepped.violation, None, "{name}: clean cycle action");
        trace.push(TraceStep {
            step: index + 1,
            changes: state_changes(&before, &stepped.state),
            state: stepped.state,
            action: Some(TraceAction {
                name: enabled.action,
                params: enabled.params,
            }),
        });
    }
    assert_eq!(
        trace[loop_start].state,
        trace.last().expect("cycle tail").state,
        "{name}: declared loop must close"
    );
    LassoCase {
        name,
        model,
        trace,
        loop_start,
        corrupted_loop_start,
    }
}

fn cases() -> [LassoCase; 3] {
    [
        native_cycle(
            "starvation",
            "examples/gallery/errors/violated_leads_to_starvation.fsl",
            &["request", "idle"],
            1,
            0,
        ),
        native_cycle(
            "simultaneous",
            "examples/gallery/adversarial/simultaneous_leads_to_satisfaction.fsl",
            &["reset", "start_and_finish"],
            0,
            1,
        ),
        native_cycle(
            "tcp",
            "examples/gallery/valid/small_tcp_handshake.fsl",
            &["send_syn", "recv_syn_ack", "close"],
            0,
            1,
        ),
    ]
}

fn replay_lasso(model: &KernelModel, trace: &[TraceStep], loop_start: usize) -> Result<(), String> {
    fsl_runtime::replay_trace(model.clone(), trace).map_err(|error| error.to_string())?;
    let loop_head = trace
        .get(loop_start)
        .ok_or_else(|| format!("loop_start {loop_start} is outside the witness trace"))?;
    let loop_tail = trace
        .last()
        .ok_or_else(|| "empty leadsTo witness trace".to_owned())?;
    if loop_head.state != loop_tail.state {
        return Err(format!(
            "lasso loop state mismatch: trace[{loop_start}] != trace[{}]",
            loop_tail.step
        ));
    }
    Ok(())
}

#[test]
fn deleted_leadsto_matrix_keeps_native_verdicts_and_replays_each_cycle() {
    for (relative, expected_lasso) in [
        (
            "examples/gallery/errors/violated_leads_to_starvation.fsl",
            true,
        ),
        (
            "examples/gallery/adversarial/simultaneous_leads_to_satisfaction.fsl",
            false,
        ),
        ("examples/gallery/valid/small_tcp_handshake.fsl", false),
    ] {
        let result = verify(&load_model(relative));
        assert!(
            result.violation.is_none(),
            "unexpected safety violation for {relative}: {:?}",
            result.violation
        );
        assert_eq!(
            result.leadsto_violation.is_some(),
            expected_lasso,
            "unexpected leadsTo verdict for {relative}"
        );
    }
    for case in cases() {
        replay_lasso(&case.model, &case.trace, case.loop_start)
            .unwrap_or_else(|error| panic!("{} baseline cycle rejected: {error}", case.name));
    }
}

#[test]
fn all_nine_case_by_corruption_cells_are_rejected_exactly() {
    for case in cases() {
        let mut state = case.trace.clone();
        state[1]
            .state
            .insert("corrupted".to_owned(), FslValue::Bool(true));

        let mut action = case.trace.clone();
        action[1].action.as_mut().expect("first cycle action").name = "corrupted_action".to_owned();

        let loop_expected = format!(
            "lasso loop state mismatch: trace[{}] != trace[{}]",
            case.corrupted_loop_start,
            case.trace.last().expect("cycle tail").step
        );
        for (corruption, trace, loop_start, expected) in [
            (
                "state",
                state,
                case.loop_start,
                "trace state mismatch at step 1".to_owned(),
            ),
            (
                "action",
                action,
                case.loop_start,
                "trace action 'corrupted_action' is not enabled at step 1".to_owned(),
            ),
            (
                "loop",
                case.trace.clone(),
                case.corrupted_loop_start,
                loop_expected,
            ),
        ] {
            let produced = replay_lasso(&case.model, &trace, loop_start)
                .expect_err("corrupted lasso must be rejected");
            assert_eq!(
                produced, expected,
                "{} × {corruption} produced an unexpected diagnostic",
                case.name
            );
        }
    }
}
