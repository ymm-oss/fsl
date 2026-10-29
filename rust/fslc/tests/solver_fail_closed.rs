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

/// Injects one fault at the `fault_at`-th satisfiability check (`check` or
/// `check_assuming`, 1-based) and counts every check it sees.
struct FirstCheckFault {
    inner: fsl_solver_z3::Z3Solver,
    first: Option<InjectedCheck>,
    fault_at: usize,
    checks: usize,
    /// The 1-based model read that answers "unavailable" (`None`), if any.
    no_model_at: Option<usize>,
    model_reads: std::cell::Cell<usize>,
}

impl FirstCheckFault {
    fn new(first: InjectedCheck) -> Self {
        Self::at(first, 1)
    }

    fn at(fault: InjectedCheck, fault_at: usize) -> Self {
        Self {
            inner: fsl_solver_z3::Z3Solver::new().expect("create Z3 solver"),
            first: Some(fault),
            fault_at,
            checks: 0,
            no_model_at: None,
            model_reads: std::cell::Cell::new(0),
        }
    }

    fn counting() -> Self {
        Self {
            inner: fsl_solver_z3::Z3Solver::new().expect("create Z3 solver"),
            first: None,
            fault_at: 0,
            checks: 0,
            no_model_at: None,
            model_reads: std::cell::Cell::new(0),
        }
    }

    fn injected(&mut self) -> Option<InjectedCheck> {
        self.checks += 1;
        if self.checks == self.fault_at {
            self.first.take()
        } else {
            None
        }
    }
}

fn injected_check(fault: InjectedCheck) -> CheckFuture<'static> {
    match fault {
        InjectedCheck::Unknown => Box::pin(async { Ok(SatResult::Unknown) }),
        InjectedCheck::BackendError => {
            Box::pin(async { Err(SolverError::new("injected backend failure")) })
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

    fn literal_value(&self, term: &Self::Term) -> Option<ModelValue> {
        self.inner.literal_value(term)
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

    fn reset(&mut self) -> SolverResult<()> {
        self.inner.reset()
    }

    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.inner.assert(term)
    }

    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        self.inner.assert_and_track(term, tracker)
    }

    fn check(&mut self) -> CheckFuture<'_> {
        match self.injected() {
            Some(fault) => injected_check(fault),
            None => self.inner.check(),
        }
    }

    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        match self.injected() {
            Some(fault) => injected_check(fault),
            None => self.inner.check_assuming(assumptions),
        }
    }

    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>> {
        self.inner.unsat_core()
    }

    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
        let read = self.model_reads.get() + 1;
        self.model_reads.set(read);
        if self.no_model_at == Some(read) {
            return Ok(None);
        }
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
    // #1251: `fslc mutate` must be able to tell this apart from a finding.
    assert!(error.is_solver_failure(), "{error}");
}

#[test]
fn bmc_rejects_backend_failure() {
    let mut solver = FirstCheckFault::new(InjectedCheck::BackendError);
    let error = block_on(fsl_verifier::verify_bounded(&model(), &mut solver, 1))
        .expect_err("backend failure must not become a clean BMC result");
    assert!(error.to_string().contains("injected backend failure"));
    assert!(error.is_solver_failure(), "{error}");
}

