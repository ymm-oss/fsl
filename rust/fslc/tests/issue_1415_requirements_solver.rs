// SPDX-License-Identifier: Apache-2.0

//! #1415: pin refute's outcome routing through the public solver API.

use fslc_rust::requirements_solver::{Rejection, refute};
use std::cell::Cell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use fsl_solver::{
    CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort,
    VerificationStatistics,
};

// This is an outcome-routing stub, not an SMT evaluator. Terms carry only
// sorts; each fresh solver answers exactly one scripted query. No Z3 needed.
struct StubSolver {
    answer: Option<SolverResult<SatResult>>,
    resets: Rc<Cell<usize>>,
}

macro_rules! binary_terms {
    ($($name:ident => $sort:ident),* $(,)?) => {
        $(fn $name(&self, _: &Sort, _: &Sort) -> SolverResult<Sort> {
            Ok(Sort::$sort)
        })*
    };
}

impl SmtSolver for StubSolver {
    type Term = Sort;

    fn version(&self) -> &'static str {
        "requirements-outcome-stub"
    }
    fn set_query_context(&mut self, _: &str, _: &str) {}
    fn statistics(&self) -> VerificationStatistics {
        VerificationStatistics::default()
    }
    fn sort(&self, term: &Sort) -> Sort {
        term.clone()
    }
    fn bool_value(&self, _: bool) -> Sort {
        Sort::Bool
    }
    fn int_value(&self, _: i64) -> Sort {
        Sort::Int
    }
    fn constant(&self, _: &str, sort: &Sort) -> SolverResult<Sort> {
        Ok(sort.clone())
    }
    fn not(&self, _: &Sort) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    fn and(&self, _: &[Sort]) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    fn or(&self, _: &[Sort]) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    binary_terms! {
        implies => Bool, equal => Bool,
        add => Int, sub => Int, mul => Int, div => Int, modulo => Int,
        lt => Bool, le => Bool, gt => Bool, ge => Bool,
    }
    fn ite(&self, _: &Sort, then_term: &Sort, _: &Sort) -> SolverResult<Sort> {
        Ok(then_term.clone())
    }
    fn neg(&self, _: &Sort) -> SolverResult<Sort> {
        Ok(Sort::Int)
    }
    fn const_array(&self, domain: &Sort, value: &Sort) -> SolverResult<Sort> {
        Ok(Sort::array(domain.clone(), value.clone()))
    }
    fn select(&self, array: &Sort, _: &Sort) -> SolverResult<Sort> {
        let Sort::Array { range, .. } = array else {
            panic!("expected array term, got {array:?}");
        };
        Ok(*range.clone())
    }
    fn store(&self, array: &Sort, _: &Sort, _: &Sort) -> SolverResult<Sort> {
        Ok(array.clone())
    }
    fn substitute(&self, term: &Sort, _: &[(Sort, Sort)]) -> SolverResult<Sort> {
        Ok(term.clone())
    }
    fn push(&mut self) {}
    fn pop(&mut self, _: u32) -> SolverResult<()> {
        Ok(())
    }
    fn reset(&mut self) -> SolverResult<()> {
        self.resets.set(self.resets.get() + 1);
        Ok(())
    }
    fn assert(&mut self, _: &Sort) -> SolverResult<()> {
        Ok(())
    }
    fn assert_and_track(&mut self, _: &Sort, _: &Sort) -> SolverResult<()> {
        panic!("unexpected tracked assertion");
    }
    fn check(&mut self) -> CheckFuture<'_> {
        let answer = self.answer.take().expect("one check per fresh solver");
        Box::pin(async move { answer })
    }
    fn check_assuming(&mut self, _: &[Sort]) -> CheckFuture<'_> {
        panic!("unexpected check_assuming");
    }
    fn unsat_core(&self) -> SolverResult<Vec<Sort>> {
        panic!("unexpected unsat_core");
    }
    fn model_eval(&self, term: &Sort) -> SolverResult<Option<ModelValue>> {
        // A concrete projection for SAT; the stub does not establish that
        // this assignment satisfies the query.
        Ok(Some(match term {
            Sort::Bool => ModelValue::Bool(false),
            Sort::Int => ModelValue::Int(0),
            Sort::Array { .. } => panic!("unexpected array projection"),
        }))
    }
}

