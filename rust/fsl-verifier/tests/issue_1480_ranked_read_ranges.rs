// SPDX-License-Identifier: Apache-2.0
//! Conditional ranked READ proofs; these pre-pass APIs are not a BMC base verdict.
use fsl_core::{FsResolver, KernelExpr as E, KernelModel, build_model, parse_kernel_source};
use fsl_verifier::{
    RankProof, RankedLeadstoResult, prove_ranked_leadstos, ranked_leadsto_lasso_discharges,
};
use std::collections::BTreeSet;
const SOURCE: &str = r"spec NeedsBound {
  state { x: 0..5 }
  init { x=0 }
  action inc() {
    requires x!=5 and (forall i in 0..x {i<=5})
    x=x+1
  }
  leadsTo Reach { x!=5 ~> x==5 decreases 5-x }
}";
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}
fn model() -> KernelModel {
    let parsed = parse_kernel_source(SOURCE, &FsResolver::new("."));
    eprintln!("RANKED_READ_SOURCE source={SOURCE:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("RANKED_READ_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn expected() -> RankedLeadstoResult {
    RankedLeadstoResult {
        proofs: vec![RankProof {
            name: "Reach".into(),
            measure: E::Binary {
                op: "-".into(),
                left: Box::new(E::Num(5)),
                right: Box::new(E::Var("x".into())),
            },
            helpful: vec![],
        }],
        failure: None,
    }
}
#[test]
fn actual_rank_and_deadlock_read_frames_prove_the_literal_conditional_rank() {
    let model = model();
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(prove_ranked_leadstos(&model, &mut solver));
    eprintln!("RANKED_READ_FULL actual={actual:#?}");
    assert_eq!(actual, Ok(expected()));
    assert_ne!(
        actual,
        Ok(RankedLeadstoResult {
            proofs: vec![],
            failure: None
        })
    );
}
#[test]
fn matching_bound_selection_discharges_but_omission_keeps_the_lasso_search() {
    let model = model();
    let included = BTreeSet::from(["_bounds_x".to_owned()]);
    let omitted = BTreeSet::new();
    // These are the existing public pre-pass configurations, not fabricated
    // successful base evidence or hand-asserted solver bounds.
    let outputs = [None, Some(&included), Some(&omitted)].map(|selected| {
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(ranked_leadsto_lasso_discharges(
            &model,
            &mut solver,
            selected,
        ));
        eprintln!("RANKED_READ_SELECTED selected={selected:?} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], BTreeSet::from([0]));
    assert_eq!(outputs[1], BTreeSet::from([0]));
    assert_eq!(outputs[2], BTreeSet::new());
    assert_ne!(outputs[2], BTreeSet::from([0]));
}
