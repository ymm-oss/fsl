// SPDX-License-Identifier: Apache-2.0
//! Actual action read-frame bounds authorize execution without public-context leakage.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceAction, TraceChange, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_verifier::{
    BmcResult, BmcViolation, VerifyError, transition_matches_step, verify_bounded_from_state,
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
fn model(body: &str) -> KernelModel {
    let source = format!(
        "spec A {{ type K=0..2 state {{n:K,m:Map<K,Int>}} init {{n=0 forall k:K {{m[k]=0}}}} action go() {{{body}}} invariant P {{m[0]==0 and m[1]==0 and m[2]==0}} }}"
    );
    let parsed = parse_kernel_source(&source, &FsResolver::new("."));
    eprintln!("ACTION_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("ACTION_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn state(n: i64, entries: [i64; 3]) -> BTreeMap<String, V> {
    BTreeMap::from([
        ("n".into(), V::Int(n)),
        (
            "m".into(),
            V::Map(BTreeMap::from([
                (V::Int(0), V::Int(entries[0])),
                (V::Int(1), V::Int(entries[1])),
                (V::Int(2), V::Int(entries[2])),
            ])),
        ),
    ])
}
fn packet(violation: Option<BmcViolation>, covered: bool) -> BmcResult {
    BmcResult {
        spec: "A".into(),
        depth: 1,
        violation,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: BTreeMap::from([("go".into(), covered)]),
        frontier_progress: false,
        vacuity: vec![],
    }
}
fn violation(
    n: i64,
    kind: &str,
    name: &str,
    after: Option<[i64; 3]>,
    record_delta: bool,
) -> BmcViolation {
    let before = state(n, [0, 0, 0]);
    let mut trace = vec![TraceStep {
        step: 0,
        state: before.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    if let Some(values) = after {
        let next = state(n, values);
        let changes = if record_delta {
            BTreeMap::from([(
                "m".into(),
                TraceChange {
                    from: before["m"].clone(),
                    to: next["m"].clone(),
                },
            )])
        } else {
            BTreeMap::new()
        };
        trace.push(TraceStep {
            step: 1,
            state: next,
            action: Some(TraceAction {
                name: "go".into(),
                params: BTreeMap::new(),
            }),
            changes,
        });
    }
    BmcViolation {
        kind: kind.into(),
        name: name.into(),
        step: usize::from(after.is_some()),
        last_action: after.map(|_| "go".into()),
        trace,
        leads_to: None,
    }
}
fn run(body: &str, n: i64, excluded: bool) -> Result<BmcResult, VerifyError> {
    let model = model(body);
    let initial = state(n, [0, 0, 0]);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let selected = BTreeSet::new();
    let actual = block_on(verify_bounded_from_state(
        &model,
        &mut solver,
        1,
        excluded.then_some(&selected),
        &initial,
    ));
    eprintln!(
        "ACTION_BOUNDED body={body:?} initial={initial:#?} excluded={excluded} actual={actual:#?}"
    );
    actual
}
#[test]
fn empty_and_filtered_bodies_skip_division_but_reached_siblings_stay_partial() {
    let outputs = [
        ("forall i in (n+1)..n {m[i]=1/i}", false),
        ("forall i in (n+1)..n {m[i]=1/i} ensures true", false),
        ("forall i in n..n where false {m[i]=1/i}", false),
        ("forall i in n..n where true {m[i]=1/i}", true),
        ("forall i in n..n {m[i]=1/i}", true),
        ("forall i in (n+1)..n {m[i]=1/i} n=1/0", true),
        ("m[0]=1/0", true),
    ]
    .map(|(body, partial)| (run(body, 0, false), partial));
    for (actual, partial) in outputs {
        let expected = if partial {
            let failure = violation(0, "partial_op", "_partial_go", Some([0, 0, 0]), false);
            packet(Some(failure), false)
        } else {
            packet(None, true)
        };
        assert_eq!(actual, Ok(expected));
        if partial {
            assert_ne!(actual, Ok(packet(None, true)));
        }
    }
}
#[test]
fn target_candidate_two_is_retained_in_the_whole_invariant_witness() {
    let actual = run("forall i in 0..n {m[i]=i}", 2, false);
    let expected = packet(
        Some(violation(2, "invariant", "P", Some([0, 1, 2]), true)),
        true,
    );
    assert_eq!(actual, Ok(expected));
    let clipped = packet(
        Some(violation(2, "invariant", "P", Some([0, 1, 0]), true)),
        true,
    );
    assert_ne!(actual, Ok(clipped));
}
#[test]
fn invalid_actual_state_has_type_bound_priority_before_action_partiality() {
    let actual = run("forall i in 0..n {m[i]=1/i}", 3, false);
    assert_eq!(
        actual,
        Ok(packet(
            Some(violation(3, "type_bound", "_bounds_n", None, false)),
            false
        ))
    );
}
#[test]
fn excluded_bounds_refuse_and_validated_agreement_checks_candidate_two() {
    let body = "forall i in 0..n {m[i]=i}";
    // An excluded bound reports nothing, but the frame still has to prove what it reads.
    let in_bounds = run(body, 0, true).unwrap();
    assert_eq!(in_bounds.violation, None);
    assert_eq!(run(body, 3, true), Err(VerifyError::new(ELIGIBILITY)));
    let model = model(body);
    let initial = state(2, [0, 0, 0]);
    let next = state(2, [0, 1, 2]);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(transition_matches_step(
        &model,
        &mut solver,
        &initial,
        "go",
        &BTreeMap::new(),
        &next,
    ));
    eprintln!("ACTION_PUBLIC initial={initial:#?} next={next:#?} actual={actual:#?}");
    let wrong_next = state(2, [0, 1, 0]);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let rejected = block_on(transition_matches_step(
        &model,
        &mut solver,
        &initial,
        "go",
        &BTreeMap::new(),
        &wrong_next,
    ));
    eprintln!("ACTION_PUBLIC_WRONG initial={initial:#?} next={wrong_next:#?} actual={rejected:#?}");
    assert_eq!(actual, Ok(true));
    assert_eq!(rejected, Ok(false));
}
