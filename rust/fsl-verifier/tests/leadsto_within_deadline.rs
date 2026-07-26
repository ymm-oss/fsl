// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

// Q first holds one step after its `within` deadline, and the path then
// deadlocks at x == 3: the deadline is missed at step 2, the deadlock is at
// step 3 (issue #266).
const LATE_Q: &str = r"
spec LateQ {
  type Count = 0..3
  state { x: Count }
  init { x = 0 }
  action step() {
    requires x < 3
    x = x + 1
  }
  leadsTo Progress { x == 1 ~> within 1 x == 3 }
}
";

#[test]
fn within_deadline_miss_is_detected_at_every_depth_at_or_beyond_the_deadline() {
    let kernel =
        parse_kernel_source(LATE_Q, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");

    for depth in [2_usize, 3, 4, 6] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth))
            .expect("verify_bounded");
        assert!(result.violation.is_none(), "depth {depth}: {result:?}");
        let leadsto = result.leadsto_violation.as_ref().unwrap_or_else(|| {
            panic!("depth {depth}: expected a leadsTo violation, got {result:?}")
        });
        assert_eq!(leadsto.kind, "leadsTo", "depth {depth}: {leadsto:?}");
        let details = leadsto.leads_to.as_ref().expect("leadsTo details");
        assert_eq!(details.pending_since, 1, "depth {depth}: {leadsto:?}");
        assert_eq!(details.deadline, Some(2), "depth {depth}: {leadsto:?}");
        assert!(!details.stutter, "depth {depth}: {leadsto:?}");

        let mut runtime = fsl_runtime::BoundedLivenessMonitor::new(model.clone())
            .expect("runtime liveness monitor");
        let mut runtime_violation = None;
        for (step, observation) in leadsto.trace.iter().enumerate() {
            runtime_violation = runtime
                .observe(&observation.state, step)
                .expect("runtime observation");
            if runtime_violation.is_some() {
                break;
            }
        }
        let runtime = runtime_violation.expect("runtime deadline violation");
        assert_eq!(runtime.property, leadsto.name);
        assert_eq!(runtime.bindings, details.bindings);
        assert_eq!(runtime.pending_since, details.pending_since);
        assert_eq!(Some(runtime.deadline), details.deadline);
    }
}

#[test]
fn within_deadline_met_stays_verified_beyond_the_deadlock_step() {
    // Same path, but Q (x == 3) holds exactly on the `within 2` deadline, so
    // the property is satisfied and the trailing deadlock must not turn into
    // a spurious deadline miss.
    let source = LATE_Q.replace("within 1", "within 2");
    let kernel =
        parse_kernel_source(&source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");

    for depth in [3_usize, 4, 6] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth))
            .expect("verify_bounded");
        assert!(result.violation.is_none(), "depth {depth}: {result:?}");
        assert!(
            result.leadsto_violation.is_none(),
            "depth {depth}: {result:?}"
        );
    }
}

#[test]
fn bounded_and_ranked_leadsto_where_filters_fail_closed_through_one_owner() {
    let source = LATE_Q.replace(
        "leadsTo Progress { x == 1 ~> within 1 x == 3 }",
        "leadsTo Progress { forall i: Count where i > 0 { x == 1 ~> within 1 x == 3 } decreases 3 - x }",
    );
    let kernel =
        parse_kernel_source(&source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2))
        .expect_err("where filters must fail closed");
    assert!(error.message.contains("where filters"), "{error}");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create ranked solver");
    let ranked_error = block_on(fsl_verifier::prove_ranked_leadstos(&model, &mut solver))
        .expect_err("ranked where filters must fail closed through the same owner");
    assert_eq!(ranked_error.message, error.message);
}
