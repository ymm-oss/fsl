// SPDX-License-Identifier: Apache-2.0
//! Init prefix facts preserve relational constraints and concrete assigned values.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, TraceStep, build_model, parse_kernel_source,
};
use fsl_verifier::{BmcResult, BmcViolation, ReachableWitness, VerifyError, verify_bounded};
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
fn source(initial: &str) -> String {
    format!(
        "spec Init {{\n  const C=2\n  type K=0..2\n  state {{n:K,q:Bool}}\n  init {{\n    {initial}\n  }}\n  action go() {{}}\n  reachable R {{true}}\n}}"
    )
}
fn model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("INIT_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("INIT_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn run(initial: &str) -> Result<BmcResult, VerifyError> {
    let model = model(&source(initial));
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(verify_bounded(&model, &mut solver, 0));
    eprintln!("INIT_BOUNDED actual={actual:#?}");
    actual
}
fn snapshot(n: i64, q: bool) -> TraceStep {
    TraceStep {
        step: 0,
        state: BTreeMap::from([("n".into(), V::Int(n)), ("q".into(), V::Bool(q))]),
        action: None,
        changes: BTreeMap::new(),
    }
}
fn initial(n: i64, q: bool) -> BmcResult {
    BmcResult {
        spec: "Init".into(),
        depth: 0,
        violation: None,
        leadsto_violation: None,
        reachables: BTreeMap::from([(
            "R".into(),
            Some(ReachableWitness {
                step: 0,
                trace: vec![snapshot(n, q)],
            }),
        )]),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: BTreeMap::from([("go".into(), true)]),
        frontier_progress: false,
        vacuity: vec![],
    }
}
#[test]
fn const_arithmetic_shadow_and_branch_prefixes_keep_the_actual_init_values() {
    let outputs = [
        ("n=C q=count(i in 0..n)==3", 2, true),
        ("n=1+1 q=sum(i in 0..n of i)==3", 2, true),
        ("n=2 q=exists n in 0..0 {count(i in 0..n)==3}", 2, false),
        (
            "if true {n=2 q=exists i in 0..n {i==2}} else {n=0 q=false}",
            2,
            true,
        ),
        ("if true {n=2} else {n=0} q=count(i in 0..n)==3", 2, true),
        ("if false {n=2} else {n=0} q=count(i in 0..n)==1", 0, true),
        ("if false {n=3} else {n=0} q=count(i in 0..n)==1", 0, true),
        ("n=if false then 3 else 0 q=count(i in 0..n)==1", 0, true),
        ("n=0\n    q=forall i in (n+1)..n {1/0>0}", 0, true),
    ]
    .map(|(init, n, q)| (run(init), n, q));
    for (actual, n, q) in outputs {
        assert_eq!(actual, Ok(initial(n, q)));
        assert_ne!(actual, Ok(initial(n, !q)));
    }
}
#[test]
fn scalar_outside_carrier_keeps_the_full_initial_bound_violation() {
    // Deliberately no dynamic RHS: that invalid-carrier priority remains separate.
    let actual = run("n=3 q=true");
    let mut expected = initial(3, true);
    expected.reachables.insert("R".into(), None);
    expected.action_coverage.insert("go".into(), false);
    expected.violation = Some(BmcViolation {
        kind: "type_bound".into(),
        name: "_bounds_n".into(),
        step: 0,
        last_action: None,
        trace: vec![snapshot(3, true)],
        leads_to: None,
    });
    assert_eq!(actual, Ok(expected));
    assert_ne!(actual, Ok(initial(3, true)));
}
#[test]
fn relational_conflict_and_unestablished_prefixes_keep_their_public_api_errors() {
    // Public Kernel BMC contracts; these do not claim strict CLI admission.
    let outputs=[
        ("n=0 n=1 q=true","init constraints are unsatisfiable"),
        ("n=3 q=count(i in 0..n)==4","unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope"),
        ("q=count(i in 0..n)==1 n=0","unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope"),
        ("if false {n=2 q=true} else {q=count(i in 0..n)==1 n=0}","unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope"),
    ].map(|(init,message)|(run(init),message));
    for (actual, message) in outputs {
        assert_eq!(actual, Err(VerifyError::new(message)));
        assert_ne!(actual, Err(VerifyError::solver(message)));
    }
}
#[test]
fn reached_init_division_keeps_its_located_semantic_error() {
    let actual = run("n=0\n    q=exists i in 0..n {1/0==0}");
    assert_eq!(
        actual,
        Err(VerifyError::new("division by zero in init at 7:5"))
    );
    assert_ne!(actual, Ok(initial(0, true)));
}
#[test]
fn assigned_state_still_cannot_supply_an_init_statement_range() {
    let model = model(
        "spec Admission {type K=0..2 state {n:K,m:Map<K,Bool>} init {n=2 forall i in 0..n {m[i]=true}} action go(){}}",
    );
    let actual = fsl_runtime::deterministic_initial_state(&model);
    eprintln!("INIT_STATEMENT_ADMISSION actual={actual:#?}");
    assert_eq!(actual,Err(fsl_runtime::RuntimeError {
        message:"init forall range bounds must be compile-time constants; state variable 'n' is not allowed".into(),
        span:None,partial_operation:None,map_key_outside_domain:false,
    }));
}
