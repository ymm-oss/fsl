// SPDX-License-Identifier: Apache-2.0
//! Expression agreement proves referenced bounds without admitting the whole state.
use fsl_core::{FsResolver, FslValue as V, KernelModel, build_model, parse_kernel_source};
use fsl_verifier::{VerifyError, expression_matches_value};
use std::collections::BTreeMap;
const SOURCE: &str = "spec Expression {state {n:0..2,q:0..2} init {n=0 q=0} action idle(){}}";
const FORALL: &str = "forall i in 0..n {i<=2}";
const COUNT: &str = "count(i in 0..n)";
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
fn model() -> KernelModel {
    let parsed = parse_kernel_source(SOURCE, &FsResolver::new("."));
    eprintln!("EXPRESSION_SOURCE source={SOURCE:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("EXPRESSION_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn state(n: i64, q: i64) -> BTreeMap<String, V> {
    BTreeMap::from([("n".into(), V::Int(n)), ("q".into(), V::Int(q))])
}
fn observe(
    model: &KernelModel,
    query: &str,
    state: &BTreeMap<String, V>,
    expected: &V,
) -> Result<bool, VerifyError> {
    let expression = fsl_syntax::parse_expr(query);
    eprintln!("EXPRESSION_RAW query={query:?} parsed={expression:#?}");
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(expression_matches_value(
        model,
        &mut solver,
        &expression.unwrap(),
        state,
        expected,
    ));
    eprintln!(
        "EXPRESSION_RESULT query={query:?} state={state:#?} expected={expected:#?} actual={actual:#?}"
    );
    actual
}
#[test]
fn valid_referenced_endpoints_compare_full_boolean_and_count_values() {
    let model = model();
    let outputs = [
        (FORALL, 0, V::Bool(true)),
        (FORALL, 0, V::Bool(false)),
        (FORALL, 2, V::Bool(true)),
        (FORALL, 2, V::Bool(false)),
        (COUNT, 2, V::Int(3)),
        (COUNT, 2, V::Int(2)),
    ]
    .map(|(query, n, expected)| observe(&model, query, &state(n, 0), &expected));
    assert_eq!(
        outputs,
        [
            Ok(true),
            Ok(false),
            Ok(true),
            Ok(false),
            Ok(true),
            Ok(false)
        ]
    );
}
#[test]
fn invalid_referenced_endpoint_never_becomes_an_eligible_comparison() {
    let model = model();
    let outputs = [
        (FORALL, V::Bool(true)),
        (FORALL, V::Bool(false)),
        (COUNT, V::Int(4)),
        (COUNT, V::Int(3)),
    ]
    .map(|(query, expected)| observe(&model, query, &state(3, 0), &expected));
    for actual in outputs {
        assert_eq!(actual, Err(VerifyError::new(ELIGIBILITY)));
        assert_ne!(actual, Ok(true));
        assert_ne!(actual, Ok(false));
    }
}
#[test]
fn unrelated_invalid_state_does_not_change_expression_locality_or_plain_comparison() {
    let model = model();
    let current = state(2, 3);
    let outputs = [
        (FORALL, V::Bool(true)),
        (FORALL, V::Bool(false)),
        (COUNT, V::Int(3)),
        (COUNT, V::Int(2)),
        ("q==3", V::Bool(true)),
        ("q==3", V::Bool(false)),
    ]
    .map(|(query, expected)| observe(&model, query, &current, &expected));
    assert_eq!(
        outputs,
        [
            Ok(true),
            Ok(false),
            Ok(true),
            Ok(false),
            Ok(true),
            Ok(false)
        ]
    );
    // Referenced n is valid. Unrelated q3 remains unvalidated, including the
    // existing plain q==3 path; no whole-state or BMC assurance is implied.
}
#[test]
fn pinning_unknown_identifier_and_value_type_errors_keep_their_exact_contract() {
    let model = model();
    let missing = BTreeMap::from([("q".into(), V::Int(0))]);
    let mut extra = state(2, 0);
    extra.insert("extra".into(), V::Int(0));
    let wrong_type = BTreeMap::from([("n".into(), V::Bool(true)), ("q".into(), V::Int(0))]);
    let outputs = [
        observe(&model, "true", &missing, &V::Bool(true)),
        observe(&model, "true", &extra, &V::Bool(true)),
        observe(&model, "true", &wrong_type, &V::Bool(true)),
        observe(&model, FORALL, &state(2, 0), &V::Int(1)),
        observe(&model, "missing", &state(2, 0), &V::Int(0)),
    ];
    assert_eq!(
        outputs,
        [
            Err(VerifyError::new("agreement state is missing 'n'")),
            Err(VerifyError::new(
                "agreement state contains an unknown state variable"
            )),
            Err(VerifyError::new(
                "concrete value Bool(true) does not conform to Range(0, 2)"
            )),
            Err(VerifyError::new(
                "concrete value Int(1) does not conform to Bool"
            )),
            Err(VerifyError::new("unknown identifier 'missing'")),
        ]
    );
}
