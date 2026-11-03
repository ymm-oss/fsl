// SPDX-License-Identifier: Apache-2.0
//! Fresh D0 invariant assumptions and PROPERTY status share their own checked frame.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceStep, build_model, parse_kernel_source,
};
use fsl_solver::{SatResult, SmtSolver};
use fsl_verifier::{InductionCti, InductionResult, VerifyError, prove_induction};
use std::collections::BTreeMap;
const JUMP: &str = "spec Defined { type K=0..2 state {n:K} init {n=0} action jump(){requires n==0 n=2} invariant P {PREDICATE} }";
const QUEUE: &str = "spec DefinedQueue { type K=0..2 state {n:K,q:Seq<Int,1>} init {n=0 q=Seq {0}} action stay(){n=n q=q} invariant P {PREDICATE} }";
const REACHED: &str = "n!=0 or (forall i in 0..n {q.head()==q.head()})";
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
fn model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("INDUCTION_D0_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("INDUCTION_D0_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn run(source: &str) -> Result<InductionResult, VerifyError> {
    let model = model(source);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(prove_induction(&model, &mut solver, 1));
    let after = block_on(solver.check());
    eprintln!("INDUCTION_D0_RESULT actual={actual:#?} post_pop={after:?}");
    assert_eq!(after, Ok(SatResult::Sat));
    actual
}
fn proof(queue: bool) -> InductionResult {
    let mut k_used = BTreeMap::from([("_bounds_n".into(), 1), ("P".into(), 1)]);
    if queue {
        k_used.insert("_bounds_q".into(), 1);
    }
    InductionResult { k_used, cti: None }
}
fn partial() -> InductionResult {
    let mut result = proof(true);
    result.cti = Some(InductionCti {
        kind: "partial_op".into(),
        name: "_partial_property_P".into(),
        k: 0,
        trace: vec![TraceStep {
            step: 0,
            state: BTreeMap::from([("n".into(), V::Int(0)), ("q".into(), V::Seq(vec![]))]),
            action: None,
            changes: BTreeMap::new(),
        }],
    });
    result
}
#[test]
fn sum_identity_and_empty_range_division_finish_the_fresh_d0_obligation() {
    let outputs = [
        "sum(i in 0..n of i)==n*(n+1)/2",
        "n!=0 or (forall i in (n+1)..n {1/0>0})",
    ]
    .map(|predicate| run(&JUMP.replace("PREDICATE", predicate)));
    for actual in outputs {
        assert_eq!(actual, Ok(proof(false)));
    }
}
#[test]
fn reached_head_has_the_complete_abstract_property_partial_cti() {
    // D0 invariant queries do not assume action guards. P's short-circuit
    // reachability fixes n=0; the head failure fixes q=[] independently of init.
    let actual = run(&QUEUE.replace("PREDICATE", REACHED));
    assert_eq!(actual, Ok(partial()));
    assert_ne!(actual, Ok(proof(true)));
    let mut wrong_action_name = partial();
    wrong_action_name.cti.as_mut().unwrap().name = "_partial_P".into();
    assert_ne!(actual, Ok(wrong_action_name));
    let mut wrong_invariant_kind = partial();
    wrong_invariant_kind.cti.as_mut().unwrap().kind = "invariant".into();
    assert_ne!(actual, Ok(wrong_invariant_kind));
}
#[test]
fn empty_and_unreached_dynamic_bodies_do_not_create_a_head_cti() {
    let outputs = [
        "forall i in (n+1)..n {q.head()==q.head()}",
        "true or (forall i in 0..n {q.head()==q.head()})",
    ]
    .map(|predicate| run(&QUEUE.replace("PREDICATE", predicate)));
    for actual in outputs {
        assert_eq!(actual, Ok(proof(true)));
        assert_ne!(actual, Ok(partial()));
    }
}
#[test]
fn no_range_and_static_range_keep_property_division_total() {
    let outputs = ["1/0==0", "forall i in 0..2 {1/0==0}"]
        .map(|predicate| run(&JUMP.replace("PREDICATE", predicate)));
    for actual in outputs {
        assert_eq!(actual, Ok(proof(false)));
    }
}
#[test]
fn unranked_leadsto_before_and_after_have_property_status_not_a_rank_proof() {
    let source = JUMP.replace("PREDICATE", "n<=2");
    let source = format!(
        "{} leadsTo L {{ count(i in 0..n)==n+1 and 1/0==0 ~> sum(i in 0..n of i)==n*(n+1)/2 }} }}",
        source.strip_suffix('}').unwrap()
    );
    let actual = run(&source);
    // This API checks invariants and definedness; L acquires no k_used/rank row.
    assert_eq!(actual, Ok(proof(false)));
}
#[test]
fn leadsto_binder_n_shadows_state_n_in_property_status_range_sources() {
    let source = JUMP.replace("PREDICATE", "n<=2");
    let source = format!(
        "{} leadsTo L {{ forall n:K {{ count(i in 0..n)==n+1 and 1/0==0 ~> sum(i in 0..n of i)==n*(n+1)/2 }} }} }}",
        source.strip_suffix('}').unwrap()
    );
    let actual = run(&source);
    // Caller-shadowing preservation; this is not a calibrated bound_names detector.
    assert_eq!(actual, Ok(proof(false)));
}
