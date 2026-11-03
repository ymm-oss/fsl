// SPDX-License-Identifier: Apache-2.0
//! Conditional property obligations; Boolean truth never substitutes for index safety.
use fsl_core::{
    FsResolver, FslValue as V, KernelExpr as E, KernelModel, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_verifier::{
    InductionCti, InductionResult, RankFailure, RankedLeadstoResult, prove_induction,
    prove_ranked_leadstos,
};
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
    eprintln!("CURRENT_PROPERTY_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("CURRENT_PROPERTY_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn key_proof() -> InductionResult {
    InductionResult {
        k_used: BTreeMap::from([
            ("_bounds_n".into(), 1),
            ("MapZero".into(), 1),
            ("P".into(), 1),
        ]),
        cti: None,
    }
}
#[test]
fn current_key_property_truth_does_not_bless_a_real_map_key_miss() {
    let outputs=["n","(n+1)"].map(|endpoint| {
        let source=format!("spec KeyRange {{type K=0..2 state {{n:0..2,m:Map<K,Int>}} init {{n=0 forall k:K {{m[k]=0}}}} action go(){{}} invariant MapZero {{m[0]==0 and m[1]==0 and m[2]==0}} invariant P {{forall i in 0..{endpoint} {{m[i]==m[i]}}}} }}");
        let model=model(&source);
        let mut solver=fsl_solver_z3::Z3Solver::new().unwrap();
        let actual=block_on(prove_induction(&model,&mut solver,1));
        eprintln!("CURRENT_KEY endpoint={endpoint} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(key_proof()));
    // n2 is the ONLY valid n whose n+1 endpoint includes key3. MapZero,
    // not init, fixes all map entries in the fresh abstract key0 witness.
    let state = BTreeMap::from([
        ("n".into(), V::Int(2)),
        (
            "m".into(),
            V::Map(BTreeMap::from([
                (V::Int(0), V::Int(0)),
                (V::Int(1), V::Int(0)),
                (V::Int(2), V::Int(0)),
            ])),
        ),
    ]);
    let mut miss = key_proof();
    miss.cti = Some(InductionCti {
        kind: "key_domain".into(),
        name: "_key_domain_property_P".into(),
        k: 0,
        trace: vec![TraceStep {
            step: 0,
            state,
            action: None,
            changes: BTreeMap::new(),
        }],
    });
    assert_eq!(outputs[1], Ok(miss));
    assert_ne!(outputs[1], Ok(key_proof()));
}
#[test]
fn current_rank_trigger_reaches_the_exact_early_negative_measure_failure() {
    let model = model(
        "spec RankCurrent {state {x:0..5} init {x=0} action inc(){requires x<5 x=x+1} leadsTo Reach {x==1 and (forall i in 0..x {i<=5}) ~> x==5 decreases 0-x}}",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(prove_ranked_leadstos(&model, &mut solver));
    eprintln!("CURRENT_RANK_TRIGGER actual={actual:#?}");
    let expected = RankedLeadstoResult {
        proofs: vec![],
        failure: Some(RankFailure {
            name: "Reach".into(),
            bindings: BTreeMap::new(),
            measure: E::Binary {
                op: "-".into(),
                left: Box::new(E::Num(0)),
                right: Box::new(E::Var("x".into())),
            },
            kind: "unbounded_below".into(),
            measure_value: Some(-1),
            measure_before: None,
            measure_after: None,
            action: None,
            trace: vec![TraceStep {
                step: 0,
                state: BTreeMap::from([("x".into(), V::Int(1))]),
                action: None,
                changes: BTreeMap::new(),
            }],
            hint: "the decreases measure must be non-negative whenever the leadsTo trigger is pending (P holds and Q is false); add an invariant or use a bounded domain that proves the measure is >= 0".into(),
            message: "leadsTo 'Reach' decreases measure can be negative while P holds and Q is false".into(),
            helpful: vec![],
            helpful_actions: vec![],
        }),
    };
    assert_eq!(actual, Ok(expected));
    assert_ne!(
        actual,
        Ok(RankedLeadstoResult {
            proofs: vec![],
            failure: None
        })
    );
    // This real rank0 failure precedes unconditional state1 trigger evaluation.
    // No state1/deadlock-trigger reachability or full ranked success is claimed.
}
