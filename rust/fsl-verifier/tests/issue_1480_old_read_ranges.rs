// SPDX-License-Identifier: Apache-2.0
//! Old and current range reads use their own real, distinct execution frames.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceAction, TraceChange, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_verifier::{
    BmcResult, BmcViolation, InductionCti, InductionResult, VerifyError, prove_induction,
    verify_bounded, verify_bounded_selected,
};
use std::collections::{BTreeMap, BTreeSet};
const ELIGIBILITY: &str = "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope";
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
fn model(offset: i64) -> KernelModel {
    let source = format!(
        "spec OldRead {{state {{n:0..2}} init {{n=0}} action inc(){{requires n<2 n=n+1}} trans T {{count(i in 0..n)==old(count(i in 0..n))+{offset}}}}}"
    );
    let parsed = parse_kernel_source(&source, &FsResolver::new("."));
    eprintln!("OLD_READ_SOURCE offset={offset} source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("OLD_READ_MODEL offset={offset} actual={actual:#?}");
    actual.unwrap()
}
fn trace(before: i64, after: i64) -> Vec<TraceStep> {
    vec![
        TraceStep {
            step: 0,
            state: BTreeMap::from([("n".into(), V::Int(before))]),
            action: None,
            changes: BTreeMap::new(),
        },
        TraceStep {
            step: 1,
            state: BTreeMap::from([("n".into(), V::Int(after))]),
            action: Some(TraceAction {
                name: "inc".into(),
                params: BTreeMap::new(),
            }),
            changes: BTreeMap::from([(
                "n".into(),
                TraceChange {
                    from: V::Int(before),
                    to: V::Int(after),
                },
            )]),
        },
    ]
}
fn packet(violation: Option<BmcViolation>) -> BmcResult {
    BmcResult {
        spec: "OldRead".into(),
        depth: 1,
        violation,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: BTreeMap::from([("inc".into(), true)]),
        frontier_progress: false,
        vacuity: vec![],
    }
}
fn proof() -> InductionResult {
    InductionResult {
        k_used: BTreeMap::from([("_bounds_n".into(), 1)]),
        cti: None,
    }
}
#[test]
fn distinct_zero_to_one_frames_keep_old_count_and_reject_wrong_offset() {
    let outputs = [1, 2].map(|offset| {
        let model = model(offset);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(verify_bounded(&model, &mut solver, 1));
        eprintln!("OLD_READ_BMC offset={offset} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(packet(None)));
    let wrong_offset = packet(Some(BmcViolation {
        kind: "trans".into(),
        name: "T".into(),
        step: 1,
        last_action: Some("inc".into()),
        trace: trace(0, 1),
        leads_to: None,
    }));
    assert_eq!(outputs[1], Ok(wrong_offset));
    assert_ne!(outputs[1], Ok(packet(None)));
    // Current count=2, old count=1. Aliasing the old read to current makes
    // even the +1 accepting control false; clipping either roster also fails.
}
#[test]
fn conditional_induction_proves_the_relation_and_keeps_complete_abstract_ctis() {
    let outputs = [1, 2].map(|offset| {
        let model = model(offset);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(prove_induction(&model, &mut solver, 1));
        eprintln!("OLD_READ_INDUCTION offset={offset} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(proof()));
    // This API is conditional: init does not constrain its abstract predecessor.
    // The guard and carrier allow exactly 0->1 or 1->2, both whole literal CTIs.
    let alternatives = [(0, 1), (1, 2)].map(|(before, after)| {
        let mut result = proof();
        result.cti = Some(InductionCti {
            kind: "trans".into(),
            name: "T".into(),
            k: 1,
            trace: trace(before, after),
        });
        Ok(result)
    });
    assert!(alternatives.contains(&outputs[1]), "{:#?}", outputs[1]);
    assert_ne!(outputs[1], Ok(proof()));
}
#[test]
fn selecting_bounds_is_not_permission_to_skip_the_old_read_prerequisite() {
    let model = model(1);
    let included = BTreeSet::from(["_bounds_n".into()]);
    let omitted = BTreeSet::new();
    let outputs = [&included, &omitted].map(|selected| {
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(verify_bounded_selected(
            &model,
            &mut solver,
            1,
            Some(selected),
        ));
        eprintln!("OLD_READ_SELECTED selected={selected:?} actual={actual:#?}");
        actual
    });
    assert_eq!(outputs[0], Ok(packet(None)));
    assert_eq!(outputs[1], Err(VerifyError::new(ELIGIBILITY)));
    assert_ne!(outputs[1], Ok(packet(None)));
}

fn literal_model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("OLD_SEQUENCE_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("OLD_SEQUENCE_MODEL actual={actual:#?}");
    actual.unwrap()
}
#[test]
fn pure_count_transition_with_ensures_true_has_no_extra_definedness_refusal() {
    let model = literal_model(
        "spec OldRead {state {n:0..2} init {n=0} action inc(){requires n<2 n=n+1 ensures true} trans T {count(i in 0..n)==old(count(i in 0..n))+1}}",
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let bounded = block_on(verify_bounded(&model, &mut solver, 1));
    eprintln!("OLD_ENSURES_BMC actual={bounded:#?}");
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let induction = block_on(prove_induction(&model, &mut solver, 1));
    eprintln!("OLD_ENSURES_INDUCTION actual={induction:#?}");
    assert_eq!(bounded, Ok(packet(None)));
    assert_eq!(induction, Ok(proof()));
    assert_ne!(induction, Err(VerifyError::new(ELIGIBILITY)));
}
#[test]
fn pending_transition_is_rechecked_after_its_target_bound_proves_at_two() {
    let outputs = [(0, 1), (0, 2), (1, 2)].map(|(offset, depth)| {
        let source = format!(
            "spec OldDelayed {{state {{n:0..2}} init {{n=1}} action go(){{if n==0 {{n=n+3}} else {{n=1}}}} invariant NotTwo {{n!=2}} trans T {{count(i in 0..n)==old(count(i in 0..n))+{offset}}}}}"
        );
        let model = literal_model(&source);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(prove_induction(&model, &mut solver, depth));
        eprintln!("OLD_DELAYED offset={offset} depth={depth} actual={actual:#?}");
        actual
    });
    // Init1 is NOT an induction assumption. At k1, 0->3 is the unique
    // bound failure, while NotTwo already proves for both possible edges.
    let mut bound_trace = trace(0, 3);
    bound_trace[1].action = Some(TraceAction {
        name: "go".into(),
        params: BTreeMap::new(),
    });
    assert_eq!(
        outputs[0],
        Ok(InductionResult {
            k_used: BTreeMap::from([("NotTwo".into(), 1)]),
            cti: Some(InductionCti {
                kind: "invariant".into(),
                name: "_bounds_n".into(),
                k: 1,
                trace: bound_trace,
            }),
        })
    );
    // The additional prior bound at k2 excludes the earlier 0->3 chain.
    // NotTwo leaves only the complete 1->1->1 chain; both counts are2.
    let complete = InductionResult {
        k_used: BTreeMap::from([("NotTwo".into(), 1), ("_bounds_n".into(), 2)]),
        cti: None,
    };
    assert_eq!(outputs[1], Ok(complete.clone()));
    let mut false_relation = complete.clone();
    false_relation.cti = Some(InductionCti {
        kind: "trans".into(),
        name: "T".into(),
        k: 2,
        trace: [0, 1, 2]
            .map(|step| TraceStep {
                step,
                state: BTreeMap::from([("n".into(), V::Int(1))]),
                action: (step > 0).then(|| TraceAction {
                    name: "go".into(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            })
            .to_vec(),
    });
    assert_eq!(outputs[2], Ok(false_relation));
    assert_ne!(outputs[2], Ok(complete));
}
