// SPDX-License-Identifier: Apache-2.0
//! Successor PROPERTY assumptions are reached only after their real frame bounds.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceAction, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_verifier::{InductionCti, InductionResult, prove_induction};
use std::collections::BTreeMap;
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
    eprintln!("POSTFRAME_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("POSTFRAME_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn proof(with_map: bool) -> InductionResult {
    let mut k_used = BTreeMap::from([
        ("_bounds_n".into(), 1),
        ("NZero".into(), 1),
        ("P".into(), 1),
    ]);
    if with_map {
        k_used.insert("MapZero".into(), 1);
    }
    InductionResult { k_used, cti: None }
}
fn trace(with_map: bool) -> Vec<TraceStep> {
    let mut state = BTreeMap::from([("n".into(), V::Int(0))]);
    if with_map {
        state.insert(
            "m".into(),
            V::Map(BTreeMap::from([
                (V::Int(0), V::Int(0)),
                (V::Int(1), V::Int(0)),
                (V::Int(2), V::Int(0)),
            ])),
        );
    }
    vec![
        TraceStep {
            step: 0,
            state: state.clone(),
            action: None,
            changes: BTreeMap::new(),
        },
        TraceStep {
            step: 1,
            state,
            action: Some(TraceAction {
                name: "stay".into(),
                params: BTreeMap::new(),
            }),
            changes: BTreeMap::new(),
        },
    ]
}
#[test]
fn late_def1_conditional_range_assumption_precedes_reached_ensures_verdict() {
    let outputs = ["true", "false"].map(|ensure| {
        let source = format!(
            "spec PostDefined {{state {{n:0..2}} init {{n=0}} action stay(){{ensures {ensure}}} invariant NZero {{n==0}} invariant P {{if n==0 then (forall i in 0..n {{i<=2}}) else true}}}}"
        );
        let model = model(&source);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(prove_induction(&model, &mut solver, 1));
        eprintln!("POSTFRAME_DEF1 ensure={ensure} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(proof(false)));
    let mut rejected = proof(false);
    rejected.cti = Some(InductionCti {
        kind: "ensures".into(),
        name: "stay".into(),
        k: 1,
        trace: trace(false),
    });
    assert_eq!(outputs[1], Ok(rejected));
    assert_ne!(outputs[1], Ok(proof(false)));
    // No transition property exists: the early def1 status loop is empty.
    // Ensures forces the late path to assert successor bounds/invariants,
    // including the current conditional range P, before its actual verdict.
}
#[test]
fn actual_key1_conditional_range_assumption_does_not_erase_a_real_key_miss() {
    let outputs = [1, 3].map(|offset| {
        let source = format!(
            "spec PostKey {{type K=0..2 state {{n:0..2,m:Map<K,Int>}} init {{n=0 forall k:K {{m[k]=0}}}} action stay(){{ensures m[n+{offset}]==m[n+{offset}]}} invariant NZero {{n==0}} invariant MapZero {{m[0]==0 and m[1]==0 and m[2]==0}} invariant P {{if n==0 then (forall i in 0..n {{i<=2}}) else true}}}}"
        );
        let model = model(&source);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(prove_induction(&model, &mut solver, 1));
        eprintln!("POSTFRAME_KEY1 offset={offset} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(proof(true)));
    let mut rejected = proof(true);
    rejected.cti = Some(InductionCti {
        kind: "key_domain".into(),
        name: "_key_domain_stay".into(),
        k: 1,
        trace: trace(true),
    });
    assert_eq!(outputs[1], Ok(rejected));
    assert_ne!(outputs[1], Ok(proof(true)));
    // The ensures key site is syntactically nonvacuous in BOTH twins.
    // NZero proves key1 valid; key3 misses K. MapZero fixes complete abstract
    // map witnesses independently of init; reflexive equality proves no safety.
    // No key0 invariant/guard/body key miss can preempt this key1 obligation.
}
