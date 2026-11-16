// SPDX-License-Identifier: Apache-2.0

//! Pin all six ranked obligations with real Z3, including injected Unknown and
//! failed pop. The optional transcript is also compared against the base SHA.

use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_solver::{
    CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort,
    VerificationStatistics,
};
use fsl_solver_z3::Z3Solver;

struct RecordingSolver {
    inner: Z3Solver,
    calls: RefCell<Vec<String>>,
    checks: usize,
    unknown_at: Option<usize>,
    fail_pop: bool,
}

macro_rules! binary {
    ($($name:ident),* $(,)?) => {$(
        fn $name(&self, a: &Self::Term, b: &Self::Term) -> SolverResult<Self::Term> {
            self.calls.borrow_mut().push(stringify!($name).to_owned());
            self.inner.$name(a, b)
        }
    )*};
}

impl SmtSolver for RecordingSolver {
    type Term = <Z3Solver as SmtSolver>::Term;
    fn version(&self) -> &str {
        self.inner.version()
    }
    fn set_query_context(&mut self, kind: &str, name: &str) {
        self.calls.get_mut().push(format!("context {kind} {name}"));
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
        self.calls.borrow_mut().push("not".to_owned());
        self.inner.not(term)
    }
    fn and(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        self.calls.borrow_mut().push("and".to_owned());
        self.inner.and(terms)
    }
    fn or(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        self.inner.or(terms)
    }
    binary!(implies, equal, add, sub, mul, div, modulo, lt, le, gt, ge);
    fn ite(&self, c: &Self::Term, a: &Self::Term, b: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.ite(c, a, b)
    }
    fn neg(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        self.inner.neg(term)
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
        self.calls.get_mut().push("push".to_owned());
        self.inner.push();
    }
    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        self.calls.get_mut().push(format!("pop {levels}"));
        self.inner.pop(levels)?;
        if self.fail_pop && self.unknown_at == Some(self.checks) {
            Err(SolverError::new("rank pop failed"))
        } else {
            Ok(())
        }
    }
    fn reset(&mut self) -> SolverResult<()> {
        self.inner.reset()
    }
    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.calls.get_mut().push(format!("assert {term:?}"));
        self.inner.assert(term)
    }
    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        self.inner.assert_and_track(term, tracker)
    }
    fn check(&mut self) -> CheckFuture<'_> {
        self.checks += 1;
        self.calls.get_mut().push("check".to_owned());
        if self.unknown_at == Some(self.checks) {
            Box::pin(async { Ok(SatResult::Unknown) })
        } else {
            self.inner.check()
        }
    }
    fn check_assuming(&mut self, terms: &[Self::Term]) -> CheckFuture<'_> {
        self.inner.check_assuming(terms)
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
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver yielded"),
    }
}

fn source(actions: &str, trigger: &str, helpful: &str, invariant: &str) -> String {
    format!(
        "spec Ranked {{ state {{ x: Int, gate: Bool }} init {{ x = 2 gate = true }} {actions} {invariant} leadsTo Drain {{ {trigger} ~> x == 0 {helpful} decreases x }} }}"
    )
}

#[test]
fn ranked_obligation_sequences_and_unknown_pop_precedence() {
    let dec = "action dec() { requires x > 0 x = x - 1 }";
    let fair = "fair action dec() { requires x > 0 x = x - 1 }";
    let nonneg = "invariant NonNeg { x >= 0 }";
    let cases = [
        ("proved", source(dec, "x > 0", "", nonneg), None, 3),
        (
            "negative",
            source("action idle() { x = x }", "x != 0", "", ""),
            Some("unbounded_below"),
            1,
        ),
        (
            "fairness",
            source(dec, "x > 0", "helpful dec()", nonneg),
            Some("progress_action_not_fair"),
            2,
        ),
        (
            "sticky",
            source(
                "fair action a() { requires gate x = 0 } fair action b() { requires gate == false x = 0 } action flip() { gate = not gate }",
                "x > 0",
                "helpful a() helpful b()",
                nonneg,
            ),
            Some("helpful_action_enabledness_not_sticky"),
            2,
        ),
        (
            "deadlock",
            source(
                "action dec() { requires x > 2 x = x - 1 }",
                "x > 0",
                "",
                nonneg,
            ),
            Some("deadlock"),
            2,
        ),
        (
            "plain_progress",
            source("action idle() { x = x }", "x > 0", "", nonneg),
            Some("non_decreasing_action"),
            3,
        ),
        (
            "helpful_proved",
            source(fair, "x > 0", "helpful dec()", nonneg),
            None,
            3,
        ),
        (
            "helpful_progress",
            source(
                "fair action idle() { x = x }",
                "x > 0",
                "helpful idle()",
                nonneg,
            ),
            Some("non_decreasing_helpful_action"),
            3,
        ),
    ];
    let mut transcript = String::new();
    for (name, text, expected, checks) in cases {
        let model = build_model(parse_kernel_source(&text, &FsResolver::new(".")).expect("parse"))
            .expect("model");
        for unknown_at in std::iter::once(None).chain((1..=checks).map(Some)) {
            for fail_pop in [false, true] {
                let mut solver = RecordingSolver {
                    inner: Z3Solver::new().expect("solver"),
                    calls: RefCell::default(),
                    checks: 0,
                    unknown_at,
                    fail_pop,
                };
                let result = block_on(fsl_verifier::prove_ranked_leadstos(&model, &mut solver));
                if unknown_at.is_some() {
                    let error = result.as_ref().expect_err("Unknown must fail");
                    assert!(error.is_solver_failure());
                    assert_eq!(
                        error.to_string(),
                        if fail_pop {
                            "rank pop failed"
                        } else {
                            "solver returned unknown in ranking proof"
                        }
                    );
                    assert_eq!(
                        solver.calls.borrow().last().map(String::as_str),
                        Some("pop 1")
                    );
                } else {
                    let result = result.as_ref().expect("ranking query");
                    assert_eq!(
                        result.failure.as_ref().map(|f| f.kind.as_str()),
                        expected,
                        "{name}"
                    );
                    assert_eq!(solver.checks, checks, "{name}");
                    assert_eq!(
                        solver
                            .calls
                            .borrow()
                            .iter()
                            .filter(|c| c.as_str() == "push")
                            .count(),
                        checks
                    );
                    assert_eq!(
                        solver
                            .calls
                            .borrow()
                            .iter()
                            .filter(|c| c.as_str() == "pop 1")
                            .count(),
                        checks
                    );
                }
                transcript.push_str(&format!(
                    "{name} unknown={unknown_at:?} pop={fail_pop}\n{result:?}\n{:#?}\n",
                    solver.calls.borrow()
                ));
            }
        }
    }
    if let Some(path) = std::env::var_os("FSLC_RANK_SEQUENCE_TRANSCRIPT") {
        std::fs::write(path, transcript).expect("write requested base/head transcript");
    }
}
