// SPDX-License-Identifier: Apache-2.0
//! Independent fresh-frame composite-range vacuity controls; no BMC-ledger reuse.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, PartialOperation, Span, TraceStep, build_model,
    parse_kernel_source,
};
use fsl_solver::{SatResult, SmtSolver, Sort};
use fsl_syntax::SourcePos;
#[path = "support/composite_vacuity_fault.rs"]
mod fault;
use fault::{Fault, FaultSolver};
use fsl_verifier::{
    BmcResult, BmcViolation, VacuityFinding, VerifyError, model_vacuity_findings,
    verify_bounded_from_state,
};
use std::collections::BTreeMap;

const W: &str = "spec A { type K=0..2 state { n:K } init { n=0 } action go() {} invariant P { n!=0 or (forall i in 0..n { i==0 }) } }";
const PARTIAL: &str = "spec Mutable { type K=0..2 state {n:K,flag:Bool,q:Seq<K,1>} init {n=2 flag=true q=Seq {0}} action idle(){q=q} invariant P {flag and (forall i in 0..n {q.head()==q.head()})} }";
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
fn checked(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("COMPOSITE_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("COMPOSITE_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn equal<T: std::fmt::Debug + PartialEq>(actual: &T, expected: &T) {
    eprintln!("COMPOSITE_EQUAL actual={actual:#?} expected={expected:#?}");
    assert_eq!(actual, expected);
}
fn span(start: usize) -> Span {
    Span {
        start: SourcePos {
            offset: start,
            line: 1,
            column: u32::try_from(start + 1).unwrap(),
        },
        end: SourcePos {
            offset: start + 9,
            line: 1,
            column: u32::try_from(start + 10).unwrap(),
        },
    }
}
fn finding() -> VacuityFinding {
    VacuityFinding::TautologyOverFrozen {
        invariant: "P".into(),
        span: span(63),
        frozen_vars: vec!["n".into()],
    }
}
fn coverage(action: &str) -> BTreeMap<String, bool> {
    BTreeMap::from([(action.into(), true)])
}
fn result(
    spec: &str,
    action: &str,
    violation: Option<BmcViolation>,
    vacuity: Vec<VacuityFinding>,
) -> BmcResult {
    BmcResult {
        spec: spec.into(),
        depth: 0,
        action_coverage: BTreeMap::from([(action.into(), violation.is_none())]),
        violation,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        frontier_progress: false,
        vacuity,
    }
}
#[test]
fn composite_frozen_literals_have_exact_findings_and_whole_bmc_results() {
    let predicates = [
        "n!=0 or (forall i in 0..n { i==0 })",
        "true and (forall i in 0..n {forall j in 0..n {i==0 and j==0}})",
        "count(i in 0..n)==1 and n==0",
        "sum(i in 0..n of i)==0 and n==0",
    ];
    let outputs=predicates.map(|predicate| {
        let source=W.replace("n!=0 or (forall i in 0..n { i==0 })",predicate);
        let m=checked(&source);
        equal(&m.invariants[0].span,&span(63));
        let mut solver=fsl_solver_z3::Z3Solver::new().unwrap();
        let diagnostic=block_on(model_vacuity_findings(&m,&mut solver,&coverage("go")));
        let mut solver=fsl_solver_z3::Z3Solver::new().unwrap();
        let bounded=block_on(verify_bounded_from_state(&m,&mut solver,0,None,&BTreeMap::from([("n".into(),V::Int(0))])));
        eprintln!("COMPOSITE_POSITIVE predicate={predicate:?} diagnostic={diagnostic:#?} bounded={bounded:#?}");
        (diagnostic,bounded)
    });
    for (diagnostic, bounded) in outputs {
        equal(&diagnostic, &Ok(vec![finding()]));
        equal(&bounded, &Ok(result("A", "go", None, vec![finding()])));
    }
}
#[test]
fn opposite_properties_keep_exact_ordinary_invariant_witnesses() {
    for predicate in [
        "n!=0 or (forall i in 0..n { i!=0 })",
        "true and (forall i in 0..n {forall j in 0..n {i==0 and j!=0}})",
        "count(i in 0..n)==0 and n==0",
        "sum(i in 0..n of i)==1 and n==0",
    ] {
        let m = checked(&W.replace("n!=0 or (forall i in 0..n { i==0 })", predicate));
        let state = BTreeMap::from([("n".into(), V::Int(0))]);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(verify_bounded_from_state(&m, &mut solver, 0, None, &state));
        let violation = BmcViolation {
            kind: "invariant".into(),
            name: "P".into(),
            step: 0,
            last_action: None,
            trace: vec![TraceStep {
                step: 0,
                state,
                action: None,
                changes: BTreeMap::new(),
            }],
            leads_to: None,
        };
        equal(&actual, &Ok(result("A", "go", Some(violation), vec![])));
    }
}
#[test]
fn mutable_empty_sequence_blocks_a_composite_frozen_tautology() {
    let m = checked(PARTIAL);
    equal(&m.invariants[0].span, &span(110));
    let mut state = BTreeMap::from([
        ("n".into(), V::Int(2)),
        ("flag".into(), V::Bool(true)),
        ("q".into(), V::Seq(vec![V::Int(0)])),
    ]);
    equal(&fsl_runtime::Monitor::new(m.clone()).unwrap().state, &state);
    equal(
        &fsl_runtime::eval(
            &m.invariants[0].expr,
            &state,
            &mut BTreeMap::new(),
            &m,
            None,
        ),
        &Ok(V::Bool(true)),
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let diagnostic = block_on(model_vacuity_findings(&m, &mut solver, &coverage("idle")));
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let bounded = block_on(verify_bounded_from_state(&m, &mut solver, 0, None, &state));
    eprintln!("COMPOSITE_ABSTRACT_NEGATIVE diagnostic={diagnostic:#?} bounded={bounded:#?}");
    equal(&diagnostic, &Ok(vec![]));
    equal(&bounded, &Ok(result("Mutable", "idle", None, vec![])));
    state.insert("q".into(), V::Seq(vec![]));
    for (name, ty) in &m.state {
        equal(
            &fsl_runtime::value_conforms(&state[name], ty, &m),
            &Ok(true),
        );
    }
    equal(
        &fsl_runtime::eval(
            &m.invariants[0].expr,
            &state,
            &mut BTreeMap::new(),
            &m,
            None,
        ),
        &Err(fsl_runtime::RuntimeError {
            message: "head() on empty sequence".into(),
            span: None,
            partial_operation: Some(PartialOperation::Head),
            map_key_outside_domain: false,
        }),
    );
}
#[test]
fn supported_static_and_root_range_findings_are_preserved() {
    for predicate in ["n==0", "forall i in 0..n {i==0}"] {
        let m = checked(&W.replace("n!=0 or (forall i in 0..n { i==0 })", predicate));
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        equal(
            &block_on(model_vacuity_findings(&m, &mut solver, &coverage("go"))),
            &Ok(vec![finding()]),
        );
    }
}
#[test]
fn two_property_queries_and_a_following_query_do_not_leak_premises() {
    let pair = checked(
        "spec A { type K=0..2 state { n:K } init { n=0 } action go() {} invariant P { n!=0 or (forall i in 0..n { i==0 }) } invariant Q { n!=0 or (forall i in 0..n { i!=0 }) } }",
    );
    equal(&pair.invariants[1].span, &span(115));
    let opposite = checked(&W.replace("i==0", "i!=0"));
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let first = block_on(model_vacuity_findings(&pair, &mut solver, &coverage("go")));
    let second = block_on(model_vacuity_findings(
        &opposite,
        &mut solver,
        &coverage("go"),
    ));
    // Actual existing diagnostic symbol spelling (value.rs), not a new test seam.
    let unrelated = solver.constant("n@vac_frozen", &Sort::Int).unwrap();
    solver.push();
    solver
        .assert(&solver.equal(&unrelated, &solver.int_value(2)).unwrap())
        .unwrap();
    let after = block_on(solver.check());
    solver.pop(1).unwrap();
    eprintln!("COMPOSITE_QUERY_SEQUENCE first={first:#?} second={second:#?} post_pop={after:?}");
    equal(&first, &Ok(vec![finding()]));
    equal(&second, &Ok(vec![]));
    equal(&after, &Ok(SatResult::Sat));
    equal(&block_on(solver.check()), &Ok(SatResult::Sat));
}

#[test]
fn no_range_partial_reads_do_not_establish_frozen_tautologies() {
    let source = "spec Legacy { state {flag:Bool,q:Seq<Int,1>} init {flag=true q=Seq {0}} action idle(){q=q} invariant P {flag and (q.head()==q.head())} }";
    for (predicate, expected) in [("q.head()==q.head()", vec![]), ("q.head()==0", vec![])] {
        let m = checked(&source.replace("q.head()==q.head()", predicate));
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        equal(
            &block_on(model_vacuity_findings(&m, &mut solver, &coverage("idle"))),
            &Ok(expected),
        );
    }
}
#[test]
fn composite_diagnostic_faults_are_consumed_and_keep_typed_outcomes() {
    let m = checked(W);
    let faults = [Fault::Unknown, Fault::Backend, Fault::Assert, Fault::Pop];
    let outputs = faults.map(|fault| {
        let mut solver = FaultSolver::new(fault);
        let actual = block_on(model_vacuity_findings(&m, &mut solver, &coverage("go")));
        let consumed = solver.consumed;
        let depth = solver.depth;
        let after = if consumed == 1 { Some(block_on(solver.check())) } else { None };
        eprintln!(
            "COMPOSITE_FAULT fault={fault:?} actual={actual:#?} consumed={consumed} depth={depth} after={after:?} trace={:#?}",
            solver.trace
        );
        let expected = match fault {
            Fault::Unknown => Ok(vec![]),
            Fault::Backend => Err(VerifyError::solver("composite-check failure")),
            Fault::Assert => Err(VerifyError::solver("composite-assert failure")),
            Fault::Pop => Err(VerifyError::solver("composite-pop failure")),
        };
        (actual, expected, consumed, depth, after)
    });
    for (actual, expected, consumed, depth, after) in outputs {
        equal(&consumed, &1);
        equal(&depth, &0);
        equal(&actual, &expected);
        equal(&after, &Some(Ok(SatResult::Sat)));
    }
}