/// #1251 control: a semantic BMC error (an action body undefined in a
/// reachable state) is a finding about the model, not a solver failure, so
/// `fslc mutate` keeps counting it as the `build_spec` kill.
#[test]
fn bmc_semantic_error_is_not_a_solver_failure() {
    let source = r"
spec UnboundedInitDefinedness {
  type Amount = 0..3
  state { balance: Int }
  init { }
  action deposit(a: Amount) { requires a > 0 balance = balance + a }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse fixture");
    let model = build_model(kernel).expect("build fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 4))
        .expect_err("an undefined action body is a BMC error");
    assert!(!error.is_solver_failure(), "{error}");
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
fn terminal_partial_operations_are_checked_only_at_deadlock() {
    let source = r"
spec ShortCircuitedPropertyBoundaries {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  invariant SafeInvariant { true or queue.head() >= 0 }
  reachable SafeReachable { true or queue.head() >= 0 }
  leadsTo SafeLiveness {
    true or queue.head() >= 0 ~> true or queue.head() >= 0
  }
  trans SafeTransition { true or queue.head() >= 0 }
  terminal { queue.head() >= 0 }
}
";
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse live terminal fixture");
    let model = build_model(kernel).expect("build live terminal fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify an always-enabled transition");
    assert!(
        result.violation.is_none(),
        "a terminal expression is not evaluated while an action remains enabled: {result:?}"
    );
    assert!(result.leadsto_violation.is_none(), "{result:?}");
    assert!(
        result.reachables["SafeReachable"].is_some(),
        "the short-circuited reachable must still be witnessed: {result:?}"
    );
}

#[test]
fn pending_reachable_partial_operation_is_not_skipped() {
    let source = r"
spec PendingReachableBoundary {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  reachable PartialReachable { queue.head() >= 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new("."))
        .expect("parse pending reachable fixture");
    let model = build_model(kernel).expect("build pending reachable fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify pending reachable definedness");
    let violation = result
        .violation
        .expect("a pending reachable must be checked for partial operations");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.name, "_partial_property_PartialReachable");
    assert_eq!(
        violation.step, 0,
        "definedness must be checked while the target is pending, before raw witness recording"
    );
}

#[test]
fn transition_definedness_reads_the_previous_state() {
    let source = r"
spec TransitionOldStateBoundary {
  type Item = 0..1
  state { queue: Seq<Item, 1> }
  init { queue = Seq { 0 } }
  action clear() { queue = queue.pop() }
  trans OldHeadWasDefined { old(queue).head() >= 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new("."))
        .expect("parse transition old-state fixture");
    let model = build_model(kernel).expect("build transition old-state fixture");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 1))
        .expect("verify transition old-state definedness");
    assert!(
        result.violation.is_none(),
        "old(queue) was non-empty even though the current queue is empty: {result:?}"
    );
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

/// The decided part of a BMC result: what was violated or reached and at which
/// step, coverage, and deadlock. Witness *values* are left out: a tolerated
/// `unknown` (a dropped range lemma) can make the solver pick a different, still
/// valid, witness.
fn verdict(result: &fsl_verifier::BmcResult) -> String {
    let violation = |violation: &Option<fsl_verifier::BmcViolation>| {
        violation.as_ref().map(|violation| {
            (
                violation.kind.clone(),
                violation.name.clone(),
                violation.step,
            )
        })
    };
    let reachables = result
        .reachables
        .iter()
        .map(|(name, witness)| (name.clone(), witness.as_ref().map(|witness| witness.step)))
        .collect::<Vec<_>>();
    format!(
        "{:?}",
        (
            violation(&result.violation),
            violation(&result.leadsto_violation),
            reachables,
            &result.action_coverage,
            result.deadlock_step,
            result.frontier_progress,
        )
    )
}

/// #1251 behavioural complement to `fsl-verifier`'s source contract: an
/// `unknown` answer at *any* satisfiability check of a BMC run must either
/// surface as an error the verifier marks as a solver failure, or leave the
/// result exactly as the clean run computed it. The second case is the
/// documented tolerance of checks whose answer can only strengthen a query
/// (`entailed` keeps a range lemma out on `unknown`). A check that folded
/// `unknown` into a decided answer would change the result here: the clean run
/// has satisfiable probes (action coverage, the reachable witness), so folding
/// one of them into "unsat" drops coverage or the witness; and an unmarked
/// error fails the `is_solver_failure` assertion.
#[test]
fn bmc_never_folds_an_unknown_into_a_different_result() {
    let source = r"
spec EveryCheck {
  type Small = 0..2
  state { x: Small, q: Seq<Small, 2> }
  init { x = 0 q = Seq {} }
  action up() { requires x < 2 x = x + 1 }
  action push(v: Small) { requires q.size() < 2 q = q.push(v) }
  action pop() { requires q.size() > 0 q = q.pop() }
  invariant Small { x <= 2 }
  reachable Full { q.size() == 2 }
}
";
    let model = build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse"))
        .expect("build");
    let depth = 3;
    let mut counting = FirstCheckFault::counting();
    let clean = verdict(
        &block_on(fsl_verifier::verify_bounded(&model, &mut counting, depth)).expect("clean run"),
    );
    let checks = counting.checks;
    assert!(
        checks > 3,
        "the run must reach the per-step probes: {checks}"
    );
    let mut errors = 0;
    for fault_at in 1..=checks {
        let mut solver = FirstCheckFault::at(InjectedCheck::Unknown, fault_at);
        match block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth)) {
            Err(error) => {
                errors += 1;
                assert!(
                    error.is_solver_failure(),
                    "check {fault_at} of {checks}: {error}"
                );
            }
            Ok(result) => assert_eq!(
                verdict(&result),
                clean,
                "an unknown at check {fault_at} of {checks} was folded into a different result"
            ),
        }
    }
    assert!(errors > 0, "no injected unknown surfaced as an error");
}

/// #1251 MF2: an unconstrained `Seq` length can be projected negative in a
/// type-bound counterexample. That is a semantic error (a likely projection
/// gap like #1283's), not a solver failure, so `fslc mutate` keeps it a kill.
#[test]
fn a_negative_seq_length_projection_is_not_a_solver_failure() {
    let source = r"
spec SeqNoInit {
  type V = 0..2
  state { q: Seq<V, 2>, n: Int }
  init { n = 0 }
  action push(v: V) { requires q.size() < 2 q = q.push(v) n = 1 }
  action pop() { requires q.size() > 0 q = q.pop() }
}
";
    let model = build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse"))
        .expect("build");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 3))
        .expect_err("the unconstrained Seq length is projected out of range");
    assert!(
        error.to_string().contains("model sequence length"),
        "{error}"
    );
    assert!(!error.is_solver_failure(), "{error}");
}

