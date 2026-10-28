// SPDX-License-Identifier: Apache-2.0

//! #1542 L1: undefined rank measures must not discharge BMC lasso searches.
//! M1 is a preservation control (base ranking already fails); M2 detects the
//! invalid arithmetic-masked proof. G1/N1 keep valid discharges available.

use std::collections::BTreeSet;
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

fn discharges(measure: &str, empty: bool) -> BTreeSet<usize> {
    let invariant = if empty {
        "invariant Empty { xs.size() == 0 }"
    } else {
        ""
    };
    let source = format!(
        "spec RankRead {{
  type K = 0..1
  state {{ xs: Seq<K, 1>, n: 0..2 }}
  init {{ xs = Seq {{}} n = 0 }}
  action tick() {{ requires n < 2 n = n + 1 }}
  action idle() {{ requires n == 2 }}
  {invariant}
  leadsTo L {{ n < 2 ~> n == 2 decreases {measure} }}
}}"
    );
    let kernel = parse_kernel_source(&source, &FsResolver::new(std::env::temp_dir()))
        .expect("parse fixture");
    let model = build_model(kernel).expect("build fixture");
    assert_eq!(model.leadstos.len(), 1);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    block_on(fsl_verifier::ranked_leadsto_lasso_discharges(
        &model,
        &mut solver,
        None,
    ))
}

#[test]
fn m1_direct_undefined_measure_discharges_nothing() {
    for _ in 0..2 {
        assert_eq!(discharges("xs.head()", false), BTreeSet::new());
    }
}

#[test]
fn m2_masked_undefined_measure_discharges_nothing() {
    assert_eq!(discharges("2 - n + 0 * xs.head()", false), BTreeSet::new());
}

#[test]
fn g1_guarded_measure_keeps_its_discharge() {
    // Empty is an invariant premise here, just as in the CLI control: this
    // isolates short-circuit definedness from non-decreasing nonempty heads.
    for _ in 0..2 {
        assert_eq!(
            discharges("if xs.size() > 0 then xs.head() else 2 - n", true),
            BTreeSet::from([0])
        );
    }
}

#[test]
fn n1_total_measure_keeps_its_discharge() {
    for _ in 0..2 {
        assert_eq!(discharges("2 - n", false), BTreeSet::from([0]));
    }
}