const SOURCE: &str = r#"fsl 5
requirements R {
  number Amount
  actor Employee
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Submitted
  }
  event submit = Employee submits Claim with amount Draft -> Submitted
  rule REQ-1 "bounds" {
    allow Employee submits Claim only when input amount > 5
    allow Employee submits Claim only when input amount < 4
    never Employee submits Claim when input amount == 0
  }
}
verify {
  instances Claim = 1
  values Amount = 0..2000000
}
"#;

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "requirements stub executor deadline expired"
                );
                std::thread::park_timeout(remaining);
            }
        }
    }
}

fn run(answers: Vec<SolverResult<SatResult>>) -> Option<Rejection> {
    let options = fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    );
    let (_, model) = fslc_rust::check_stages::load_kernel_model(
        SOURCE,
        "requirements-stub.fsl",
        &fsl_core::FsResolver::new("."),
        &options,
    )
    .expect("large domain must defer the guard check, not reject during lowering");
    let mut answers = VecDeque::from(answers);
    let expected_resets = answers.len();
    let resets = Rc::new(Cell::new(0));
    let mut new_solver = || {
        Ok(StubSolver {
            answer: Some(answers.pop_front().expect("unexpected solver query")),
            resets: Rc::clone(&resets),
        })
    };
    let result = block_on(refute(SOURCE, &model, &options, &mut new_solver));
    assert!(answers.is_empty(), "all scripted queries must be consumed");
    assert_eq!(resets.get(), expected_resets, "reset every fresh solver");
    result
}

fn assert_guard_rejection(rejection: &Rejection, message: &str) {
    assert_eq!(rejection.code, fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD);
    assert_eq!(
        rejection.span.python_loc(),
        serde_json::json!({"line": 9, "column": 3})
    );
    assert_eq!(rejection.message, message);
}

#[test]
fn refute_sat_accepts_a_guard_counterexample() {
    assert_eq!(run(vec![Ok(SatResult::Sat)]), None);
}

#[test]
fn refute_unsat_reports_only_the_minimal_conflicting_subset() {
    // Initial conflict; removing either bound makes it satisfiable;
    // removing the unrelated never rule leaves the conflict unsatisfiable.
    let rejection = run(vec![
        Ok(SatResult::Unsat),
        Ok(SatResult::Sat),
        Ok(SatResult::Sat),
        Ok(SatResult::Unsat),
    ])
    .expect("unsatisfiable guard set must be rejected");
    assert_guard_rejection(
        &rejection,
        "event 'submit' can never occur: when Claim is Draft, no value of its fields and inputs satisfies the rules \
         REQ-1 `allow Employee submits Claim only when input amount > 5`; \
         REQ-1 `allow Employee submits Claim only when input amount < 4` \
         (checked by the solver, because its fields and inputs take 2000001 values, more than the 1000000 enumerated)",
    );
}

#[test]
fn refute_unknown_rejects_instead_of_accepting_or_claiming_a_conflict() {
    let rejection = run(vec![Ok(SatResult::Unknown)])
        .expect("unknown must reject fail-closed, not defer acceptance");
    assert_guard_rejection(
        &rejection,
        "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
         (enumeration skipped it because its fields and inputs take 2000001 values, more than the 1000000 enumerated): \
         solver returned unknown in invariant implication",
    );
}

#[test]
fn refute_backend_error_rejects_instead_of_accepting_or_claiming_a_conflict() {
    let rejection = run(vec![Err(SolverError::new("injected backend failure"))])
        .expect("backend error must reject fail-closed");
    assert_guard_rejection(
        &rejection,
        "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
         (enumeration skipped it because its fields and inputs take 2000001 values, more than the 1000000 enumerated): \
         injected backend failure",
    );
}
