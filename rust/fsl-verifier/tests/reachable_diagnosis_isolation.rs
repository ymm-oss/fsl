// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_solver::SmtSolver;

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model() -> fsl_core::KernelModel {
    let source = r"
spec ReachableDiagnosisIsolation {
  type N = 0..2
  state { x: N }
  init { x = 0 }
  action stay() { x = x }
  invariant Zero { x == 0 }
  reachable Impossible { x == 2 }
}
";
    let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .expect("parse fixture");
    fsl_core::build_model(kernel).expect("build fixture")
}

#[test]
fn diagnosis_queries_never_enter_the_bmc_witness_session() {
    let model = model();
    let mut witness_solver = fsl_solver_z3::Z3Solver::new().expect("witness solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut witness_solver, 1))
        .expect("bounded verification");
    assert!(result.reachable_diagnostics.is_empty());
    assert!(
        !witness_solver
            .statistics()
            .properties
            .iter()
            .any(|property| property.kind == "reachable_diagnosis"),
        "diagnosis query polluted witness solver statistics"
    );

    let mut diagnosis_solver = fsl_solver_z3::Z3Solver::new().expect("diagnosis solver");
    let diagnoses = block_on(fsl_verifier::diagnose_reachables(
        &model,
        &mut diagnosis_solver,
    ))
    .expect("diagnose reachables");
    assert!(diagnoses.contains_key("Impossible"));
    assert!(
        diagnosis_solver
            .statistics()
            .properties
            .iter()
            .any(|property| property.kind == "reachable_diagnosis")
    );
}
