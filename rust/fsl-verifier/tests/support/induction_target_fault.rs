// SPDX-License-Identifier: Apache-2.0
//! Real-Z3 target-bound routing faults; all unaffected operations delegate.
use fsl_solver::{
    CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort,
    VerificationStatistics,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Unknown,
    Backend,
    Assert,
    Pop,
}
pub struct FaultSolver {
    inner: fsl_solver_z3::Z3Solver,
    fault: Option<Fault>,
    active: bool,
    pub consumed: usize,
    pub depth: u32,
    pub trace: Vec<String>,
}
impl FaultSolver {
    pub fn new(fault: Option<Fault>) -> Self {
        Self {
            inner: fsl_solver_z3::Z3Solver::new().unwrap(),
            fault,
            active: false,
            consumed: 0,
            depth: 0,
            trace: vec![],
        }
    }
    fn take(&mut self, fault: Fault) -> bool {
        if self.active && self.fault == Some(fault) {
            self.fault = None;
            self.consumed += 1;
            self.trace
                .push(format!("INJECT {fault:?} depth={}", self.depth));
            true
        } else {
            false
        }
    }
    fn injected_check(&mut self) -> Option<SolverResult<SatResult>> {
        if self.take(Fault::Unknown) {
            Some(Ok(SatResult::Unknown))
        } else if self.take(Fault::Backend) {
            Some(Err(SolverError::new("induction-target-check failure")))
        } else {
            None
        }
    }
}
// Only delegation boilerplate; no independent terms, solver or result semantics.
macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self,$($arg:$ty),*) -> $result {self.inner.$name($($arg),*)})*
    };
}
impl SmtSolver for FaultSolver {
    type Term = <fsl_solver_z3::Z3Solver as SmtSolver>::Term;
    delegate! {
        version() -> &str;
        statistics() -> VerificationStatistics;
        sort(term: &Self::Term) -> Sort;
        literal_value(term: &Self::Term) -> Option<ModelValue>;
        bool_value(value: bool) -> Self::Term;
        int_value(value: i64) -> Self::Term;
        constant(name: &str, sort: &Sort) -> SolverResult<Self::Term>;
        not(term: &Self::Term) -> SolverResult<Self::Term>;
        and(terms: &[Self::Term]) -> SolverResult<Self::Term>;
        or(terms: &[Self::Term]) -> SolverResult<Self::Term>;
        implies(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        equal(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        ite(condition: &Self::Term, then_term: &Self::Term, else_term: &Self::Term) -> SolverResult<Self::Term>;
        neg(term: &Self::Term) -> SolverResult<Self::Term>;
        add(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        sub(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        mul(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        div(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        modulo(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        lt(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        le(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        gt(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        ge(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        const_array(domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term>;
        select(array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term>;
        store(array: &Self::Term, index: &Self::Term, value: &Self::Term) -> SolverResult<Self::Term>;
        substitute(term: &Self::Term, replacements: &[(Self::Term, Self::Term)]) -> SolverResult<Self::Term>;
        unsat_core() -> SolverResult<Vec<Self::Term>>;
    }
    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
        self.inner.model_eval(term)
    }
    fn set_query_context(&mut self, kind: &str, name: &str) {
        self.trace
            .push(format!("CONTEXT {kind}/{name} depth={}", self.depth));
        self.active = kind == "type_bound" && name == "_bounds_n";
        self.inner.set_query_context(kind, name);
    }
    fn push(&mut self) {
        self.inner.push();
        self.depth += 1;
        self.trace.push(format!("PUSH depth={}", self.depth));
    }
    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        let actual = self.inner.pop(levels);
        if actual.is_ok() {
            self.depth -= levels;
        }
        self.trace.push(format!(
            "POP levels={levels} actual={actual:?} depth={}",
            self.depth
        ));
        actual?;
        if self.take(Fault::Pop) {
            Err(SolverError::new("induction-target-pop failure"))
        } else {
            Ok(())
        }
    }
    fn reset(&mut self) -> SolverResult<()> {
        let actual = self.inner.reset();
        if actual.is_ok() {
            self.depth = 0;
            self.active = false;
        }
        self.trace
            .push(format!("RESET actual={actual:?} depth={}", self.depth));
        actual
    }
    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        let actual = if self.take(Fault::Assert) {
            Err(SolverError::new("induction-target-assert failure"))
        } else {
            self.inner.assert(term)
        };
        self.trace.push(format!(
            "ASSERT term={term:?} actual={actual:?} depth={}",
            self.depth
        ));
        actual
    }
    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        let actual = self.inner.assert_and_track(term, tracker);
        self.trace.push(format!(
            "TRACK term={term:?} tracker={tracker:?} actual={actual:?} depth={}",
            self.depth
        ));
        actual
    }
    fn check(&mut self) -> CheckFuture<'_> {
        self.trace.push(format!("CHECK depth={}", self.depth));
        let injected = self.injected_check();
        Box::pin(async move {
            let actual = match injected {
                Some(result) => result,
                None => self.inner.check().await,
            };
            self.trace.push(format!(
                "CHECK_RESULT actual={actual:?} depth={}",
                self.depth
            ));
            actual
        })
    }
    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        self.trace.push(format!(
            "ASSUMING terms={assumptions:?} depth={}",
            self.depth
        ));
        let checked: CheckFuture<'_> = match self.injected_check() {
            Some(result) => Box::pin(async move { result }),
            None => self.inner.check_assuming(assumptions),
        };
        let trace = &mut self.trace;
        let depth = self.depth;
        Box::pin(async move {
            let actual = checked.await;
            trace.push(format!("ASSUMING_RESULT actual={actual:?} depth={depth}"));
            actual
        })
    }
}
