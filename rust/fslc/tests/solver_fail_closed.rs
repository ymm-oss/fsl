// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};
use fsl_solver::{
    CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort,
    VerificationStatistics,
};

#[derive(Clone, Copy)]
enum InjectedCheck {
    Unknown,
    BackendError,
}

struct FirstCheckFault {
    inner: fsl_solver_z3::Z3Solver,
    first: Option<InjectedCheck>,
}

impl FirstCheckFault {
    fn new(first: InjectedCheck) -> Self {
        Self {
            inner: fsl_solver_z3::Z3Solver::new().expect("create Z3 solver"),
            first: Some(first),
        }
    }
}

impl SmtSolver for FirstCheckFault {
    type Term = <fsl_solver_z3::Z3Solver as SmtSolver>::Term;

    fn version(&self) -> &str {
        self.inner.version()
    }

    fn set_query_context(&mut self, kind: &str, name: &str) {
        self.inner.set_query_context(kind, name);
    }

    fn statistics(&self) -> VerificationStatistics {
        self.inner.statistics()
    }

    fn sort(&self, term: &Self::Term) -> Sort {
        self.inner.sort(term)
    }

    fn bool_value(&self, value: bool) -> Self::Term {
        self.inner.bool_value(value)
    }

    fn int_value(&self, value: i64) -> Self::Term {
        self.inner.int_value(value)
    }

    fn constant(&self, name: &str, sort: &Sort) -> SolverResult<Self::Term> {
        self.inner.constant(name, sort)
    }

    fn not(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.not(term)
    }

    fn and(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        self.inner.and(terms)
    }

    fn or(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        self.inner.or(terms)
    }

    fn implies(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.implies(left, right)
    }

    fn equal(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.equal(left, right)
    }

    fn ite(
        &self,
        condition: &Self::Term,
        then_term: &Self::Term,
        else_term: &Self::Term,
    ) -> SolverResult<Self::Term> {
        self.inner.ite(condition, then_term, else_term)
    }

    fn neg(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.neg(term)
    }

    fn add(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.add(left, right)
    }

    fn sub(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.sub(left, right)
    }

    fn mul(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.mul(left, right)
    }

    fn div(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.div(left, right)
    }

    fn modulo(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.modulo(left, right)
    }

    fn lt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.lt(left, right)
    }

    fn le(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.le(left, right)
    }

    fn gt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.gt(left, right)
    }

    fn ge(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.ge(left, right)
    }

    fn const_array(&self, domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.const_array(domain, value)
    }

    fn select(&self, array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.select(array, index)
    }

    fn store(
        &self,
        array: &Self::Term,
        index: &Self::Term,
        value: &Self::Term,
    ) -> SolverResult<Self::Term> {
        self.inner.store(array, index, value)
    }

    fn substitute(
        &self,
        term: &Self::Term,
        replacements: &[(Self::Term, Self::Term)],
    ) -> SolverResult<Self::Term> {
        self.inner.substitute(term, replacements)
    }

    fn push(&mut self) {
        self.inner.push();
    }

    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        self.inner.pop(levels)
    }

    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.inner.assert(term)
    }

    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        self.inner.assert_and_track(term, tracker)
    }

    fn check(&mut self) -> CheckFuture<'_> {
        match self.first.take() {
            Some(InjectedCheck::Unknown) => Box::pin(async { Ok(SatResult::Unknown) }),
            Some(InjectedCheck::BackendError) => {
                Box::pin(async { Err(SolverError::new("injected backend failure")) })
            }
            None => self.inner.check(),
        }
    }

    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        self.inner.check_assuming(assumptions)
    }

    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>> {
        self.inner.unsat_core()
    }

    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
        self.inner.model_eval(term)
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model() -> fsl_core::KernelModel {
    let source = r"
spec SolverFailClosed {
  type Small = 0..1
  state { x: Small }
  init { x = 0 }
  action stay() { x = x }
  invariant Safe { x <= 1 }
}
";
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse fail-closed fixture");
    build_model(kernel).expect("build fail-closed fixture")
}

#[test]
fn bmc_rejects_unknown_initial_solver_result() {
    let mut solver = FirstCheckFault::new(InjectedCheck::Unknown);
    let error = block_on(fsl_verifier::verify_bounded(&model(), &mut solver, 1))
        .expect_err("unknown must not become a clean BMC result");
    assert!(error.to_string().contains("unknown"));
}

