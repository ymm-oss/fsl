// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, KernelModel};
use fsl_verifier::BmcResult;

const DEPTH: usize = 5;

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

fn replay_liveness_witness(model: &KernelModel, result: &BmcResult) -> Result<(), String> {
    fslc_rust::verification_output::replay_bmc_witnesses(model, result, None)?;
    let violation = result
        .leadsto_violation
        .as_ref()
        .ok_or_else(|| "missing leadsTo violation".to_owned())?;
    let details = violation
        .leads_to
        .as_ref()
        .ok_or_else(|| "missing leadsTo lasso metadata".to_owned())?;
    let loop_start = details
        .loop_start
        .ok_or_else(|| "witness is a stall, not a lasso".to_owned())?;
    let loop_head = violation
        .trace
        .get(loop_start)
        .ok_or_else(|| format!("loop_start {loop_start} is outside the witness trace"))?;
    let loop_tail = violation
        .trace
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
fn deleted_leadsto_matrix_has_native_verdicts_and_replays_every_lasso() {
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
        let model = load_model(relative);
        let result = verify(&model);
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
        if expected_lasso {
            replay_liveness_witness(&model, &result)
                .unwrap_or_else(|error| panic!("native lasso replay rejected {relative}: {error}"));
        }
    }
}

#[test]
fn isolated_state_action_and_loop_corruptions_are_rejected() {
    let model = load_model("examples/gallery/errors/violated_leads_to_starvation.fsl");
    let baseline = verify(&model);
    replay_liveness_witness(&model, &baseline).expect("baseline lasso replays");

    let mut state = baseline.clone();
    state
        .leadsto_violation
        .as_mut()
        .expect("leadsTo violation")
        .trace[1]
        .state
        .insert("corrupted".to_owned(), FslValue::Bool(true));

    let mut action = baseline.clone();
    action
        .leadsto_violation
        .as_mut()
        .expect("leadsTo violation")
        .trace[1]
        .action
        .as_mut()
        .expect("request action")
        .name = "corrupted_action".to_owned();

    let mut loop_start = baseline;
    loop_start
        .leadsto_violation
        .as_mut()
        .expect("leadsTo violation")
        .leads_to
        .as_mut()
        .expect("leadsTo metadata")
        .loop_start = Some(0);

    for (field, corrupted, expected) in [
        ("state", state, "trace state mismatch"),
        ("action", action, "is not enabled"),
        ("loop", loop_start, "lasso loop state mismatch"),
    ] {
        let produced = replay_liveness_witness(&model, &corrupted)
            .expect_err("corrupted lasso must be rejected");
        assert!(
            produced.contains(expected),
            "{field} corruption produced {produced:?}; expected diagnostic containing {expected:?}"
        );
    }
}