/// #1251: a model value the solver cannot produce (`model_eval` answering
/// `None`) while extracting a witness is a solver failure, like an `unknown`
/// answer, at every extraction site: each run makes one model read (the k-th,
/// for every k a clean run performs) unavailable, which reaches the state
/// values as well as the action choice of a trace step.
#[test]
fn an_unavailable_model_value_is_a_solver_failure() {
    let source = r"
spec NoModel {
  type Small = 0..2
  state { x: Small }
  init { x = 0 }
  action up() { requires x < 2 x = x + 1 }
  reachable Two { x == 2 }
}
";
    let model = build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse"))
        .expect("build");
    let mut counting = FirstCheckFault::counting();
    let _ = block_on(fsl_verifier::verify_bounded(&model, &mut counting, 3)).expect("clean run");
    let reads = counting.model_reads.get();
    assert!(reads > 0, "the reachable witness must be extracted");
    for read in 1..=reads {
        let mut solver = FirstCheckFault::counting();
        solver.no_model_at = Some(read);
        let error = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 3))
            .expect_err(&format!("model read {read} of {reads} is needed"));
        assert!(error.is_solver_failure(), "read {read} of {reads}: {error}");
    }
}

/// #1519 mutation detectors for p2.bmc.unchecked-sequence-overflow,
/// p2.trace.missing-sequence-slots and p2.trace.action-choice-decoding.
/// Use the library, not the CLI's concrete pre-pass. Expected sequences follow
/// append semantics independently: capacity bounds state storage, not the value
/// of a failed assignment. Both rejecting and within-capacity controls matter.
mod sequence_capacity {
    use super::*;
    use fsl_core::{TraceAction, TraceChange, TraceStep};

    fn parse(source: &str) -> fsl_core::KernelModel {
        build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse Seq fixture"))
            .expect("build Seq fixture")
    }

    fn assert_overflow_error(error: &fsl_verifier::VerifyError) {
        assert_eq!(error.to_string(), "model sequence length exceeds capacity");
        assert!(!error.is_solver_failure(), "{error}");
    }

    fn xs(values: &[i64]) -> BTreeMap<String, FslValue> {
        BTreeMap::from([(
            "xs".to_owned(),
            FslValue::Seq(values.iter().copied().map(FslValue::Int).collect()),
        )])
    }

    fn action(name: &str, params: BTreeMap<String, FslValue>) -> TraceAction {
        TraceAction {
            name: name.to_owned(),
            params,
        }
    }