#[test]
fn bmc_rejects_backend_failure() {
    let mut solver = FirstCheckFault::new(InjectedCheck::BackendError);
    let error = block_on(fsl_verifier::verify_bounded(&model(), &mut solver, 1))
        .expect_err("backend failure must not become a clean BMC result");
    assert!(error.to_string().contains("injected backend failure"));
}

#[test]
fn supplied_state_rejects_unknown_variables_before_solver_success() {
    let snapshot = BTreeMap::from([
        ("x".to_owned(), FslValue::Int(0)),
        ("not_state".to_owned(), FslValue::Int(0)),
    ]);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded_from_state(
        &model(),
        &mut solver,
        1,
        None,
        &snapshot,
    ))
    .expect_err("unknown supplied-state key must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown state variable 'not_state'")
    );
}

#[test]
fn action_partial_operation_checks_do_not_cross_the_requested_depth() {
    let source = r"
spec DelayedPartialBoundary {
  type Small = 0..1
  state { x: Small, quotient: Small }
  init { x = 0 quotient = 0 }
  action advance() { requires x == 0 x = 1 }
  action divide() { requires x == 1 quotient = 1 / 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse depth fixture");
    let model = build_model(kernel).expect("build depth fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify to exact requested depth");
    assert!(
        result.violation.is_none(),
        "the divide transition lands at step 2 and is outside depth 1: {result:?}"
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2))
        .expect("verify one step beyond the boundary");
    let violation = result
        .violation
        .expect("the divide transition must be checked when depth reaches 2");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.step, 2);
}

#[test]
fn selected_empty_bounds_really_skips_implicit_bound_properties() {
    let source = r"
spec SelectedBounds {
  type Small = 0..1
  state { x: Small }
  init { x = 0 }
  action stay() { x = x }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse bounds fixture");
    let model = build_model(kernel).expect("build bounds fixture");
    let snapshot = BTreeMap::from([("x".to_owned(), FslValue::Int(2))]);
    let selected = BTreeSet::new();
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded_from_state(
        &model,
        &mut solver,
        0,
        Some(&selected),
        &snapshot,
    ))
    .expect("verify with implicit bounds explicitly unselected");
    assert!(
        result.violation.is_none(),
        "an unselected implicit bound must not be evaluated: {result:?}"
    );
}

#[test]
fn transition_properties_are_checked_after_the_initial_state() {
    let source = r"
spec TransitionBoundary {
  type Small = 0..1
  state { x: Small }
  init { x = 0 }
  action advance() { requires x == 0 x = 1 }
  trans NeverIncrease { x <= old(x) }
}
";
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse transition fixture");
    let model = build_model(kernel).expect("build transition fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify transition boundary");
    let violation = result
        .violation
        .expect("transition violation at step 1 must be observed");
    assert_eq!(
        (violation.kind.as_str(), violation.name.as_str()),
        ("trans", "NeverIncrease")
    );
    assert_eq!(violation.step, 1);
}

#[test]
fn ensures_use_the_selected_action_and_pre_state_at_the_transition_step() {
    let source = r"
spec EnsuresBoundary {
  type Small = 0..1
  state { divisor: Small, quotient: Small }
  init { divisor = 1 quotient = 0 }
  action zero() {
    requires divisor == 1
    divisor = 0
    quotient = 1 / divisor
    ensures 1 / old(divisor) == 0
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse ensures fixture");
    let model = build_model(kernel).expect("build ensures fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let mut result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify ensures boundary");
    let violation = result
        .violation
        .as_ref()
        .expect("violated ensures must be attributed to the selected action");
    assert_eq!(
        (violation.kind.as_str(), violation.name.as_str()),
        ("ensures", "zero")
    );
    assert_eq!(violation.step, 1);

    result
        .violation
        .as_mut()
        .expect("violation still present")
        .trace
        .last_mut()
        .expect("post-state trace step")
        .state
        .insert("quotient".to_owned(), FslValue::Int(0));
    let replay_error = fslc_rust::verification_output::replay_bmc_witnesses(&model, &result, None)
        .expect_err("a corrupted symbolic witness must fail concrete replay");
    assert!(!replay_error.is_empty());
}
