// SPDX-License-Identifier: Apache-2.0
//! A frozen-value diagnostic must prove definedness as well as truth.
use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_verifier::{BmcResult, VacuityFinding, model_vacuity_findings, verify_bounded};
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
fn check(expression: &str, warning: bool) {
    let source = format!(
        "spec FrozenTotality {{\n state {{flag:Bool,q:Seq<Int,1>}}\n init {{flag=true q=Seq {{0}}}}\n action idle() {{q=q}}\n invariant P {{{expression}}}\n}}"
    );
    let model = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
    let expected = if warning {
        vec![VacuityFinding::TautologyOverFrozen {
            invariant: "P".into(),
            span: model.invariants[0].span,
            frozen_vars: vec!["flag".into()],
        }]
    } else {
        vec![]
    };
    let coverage = BTreeMap::from([("idle".into(), true)]);
    let mut diagnostic_solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let direct = block_on(model_vacuity_findings(
        &model,
        &mut diagnostic_solver,
        &coverage,
    ));
    let mut base_solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let bounded = block_on(verify_bounded(&model, &mut base_solver, 1));
    let expected_base = BmcResult {
        spec: "FrozenTotality".into(),
        depth: 1,
        violation: None,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: coverage,
        frontier_progress: false,
        vacuity: expected.clone(),
    };
    eprintln!("FROZEN_TOTALITY expression={expression:?} direct={direct:#?} bounded={bounded:#?}");
    assert_eq!(direct, Ok(expected));
    assert_eq!(bounded, Ok(expected_base));
}
#[test]
fn possible_undefined_mutable_read_is_not_a_frozen_tautology() {
    // q=[0] in reachable states, but q=[] belongs to its declared type space.
    // A warning claims all dynamics, so totalized head()==head() is insufficient.
    check("flag and (q.head()==q.head())", false);
}
#[test]
fn defined_identity_remains_a_genuine_frozen_tautology() {
    check("flag and (q==q)", true);
}
#[test]
fn an_unreached_partial_read_does_not_suppress_a_genuine_tautology() {
    check("flag or (q.head()==q.head())", true);
}
