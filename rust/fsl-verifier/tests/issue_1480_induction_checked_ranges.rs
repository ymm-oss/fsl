// SPDX-License-Identifier: Apache-2.0
//! Public induction controls: prior assumptions never certify the target frame.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceAction, TraceChange, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_solver::{SatResult, SmtSolver, Sort};
use fsl_verifier::{
    InductionCti, InductionResult, VerifyError, prove_induction, prove_induction_invariants,
};
use std::collections::BTreeMap;
#[path = "support/induction_target_fault.rs"]
mod fault;
use fault::{Fault, FaultSolver};
const JUMP: &str = "spec Jump { type K=0..2 state {n:K} init {n=0} action jump(){requires n==0 n=2} invariant P {forall i in 0..n {i<=2}} }";
const TWO: &str = "spec Two { type K=0..2 type R=0..0 state {n:K,r:R} init {n=0 r=0} action step(){n=if n==2 then 3 else 0 r=0} invariant P {forall i in 0..n {i<=2}} }";
const MIXED: &str = "spec Mixed { type K=0..2 type R=0..2 state {n:K,r:R} init {n=0 r=0} action jump(){requires n==0 and r==0 n=3 r=2} invariant P {forall i in 0..n {i<=2}} invariant Q {r<=1} }";
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
    eprintln!("INDUCTION_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("INDUCTION_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn proof(used: &[(&str, usize)]) -> InductionResult {
    InductionResult {
        k_used: used.iter().map(|(name, k)| ((*name).into(), *k)).collect(),
        cti: None,
    }
}
fn row(values: &[(&str, i64)]) -> BTreeMap<String, V> {
    values
        .iter()
        .map(|(name, value)| ((*name).into(), V::Int(*value)))
        .collect()
}
fn change(from: i64, to: i64) -> TraceChange {
    TraceChange {
        from: V::Int(from),
        to: V::Int(to),
    }
}
fn trace(
    action: &str,
    before: &[(&str, i64)],
    after: &[(&str, i64)],
    changes: &[(&str, i64, i64)],
) -> Vec<TraceStep> {
    vec![
        TraceStep {
            step: 0,
            state: row(before),
            action: None,
            changes: BTreeMap::new(),
        },
        TraceStep {
            step: 1,
            state: row(after),
            action: Some(TraceAction {
                name: action.into(),
                params: BTreeMap::new(),
            }),
            changes: changes
                .iter()
                .map(|&(name, from, to)| (name.into(), change(from, to)))
                .collect(),
        },
    ]
}
fn counterexample(used: &[(&str, usize)], name: &str, trace: Vec<TraceStep>) -> InductionResult {
    let mut result = proof(used);
    result.cti = Some(InductionCti {
        kind: "invariant".into(),
        name: name.into(),
        k: 1,
        trace,
    });
    result
}
fn run(source: &str, k: usize) -> Result<InductionResult, VerifyError> {
    let m = model(source);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(prove_induction_invariants(&m, &mut solver, k));
    let after = block_on(solver.check());
    eprintln!("INDUCTION_RUN k={k} actual={actual:#?} post_pop={after:?}");
    assert_eq!(&after, &Ok(SatResult::Sat));
    actual
}
#[test]
fn prior_zero_and_target_two_keep_candidate_two_and_its_opposite_cti() {
    let positive = run(JUMP, 1);
    let opposite = run(&JUMP.replace("i<=2", "i<=1"), 1);
    let expected = proof(&[("_bounds_n", 1), ("P", 1)]);
    assert_eq!(&positive, &Ok(expected.clone()));
    assert_eq!(
        &opposite,
        &Ok(counterexample(
            &[("_bounds_n", 1)],
            "P",
            trace("jump", &[("n", 0)], &[("n", 2)], &[("n", 0, 2)]),
        )),
    );
    assert_ne!(opposite, Ok(expected.clone()));
    let m = model(JUMP);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let complete = block_on(prove_induction(&m, &mut solver, 1));
    eprintln!("INDUCTION_COMPLETE actual={complete:#?}");
    assert_eq!(&complete, &Ok(expected));
}
#[test]
fn target_bound_failure_defers_p_and_later_independent_q_keeps_cti_order() {
    let escaped = run(&JUMP.replace("n=2}", "n=3}"), 1);
    let mixed = run(MIXED, 1);
    assert_eq!(
        &escaped,
        &Ok(counterexample(
            &[],
            "_bounds_n",
            trace("jump", &[("n", 0)], &[("n", 3)], &[("n", 0, 3)]),
        )),
    );
    let witness = trace(
        "jump",
        &[("n", 0), ("r", 0)],
        &[("n", 3), ("r", 2)],
        &[("n", 0, 3), ("r", 0, 2)],
    );
    assert_eq!(
        &mixed,
        &Ok(counterexample(&[("_bounds_r", 1)], "Q", witness.clone())),
    );
    for wrong in ["_bounds_n", "P"] {
        assert_ne!(
            mixed,
            Ok(counterexample(&[("_bounds_r", 1)], wrong, witness.clone()))
        );
    }
    assert_ne!(escaped, Ok(proof(&[("_bounds_n", 1), ("P", 1)])));
}
#[test]
fn deferred_k1_bound_and_property_discharge_at_k2_keep_first_success() {
    let first = run(TWO, 1);
    let second = run(TWO, 2);
    assert_eq!(
        &first,
        &Ok(counterexample(
            &[("_bounds_r", 1)],
            "_bounds_n",
            trace(
                "step",
                &[("n", 2), ("r", 0)],
                &[("n", 3), ("r", 0)],
                &[("n", 2, 3)],
            ),
        )),
    );
    assert_eq!(
        &second,
        &Ok(proof(&[("_bounds_n", 2), ("_bounds_r", 1), ("P", 2)])),
    );
    assert_ne!(first, Ok(proof(&[("_bounds_r", 1)])));
    assert_ne!(
        second,
        Ok(proof(&[("_bounds_n", 2), ("_bounds_r", 2), ("P", 2)]))
    );
}
#[test]
fn no_range_and_static_range_keep_complete_ordinary_results() {
    for predicate in ["n<=2", "forall i in 0..2 {i<=2}"] {
        let actual = run(&JUMP.replace("forall i in 0..n {i<=2}", predicate), 1);
        assert_eq!(&actual, &Ok(proof(&[("_bounds_n", 1), ("P", 1)])));
    }
}
#[test]
fn actual_target_bound_probe_faults_are_errors_with_clean_outer_stack() {
    use Fault::{Assert, Backend, Pop, Unknown};
    let m = model(JUMP);
    let modes = [None, Some(Unknown), Some(Backend), Some(Assert), Some(Pop)];
    let outputs = modes.map(|mode| {
        let mut solver = FaultSolver::new(mode);
        let actual = block_on(prove_induction_invariants(&m, &mut solver, 1));
        let consumed = solver.consumed;
        let depth = solver.depth;
        let after = if mode.is_none() || consumed == 1 {
            solver.push();
            let target = solver.constant("n@ind1", &Sort::Int).unwrap();
            let outside = solver.equal(&target, &solver.int_value(99)).unwrap();
            solver.assert(&outside).unwrap();
            let after = block_on(solver.check());
            solver.pop(1).unwrap();
            Some(after)
        } else {
            None
        };
        let trace = solver.trace.clone();
        (mode, actual, consumed, depth, solver.depth, after, trace)
    });
    eprintln!("INDUCTION_TARGET_FAULTS whole={outputs:#?}");
    for (mode, actual, consumed, depth, final_depth, after, trace) in outputs {
        let expected = match mode {
            None => Ok(proof(&[("_bounds_n", 1), ("P", 1)])),
            Some(Unknown) => Err(VerifyError::solver("solver returned unknown in induction")),
            Some(Backend) => Err(VerifyError::solver("induction-target-check failure")),
            Some(Assert) => Err(VerifyError::solver("induction-target-assert failure")),
            Some(Pop) => Err(VerifyError::solver("induction-target-pop failure")),
        };
        assert_eq!(&actual, &expected);
        assert_eq!(&consumed, &usize::from(mode.is_some()));
        assert_eq!(&depth, &0);
        assert_eq!(&final_depth, &0);
        assert_eq!(&after, &Some(Ok(SatResult::Sat)));
        let target = trace
            .iter()
            .position(|event| event == "CONTEXT type_bound/_bounds_n depth=1")
            .expect("actual target bound context");
        if let Some(mode) = mode {
            let injection_depth = if mode == Pop { 1 } else { 2 };
            let event = format!("INJECT {mode:?} depth={injection_depth}");
            let injected = trace
                .iter()
                .position(|actual| actual == &event)
                .expect("actual target witness scope");
            assert!(target < injected);
            if mode == Pop {
                assert_eq!(trace[injected - 1], "POP levels=1 actual=Ok(()) depth=1");
            }
        }
    }
}