    // Construct complete expected traces from literal states/actions, including
    // changes and absence of an initial action. No observed fields are excluded.
    fn trace(states: &[BTreeMap<String, FslValue>], actions: &[TraceAction]) -> Vec<TraceStep> {
        assert_eq!(states.len(), actions.len() + 1);
        states
            .iter()
            .enumerate()
            .map(|(step, state)| TraceStep {
                step,
                state: state.clone(),
                action: step.checked_sub(1).map(|index| actions[index].clone()),
                changes: if step == 0 {
                    BTreeMap::new()
                } else {
                    state
                        .iter()
                        .filter(|(name, value)| states[step - 1][*name] != **value)
                        .map(|(name, value)| {
                            (
                                name.clone(),
                                TraceChange {
                                    from: states[step - 1][name].clone(),
                                    to: value.clone(),
                                },
                            )
                        })
                        .collect()
                },
            })
            .collect()
    }

    #[test]
    fn unchecked_overflow_is_a_semantic_error_for_both_property_polarities() {
        // R0/R1/C1: neither a true invariant nor a false reachable may consume
        // a fixed-capacity frame whose logical length exceeds its slots.
        for (capacity, initial, property, depth) in [
            (0, "", "invariant P { xs.size() <= 1 }", 1),
            (1, "0", "invariant P { xs.size() == 1 or xs.at(1) == 1 }", 1),
            (
                1,
                "0",
                "reachable R { xs.size() == 2 and xs.at(1) == 0 }",
                1,
            ),
            (1, "", "invariant P { xs.size() <= 1 or xs.at(1) == 1 }", 2),
        ] {
            let model = parse(&format!(
                "spec UncheckedSeq {{ type K = 0..1
                 state {{ xs: Seq<K, {capacity}> }}
                 init {{ xs = Seq {{ {initial} }} }}
                 action append() {{ xs = xs.push(1) }}
                 {property} }}"
            ));
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
            let error = block_on(fsl_verifier::verify_bounded_selected(
                &model,
                &mut solver,
                depth,
                Some(&BTreeSet::new()),
            ))
            .expect_err("an unselected Seq bound must still reject a missing suffix");
            assert_overflow_error(&error);
        }
    }

    #[test]
    fn guarded_unchecked_sequence_can_be_empty_partial_and_exactly_full() {
        // R2/R3/R4: using ==, < or >= instead of > must not reject any of
        // lengths 0, 1, 2 at capacity 2. The guard prevents a third append.
        let model = parse(
            "spec GuardedSeq { type K = 0..1
             state { xs: Seq<K, 2> }
             init { xs = Seq {} }
             action append() { requires xs.size() < 2 xs = xs.push(1) }
             invariant P { xs.size() <= 2 }
             reachable Full { xs.size() == 2 }
            }",
        );
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let result = block_on(fsl_verifier::verify_bounded_selected(
            &model,
            &mut solver,
            3,
            Some(&BTreeSet::new()),
        ))
        .expect("all reachable lengths fit the unchecked Seq frame");
        assert!(result.violation.is_none(), "{result:?}");
        assert!(result.leadsto_violation.is_none(), "{result:?}");
        let reached = result.reachables["Full"].as_ref().expect("reach capacity");
        assert_eq!(reached.step, 2);
        assert_eq!(
            reached.trace,
            trace(
                &[xs(&[]), xs(&[1]), xs(&[1, 1])],
                &[
                    action("append", BTreeMap::new()),
                    action("append", BTreeMap::new()),
                ],
            )
        );
    }

    #[test]
    fn checked_overflow_projects_the_selected_actions_actual_attempted_sequence() {
        // H0/H2/H3/T4/T5/A1: the second action and its parameter determine
        // the lost suffix, not the first instance or a padded frame value.
        for (capacity, initial, actions, depth, states, expected_actions) in [
            (
                0,
                "",
                "action append(v: K) { requires v == 1 xs = xs.push(v) }",
                1,
                vec![xs(&[]), xs(&[1])],
                vec![action(
                    "append",
                    BTreeMap::from([("v".to_owned(), FslValue::Int(1))]),
                )],
            ),
            (
                1,
                "",
                "action seed() { requires xs.size() == 0 xs = xs.push(0) }
                 action append(v: K) { requires xs.size() == 1 and v == 1 xs = xs.push(v) }",
                2,
                vec![xs(&[]), xs(&[0]), xs(&[0, 1])],
                vec![
                    action("seed", BTreeMap::new()),
                    action(
                        "append",
                        BTreeMap::from([("v".to_owned(), FslValue::Int(1))]),
                    ),
                ],
            ),
        ] {
            let model = parse(&format!(
                "spec CheckedSeq {{ type K = 0..1
                 state {{ xs: Seq<K, {capacity}> }}
                 init {{ xs = Seq {{ {initial} }} }} {actions} }}"
            ));
            let expected = trace(&states, &expected_actions);
            let selected = BTreeSet::from(["_bounds_xs".to_owned()]);
            for bounds in [None, Some(&selected)] {
                let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
                let result = block_on(fsl_verifier::verify_bounded_selected(
                    &model,
                    &mut solver,
                    depth,
                    bounds,
                ))
                .expect("a checked overflow is a type-bound witness, not a projection error");
                let violation = result
                    .violation
                    .expect("reject the failed state assignment");
                assert_eq!(
                    violation,
                    fsl_verifier::BmcViolation {
                        kind: "type_bound".to_owned(),
                        name: "_bounds_xs".to_owned(),
                        step: depth,
                        last_action: Some("append".to_owned()),
                        trace: expected.clone(),
                        leads_to: None,
                    }
                );
            }
        }
    }

    #[test]
    fn induction_non_bound_cti_with_missing_suffix_fails_closed() {
        // Plan A: depth-one BMC passes ([] -> [1]), but induction
        // also considers [0] -> [0,1]. Its non-bound CTI must not read the
        // missing at(1) slot as 0. Exercise invariant and transition SAT arms.
        // This is not an H0 detector: H0 fails earlier while projecting the
        // bound CTI. The checked-overflow test above detects that mutation.
        for property in ["invariant", "trans"] {
            let model = parse(&format!(
                "spec InductionOverflow {{ type K = 0..1
                 state {{ xs: Seq<K, 1> }} init {{ xs = Seq {{}} }}
                 action append() {{ xs = xs.push(1) }}
                 {property} P {{ xs.size() <= 1 or xs.at(1) == 1 }} }}"
            ));
            let mut base_solver = fsl_solver_z3::Z3Solver::new().expect("create base solver");
            let base = block_on(fsl_verifier::verify_bounded(&model, &mut base_solver, 1))
                .expect("the reachable depth-one base fits capacity");
            assert!(base.violation.is_none(), "{base:?}");
            let mut solver = fsl_solver_z3::Z3Solver::new().expect("create induction solver");
            let error = block_on(fsl_verifier::prove_induction(&model, &mut solver, 1))
                .expect_err("a non-bound CTI cannot use a missing Seq suffix");
            assert_overflow_error(&error);
        }
    }

    #[test]
    fn induction_with_a_representable_full_sequence_keeps_the_real_cti() {
        // H1/H4: true or >= must not classify a representable CTI as missing.
        // The CTI is unreachable from init (go=false); it is a valid induction
        // step with go=true, [] -> [1], violating P without exceeding capacity.
        let model = parse(
            "spec RepresentableCti { type K = 0..1
             state { xs: Seq<K, 1>, go: Bool }
             init { xs = Seq {} go = false }
             action append() { requires go and xs.size() == 0 xs = xs.push(1) }
             action idle() { requires not go }
             invariant P { xs.size() == 0 }
            }",
        );
        let mut base_solver = fsl_solver_z3::Z3Solver::new().expect("create base solver");
        let base = block_on(fsl_verifier::verify_bounded(&model, &mut base_solver, 1))
            .expect("the initialized system stutters");
        assert!(base.violation.is_none(), "{base:?}");
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create induction solver");
        let result = block_on(fsl_verifier::prove_induction(&model, &mut solver, 1))
            .expect("an in-capacity induction CTI must remain reportable");
        let states = [xs(&[]), xs(&[1])]
            .map(|mut state| {
                state.insert("go".to_owned(), FslValue::Bool(true));
                state
            })
            .to_vec();
        assert_eq!(
            result,
            fsl_verifier::InductionResult {
                k_used: BTreeMap::from([("_bounds_xs".to_owned(), 1)]),
                cti: Some(fsl_verifier::InductionCti {
                    kind: "invariant".to_owned(),
                    name: "P".to_owned(),
                    k: 1,
                    trace: trace(&states, &[action("append", BTreeMap::new())]),
                }),
            }
        );
    }
}
