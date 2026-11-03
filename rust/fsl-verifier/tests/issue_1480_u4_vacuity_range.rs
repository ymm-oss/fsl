// SPDX-License-Identifier: Apache-2.0

//! Future U4 acceptance: fresh vacuity frames need their own range proof.
//! No current-2e result or fake/unknown solver establishes this contract.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, PartialOperation, Span, TypeRef, build_model,
    parse_kernel_source,
};
use fsl_syntax::SourcePos;
use fsl_verifier::{BmcResult, VacuityFinding, model_vacuity_findings, verify_bounded_from_state};
use std::collections::BTreeMap;
use std::fmt::Debug;

const FROZEN: &str = "spec U4 { type K=0..2 state {n:K} init {n=1} action idle(){} invariant Q {forall i in 0..n {i<=2}} }";
const PARTIAL: &str = "spec U4Partial { type K=0..2 state {n:K, flag:Bool, q:Seq<K,1>} init {n=2 flag=true q=Seq {0}} action idle(){q=q} invariant Q {forall i in 0..n {flag and (q.head()==q.head())}} }";

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
fn equal<T: Debug + PartialEq>(label: &str, actual: &T, expected: &T) {
    eprintln!("{label}\nactual={actual:#?}\nexpected={expected:#?}");
    assert_eq!(actual, expected, "{label}");
}
fn checked(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("provider=FsResolver(.) source={source:?}\nparsed={parsed:#?}");
    let actual = build_model(parsed.expect("literal source parses"));
    eprintln!("checked-model={actual:#?}");
    actual.expect("literal source checks; no unchecked-model fallback")
}
fn q_span(start: usize, end: usize) -> Span {
    // Independently counted invariant-keyword offsets, never model-derived.
    Span {
        start: SourcePos {
            offset: start,
            line: 1,
            column: u32::try_from(start + 1).unwrap(),
        },
        end: SourcePos {
            offset: end,
            line: 1,
            column: u32::try_from(end + 1).unwrap(),
        },
    }
}
fn success(spec: &str, vacuity: Vec<VacuityFinding>) -> BmcResult {
    BmcResult {
        spec: spec.into(),
        depth: 0,
        violation: None,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: BTreeMap::from([("idle".into(), true)]),
        frontier_progress: false,
        vacuity,
    }
}
fn verify(
    model: &KernelModel,
    root: &BTreeMap<String, V>,
    findings: Vec<VacuityFinding>,
    name: &str,
) {
    eprintln!("root-provider=verify_bounded_from_state checked_bounds=None depth=0 root={root:#?}");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("native Z3");
    equal(
        "whole bounded result",
        &block_on(verify_bounded_from_state(model, &mut solver, 0, None, root)),
        &Ok(success(name, findings.clone())),
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("fresh diagnostic Z3");
    equal(
        "whole fresh public diagnostic result",
        &block_on(model_vacuity_findings(
            model,
            &mut solver,
            &BTreeMap::from([("idle".into(), true)]),
        )),
        &Ok(findings),
    );
}

#[test]
fn frozen_root_range_keeps_its_exact_fresh_frame_tautology_finding() {
    let model = checked(FROZEN);
    equal("spec identity", &model.name.as_str(), &"U4");
    equal(
        "declared state",
        &model.state.clone(),
        &vec![("n".into(), TypeRef::Named("K".into()))],
    );
    equal(
        "literal property span",
        &model.invariants[0].span,
        &q_span(61, 70),
    );
    // Preserve the failed public cohort: source init is1, actual BMC root is2.
    let root = BTreeMap::from([("n".into(), V::Int(2))]);
    equal(
        "real root members",
        &fsl_runtime::eval(
            &model.invariants[0].expr,
            &root,
            &mut BTreeMap::new(),
            &model,
            None,
        ),
        &Ok(V::Bool(true)),
    );
    let finding = VacuityFinding::TautologyOverFrozen {
        invariant: "Q".into(),
        span: q_span(61, 70),
        frozen_vars: vec!["n".into()],
    };
    verify(&model, &root, vec![finding], "U4");
}

#[test]
fn abstract_empty_mutable_sequence_prevents_a_manufactured_frozen_tautology() {
    let model = checked(PARTIAL);
    equal("spec identity", &model.name.as_str(), &"U4Partial");
    equal(
        "declared state",
        &model.state.clone(),
        &vec![
            ("n".into(), TypeRef::Named("K".into())),
            ("flag".into(), TypeRef::Bool),
            (
                "q".into(),
                TypeRef::Seq(Box::new(TypeRef::Named("K".into())), 1),
            ),
        ],
    );
    equal(
        "literal property span",
        &model.invariants[0].span,
        &q_span(114, 123),
    );
    let root = BTreeMap::from([
        ("n".into(), V::Int(2)),
        ("flag".into(), V::Bool(true)),
        ("q".into(), V::Seq(vec![V::Int(0)])),
    ]);
    let monitor = fsl_runtime::Monitor::new(model.clone());
    eprintln!("reachable init monitor={monitor:#?}");
    equal(
        "literal reachable init",
        &monitor.expect("source init").state,
        &root.clone(),
    );
    equal(
        "reachable property",
        &fsl_runtime::eval(
            &model.invariants[0].expr,
            &root,
            &mut BTreeMap::new(),
            &model,
            None,
        ),
        &Ok(V::Bool(true)),
    );
    verify(&model, &root, vec![], "U4Partial");
    // q=q is a real write: q is abstractly mutable even though init/idle keep[0].
    let mut abstract_root = root;
    abstract_root.insert("q".into(), V::Seq(vec![]));
    for (name, ty) in &model.state {
        equal(
            "abstract empty-q state stays type-valid",
            &fsl_runtime::value_conforms(&abstract_root[name], ty, &model),
            &Ok(true),
        );
    }
    equal(
        "genuine abstract partial operation",
        &fsl_runtime::eval(
            &model.invariants[0].expr,
            &abstract_root,
            &mut BTreeMap::new(),
            &model,
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
