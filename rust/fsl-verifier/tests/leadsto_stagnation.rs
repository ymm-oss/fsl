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

const STUCK_EARLY: &str = r"
spec StuckEarly {
  enum Stage { Idle, Pending, Done }
  state { stage: Stage }
  init { stage = Idle }
  action submit() {
    requires stage == Idle
    stage = Pending
  }
  leadsTo Progress { stage == Pending ~> stage == Done }
}
";

#[test]
fn leadsto_stagnation_is_detected_at_every_depth_at_or_beyond_the_deadlock_step() {
    let kernel =
        parse_kernel_source(STUCK_EARLY, &FsResolver::new(std::env::temp_dir())).expect("parse");
    let model = build_model(kernel).expect("build model");

    for depth in [1_usize, 2, 6] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth))
            .expect("verify_bounded");
        assert!(result.violation.is_none(), "depth {depth}: {result:?}");
        let leadsto = result.leadsto_violation.as_ref().unwrap_or_else(|| {
            panic!("depth {depth}: expected a leadsTo violation, got {result:?}")
        });
        assert_eq!(leadsto.kind, "leadsTo", "depth {depth}: {leadsto:?}");
        assert_eq!(
            leadsto.leads_to.as_ref().map(|leads_to| leads_to.stutter),
            Some(true),
            "depth {depth}: {leadsto:?}"
        );
    }
}
