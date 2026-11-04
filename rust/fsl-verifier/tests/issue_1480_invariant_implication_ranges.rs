// SPDX-License-Identifier: Apache-2.0
//! Every implication-side range uses its own model's established state carrier.
use fsl_core::{FsResolver, FslValue as V, KernelModel, build_model, parse_kernel_source};
use fsl_verifier::{ImplicationResult, VerifyError, invariant_implication};
use std::collections::BTreeMap;
const LITERAL: &str = "spec Implication {state {n:0..2} init {n=0} action idle(){} INVARIANTS}";
const NAMED: &str =
    "spec Nominal {type K=0..MAX state {n:K} init {n=0} action idle(){} INVARIANTS}";
const COUNT: &str = "invariant P {count(i in 0..n)==n+1}";
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
fn model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("IMPLICATION_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("IMPLICATION_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn observe(antecedent: &str, consequent: &str) -> Result<ImplicationResult, VerifyError> {
    let antecedent = model(antecedent);
    let consequent = model(consequent);
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual = block_on(invariant_implication(&antecedent, &consequent, &mut solver));
    eprintln!("IMPLICATION_RESULT actual={actual:#?}");
    actual
}
#[test]
fn literal_carrier_authorizes_consequent_and_antecedent_ranges_independently() {
    let outputs = [
        ("", COUNT),
        ("", "invariant P {n!=2 or count(i in 0..n)==n}"),
        (COUNT, "invariant Q {true}"),
    ]
    .map(|(antecedent, consequent)| {
        observe(
            &LITERAL.replace("INVARIANTS", antecedent),
            &LITERAL.replace("INVARIANTS", consequent),
        )
    });
    assert_eq!(
        outputs,
        [
            Ok(ImplicationResult::Implied),
            Ok(ImplicationResult::Counterexample(BTreeMap::from([(
                "n".into(),
                V::Int(2)
            )]))),
            Ok(ImplicationResult::Implied),
        ]
    );
    assert_ne!(outputs[1], Ok(ImplicationResult::Implied));
    assert_ne!(
        outputs[1],
        Ok(ImplicationResult::Counterexample(BTreeMap::from([(
            "n".into(),
            V::Int(1)
        )])))
    );
}
#[test]
fn identical_nominal_names_do_not_prove_different_model_carriers() {
    let outputs = [(3, 2), (2, 3)].map(|(antecedent_max, consequent_max)| {
        let antecedent = NAMED
            .replace("MAX", &antecedent_max.to_string())
            .replace("INVARIANTS", COUNT);
        let consequent = NAMED
            .replace("MAX", &consequent_max.to_string())
            .replace("INVARIANTS", "invariant Q {true}");
        observe(&antecedent, &consequent)
    });
    // Both schemas literally contain Named(K). But n<=2 entails the FIRST
    // model's n<=3 carrier; n<=3 does NOT entail the other's n<=2 carrier.
    assert_eq!(
        outputs,
        [
            Ok(ImplicationResult::Implied),
            Err(VerifyError::new(ELIGIBILITY)),
        ]
    );
    assert_ne!(outputs[1], Ok(ImplicationResult::Implied));
}
#[test]
fn actually_different_literal_state_schemas_preserve_the_existing_rejection() {
    let antecedent = LITERAL.replace("INVARIANTS", "");
    let consequent = antecedent.replace("n:0..2", "n:0..3");
    let actual = observe(&antecedent, &consequent);
    assert_eq!(
        actual,
        Err(VerifyError::new("invariant state schemas differ"))
    );
    assert_ne!(actual, Ok(ImplicationResult::Implied));
}
