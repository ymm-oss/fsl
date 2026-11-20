// SPDX-License-Identifier: Apache-2.0

//! Native Z3 implementation of the backend-neutral FSL SMT contract.

use fsl_solver::{
    BackendStatistics, CheckFuture, ModelFuture, ModelValue, SatResult, SmtSolver, SolverError,
    SolverMetrics, SolverResult, Sort, VerificationStatistics,
};
use std::sync::OnceLock;
use z3::ast::{Array, Ast, Bool, Dynamic, Int};
use z3::{Model, Params, Solver, StatisticsValue};

const REQUIRED_Z3_VERSION: &str = "4.16.0";
const RANDOM_SEED: u32 = 0;

/// Return the version reported by the linked Z3 library.
#[must_use]
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let version = z3::full_version();
        if version.starts_with("Z3 ") {
            version.to_owned()
        } else {
            format!("Z3 {version}")
        }
    })
}

/// Run `callback` with every [`Z3Solver`] it creates on a new Z3 context.
///
/// Solvers created on one thread share that thread's context, and the model a
/// check returns can depend on what earlier solvers on it did; a satisfiability
/// verdict cannot. Work run here leaves the calling thread's context as it
/// found it, so it does not change the models of later solvers there.
pub fn with_fresh_context<R: Send + Sync>(callback: impl FnOnce() -> R + Send + Sync) -> R {
    z3::with_z3_config(&z3::Config::new(), callback)
}

#[derive(Clone, Debug)]
pub enum Z3Term {
    Bool(Bool),
    Int(Int),
    Array { ast: Array, sort: Sort },
}

impl Z3Term {
    fn sort(&self) -> Sort {
        match self {
            Self::Bool(_) => Sort::Bool,
            Self::Int(_) => Sort::Int,
            Self::Array { sort, .. } => sort.clone(),
        }
    }

    fn dynamic(&self) -> Dynamic {
        match self {
            Self::Bool(ast) => Dynamic::from_ast(ast),
            Self::Int(ast) => Dynamic::from_ast(ast),
            Self::Array { ast, .. } => Dynamic::from_ast(ast),
        }
    }
}

/// Native solver pinned to the project-wide Z3 version.
#[derive(Debug)]
pub struct Z3Solver {
    solver: Solver,
    version: String,
    stack_depth: u32,
    metrics: SolverMetrics,
    timeout_ms: Option<u32>,
    /// Whether [`SmtSolver::resolve_output_model`] re-solves (#1940).
    resolves_output: bool,
    /// The assumptions of the last check, part of what its model satisfies.
    assumptions: Vec<Bool>,
    /// The re-solved model the next reads come from, until the session moves.
    output_model: Option<Model>,
    /// The trackers of the tracked assertions in scope: Z3 asserts
    /// `tracker => term` and assumes every tracker in each check, so they are
    /// part of what a check's model satisfies.
    trackers: Vec<Bool>,
    /// The number of trackers in scope at each open `push`.
    tracker_marks: Vec<usize>,
}

fn session_params(timeout_ms: Option<u32>) -> Params {
    let mut params = Params::new();
    params.set_u32("random_seed", RANDOM_SEED);
    params.set_u32("smt.random_seed", RANDOM_SEED);
    if let Some(timeout_ms) = timeout_ms {
        params.set_u32("timeout", timeout_ms);
    }
    params
}

/// What a fresh context answered for the assertions of a `sat` session.
enum Resolved {
    Model(z3::Synchronized<Model>),
    Unsat,
    Unknown,
}

/// Solve `assertions` alone on a new Z3 context, configured as a session is,
/// and return its model translated back to the calling thread's context.
///
/// A new context has no history, so the model depends only on `assertions`
/// (their terms and order) and on the Z3 build (#1940).
fn resolve_in_fresh_context(
    assertions: &[Bool],
    timeout_ms: Option<u32>,
) -> SolverResult<Option<Model>> {
    let assertions = z3::Synchronized::new(&assertions.to_vec());
    let resolved = z3::with_z3_config(&z3::Config::new(), || {
        let assertions = assertions.recover();
        let solver = Solver::new();
        solver.set_params(&session_params(timeout_ms));
        for assertion in &assertions {
            solver.assert(assertion);
        }
        match solver.check() {
            z3::SatResult::Sat => solver.get_model().map_or(Resolved::Unknown, |model| {
                Resolved::Model(z3::Synchronized::new(&model))
            }),
            z3::SatResult::Unsat => Resolved::Unsat,
            z3::SatResult::Unknown => Resolved::Unknown,
        }
    });
    match resolved {
        Resolved::Model(model) => Ok(Some(model.recover())),
        // Only a check with a time limit can answer `unknown` here; the
        // reads then keep the session's model, as a backend without this
        // method does.
        Resolved::Unknown => Ok(None),
        Resolved::Unsat => Err(SolverError::new(
            "the assertions of a satisfiable check are unsatisfiable in a fresh Z3 context",
        )),
    }
}

impl Z3Solver {
    /// Construct a backend, rejecting a library that does not satisfy the pin.
    ///
    /// # Errors
    ///
    /// Returns an error when the loaded Z3 library is not version 4.16.0.
    pub fn new() -> SolverResult<Self> {
        Self::configured(None)
    }

    /// [`Self::new`], except that every `check` gives up after `milliseconds`
    /// of wall-clock time and answers [`SatResult::Unknown`].
    ///
    /// For optional evidence only (the #1149 ranking pre-pass), whose caller
    /// treats `Unknown` as "no shortcut" and falls back to a complete search:
    /// a wall-clock limit makes the answer depend on machine load, so a
    /// verdict must never rest on it.
    ///
    /// # Errors
    ///
    /// Returns an error when the loaded Z3 library is not version 4.16.0.
    pub fn with_timeout_ms(milliseconds: u32) -> SolverResult<Self> {
        Self::configured(Some(milliseconds))
    }

    fn configured(timeout_ms: Option<u32>) -> SolverResult<Self> {
        let version = version().to_owned();
        let required_prefix = format!("Z3 {REQUIRED_Z3_VERSION}.");
        if !version.starts_with(&required_prefix) {
            return Err(SolverError::new(format!(
                "expected Z3 {REQUIRED_Z3_VERSION}, loaded {version}"
            )));
        }
        let solver = Solver::new();
        solver.set_params(&session_params(timeout_ms));
        Ok(Self {
            solver,
            version,
            stack_depth: 0,
            metrics: SolverMetrics::default(),
            timeout_ms,
            resolves_output: true,
            assumptions: Vec::new(),
            output_model: None,
            trackers: Vec::new(),
            tracker_marks: Vec::new(),
        })
    }

    /// This solver for a session whose models nobody sees: only its verdicts
    /// are used, and every trace it projects is dropped. Its
    /// [`SmtSolver::resolve_output_model`] keeps the check's model, so it
    /// spends no fresh-context solve. A model read from it can depend on the
    /// queries the session asked before (#1940).
    #[must_use]
    pub fn verdicts_only(mut self) -> Self {
        self.resolves_output = false;
        self
    }

    /// The session moved: the next reads come from its next check.
    fn moved(&mut self) {
        self.output_model = None;
    }

    #[cfg(feature = "test-hooks")]
    fn perturb_history(&self) {
        if test_hooks::perturbing() {
            // One more scoped query before the check: the negation of the
            // latest assertion, as a probe ordered before the query would be.
            if let Some(latest) = self.solver.get_assertions().last() {
                self.solver.push();
                self.solver.assert(latest.not());
                let _ = self.solver.check();
                self.solver.pop(1);
            }
        }
    }

    #[cfg(not(feature = "test-hooks"))]
    #[allow(clippy::unused_self)]
    fn perturb_history(&self) {}
}

/// Test-only switches (#1940), compiled only with the `test-hooks` feature:
/// they let a test perturb a session's query history and turn the
/// output-model re-solve off, on the calling thread.
#[cfg(feature = "test-hooks")]
pub mod test_hooks {
    use std::cell::Cell;

    thread_local! {
        static PERTURB: Cell<bool> = const { Cell::new(false) };
        static NO_RESOLVE: Cell<bool> = const { Cell::new(false) };
    }

    /// Before every check of every solver on this thread, ask one extra
    /// scoped query: the negation of the latest assertion.
    pub fn set_perturbing(on: bool) {
        PERTURB.with(|flag| flag.set(on));
    }

    pub(crate) fn perturbing() -> bool {
        PERTURB.with(Cell::get)
    }

    /// Make every solver on this thread read the check's own model for
    /// output, as before #1940: the control that shows what the re-solve
    /// changes.
    pub fn set_resolve_disabled(disabled: bool) {
        NO_RESOLVE.with(|flag| flag.set(disabled));
    }

    pub(crate) fn resolve_disabled() -> bool {
        NO_RESOLVE.with(Cell::get)
    }
}

#[cfg(feature = "test-hooks")]
fn resolve_disabled() -> bool {
    test_hooks::resolve_disabled()
}

#[cfg(not(feature = "test-hooks"))]
const fn resolve_disabled() -> bool {
    false
}

fn statistic_value(solver: &Solver, names: &[&str]) -> Option<StatisticsValue> {
    let statistics = solver.get_statistics();
    names.iter().find_map(|name| statistics.value(name))
}

fn statistic_count(solver: &Solver, names: &[&str]) -> Option<u64> {
    statistic_value(solver, names).and_then(|value| match value {
        StatisticsValue::UInt(value) => Some(u64::from(value)),
        StatisticsValue::Double(_) => None,
    })
}

fn statistic_double(solver: &Solver, names: &[&str]) -> Option<f64> {
    statistic_value(solver, names).map(|value| match value {
        StatisticsValue::UInt(value) => f64::from(value),
        StatisticsValue::Double(value) => value,
    })
}

fn backend_statistics(solver: &Solver) -> BackendStatistics {
    let propagations = statistic_count(solver, &["propagations"]).or_else(|| {
        let binary = statistic_count(solver, &["sat propagations 2ary"]);
        let nary = statistic_count(solver, &["sat propagations nary"]);
        match (binary, nary) {
            (Some(binary), Some(nary)) => Some(binary + nary),
            (binary, nary) => binary.or(nary),
        }
    });
    BackendStatistics {
        conflicts: statistic_count(solver, &["conflicts", "sat conflicts"]),
        decisions: statistic_count(solver, &["decisions", "sat decisions"]),
        propagations,
        memory_mb: statistic_double(solver, &["memory", "max memory"]),
    }
}

fn z3_sort(sort: &Sort) -> z3::Sort {
    match sort {
        Sort::Bool => z3::Sort::bool(),
        Sort::Int => z3::Sort::int(),
        Sort::Array { domain, range } => z3::Sort::array(&z3_sort(domain), &z3_sort(range)),
    }
}

fn term_from_dynamic(ast: &Dynamic, sort: &Sort) -> SolverResult<Z3Term> {
    match sort {
        Sort::Bool => ast
            .as_bool()
            .map(Z3Term::Bool)
            .ok_or_else(|| SolverError::new("Z3 returned a non-Boolean term")),
        Sort::Int => ast
            .as_int()
            .map(Z3Term::Int)
            .ok_or_else(|| SolverError::new("Z3 returned a non-integer term")),
        Sort::Array { .. } => ast
            .as_array()
            .map(|ast| Z3Term::Array {
                ast,
                sort: sort.clone(),
            })
            .ok_or_else(|| SolverError::new("Z3 returned a non-array term")),
    }
}

fn expect_bool(term: &Z3Term) -> SolverResult<&Bool> {
    if let Z3Term::Bool(ast) = term {
        Ok(ast)
    } else {
        Err(SolverError::new("expected Boolean SMT term"))
    }
}

fn expect_int(term: &Z3Term) -> SolverResult<&Int> {
    if let Z3Term::Int(ast) = term {
        Ok(ast)
    } else {
        Err(SolverError::new("expected integer SMT term"))
    }
}

fn expect_array(term: &Z3Term) -> SolverResult<(&Array, &Sort, &Sort)> {
    let Z3Term::Array {
        ast,
        sort: Sort::Array { domain, range },
    } = term
    else {
        return Err(SolverError::new("expected array SMT term"));
    };
    Ok((ast, domain, range))
}

fn ensure_same_sort(left: &Z3Term, right: &Z3Term) -> SolverResult<Sort> {
    let left_sort = left.sort();
    if left_sort == right.sort() {
        Ok(left_sort)
    } else {
        Err(SolverError::new("SMT term sort mismatch"))
    }
}

fn map_sat(result: z3::SatResult) -> SatResult {
    match result {
        z3::SatResult::Sat => SatResult::Sat,
        z3::SatResult::Unsat => SatResult::Unsat,
        z3::SatResult::Unknown => SatResult::Unknown,
    }
}

fn evaluate_model(model: &Model, term: &Z3Term) -> SolverResult<Option<ModelValue>> {
    match term {
        Z3Term::Bool(ast) => Ok(model
            .eval(ast, true)
            .and_then(|value| value.as_bool())
            .map(ModelValue::Bool)),
        Z3Term::Int(ast) => Ok(model
            .eval(ast, true)
            .and_then(|value| value.as_i64())
            .map(ModelValue::Int)),
        Z3Term::Array { .. } => Err(SolverError::new(
            "project array elements with select before model evaluation",
        )),
    }
}

impl SmtSolver for Z3Solver {
    type Term = Z3Term;

    fn literal_value(&self, term: &Self::Term) -> Option<ModelValue> {
        match term {
            Z3Term::Bool(value) => value.as_bool().map(ModelValue::Bool),
            Z3Term::Int(value) => value.as_i64().map(ModelValue::Int),
            Z3Term::Array { .. } => None,
        }
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn set_query_context(&mut self, kind: &str, name: &str) {
        self.metrics.set_context(kind, name);
    }

    fn statistics(&self) -> VerificationStatistics {
        self.metrics.statistics()
    }

    fn sort(&self, term: &Self::Term) -> Sort {
        term.sort()
    }

    fn bool_value(&self, value: bool) -> Self::Term {
        Z3Term::Bool(Bool::from_bool(value))
    }

    fn int_value(&self, value: i64) -> Self::Term {
        Z3Term::Int(Int::from_i64(value))
    }

    fn constant(&self, name: &str, sort: &Sort) -> SolverResult<Self::Term> {
        term_from_dynamic(&Dynamic::new_const(name, &z3_sort(sort)), sort)
    }

    fn not(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(expect_bool(term)?.not()))
    }

    fn and(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        let terms = terms
            .iter()
            .map(expect_bool)
            .collect::<SolverResult<Vec<_>>>()?;
        Ok(Z3Term::Bool(Bool::and(&terms)))
    }

    fn or(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        let terms = terms
            .iter()
            .map(expect_bool)
            .collect::<SolverResult<Vec<_>>>()?;
        Ok(Z3Term::Bool(Bool::or(&terms)))
    }

    fn implies(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(
            expect_bool(left)?.implies(expect_bool(right)?),
        ))
    }

    fn equal(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        ensure_same_sort(left, right)?;
        left.dynamic()
            .safe_eq(right.dynamic())
            .map(Z3Term::Bool)
            .map_err(|error| SolverError::new(error.to_string()))
    }

    fn ite(
        &self,
        condition: &Self::Term,
        then_term: &Self::Term,
        else_term: &Self::Term,
    ) -> SolverResult<Self::Term> {
        let sort = ensure_same_sort(then_term, else_term)?;
        let condition = expect_bool(condition)?;
        let result = match (then_term, else_term) {
            (Z3Term::Bool(left), Z3Term::Bool(right)) => Z3Term::Bool(condition.ite(left, right)),
            (Z3Term::Int(left), Z3Term::Int(right)) => Z3Term::Int(condition.ite(left, right)),
            (Z3Term::Array { ast: left, .. }, Z3Term::Array { ast: right, .. }) => Z3Term::Array {
                ast: condition.ite(left, right),
                sort,
            },
            _ => return Err(SolverError::new("SMT term sort mismatch")),
        };
        Ok(result)
    }

    fn neg(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Int(expect_int(term)?.unary_minus()))
    }

    fn add(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Int(Int::add(&[
            expect_int(left)?,
            expect_int(right)?,
        ])))
    }

    fn sub(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Int(Int::sub(&[
            expect_int(left)?,
            expect_int(right)?,
        ])))
    }

    fn mul(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Int(Int::mul(&[
            expect_int(left)?,
            expect_int(right)?,
        ])))
    }

    fn div(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        let (left, right) = (expect_int(left)?, expect_int(right)?);
        let zero = Int::from_i64(0);
        Ok(Z3Term::Int(right.eq(&zero).ite(&zero, &left.div(right))))
    }

    fn modulo(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        let (left, right) = (expect_int(left)?, expect_int(right)?);
        let zero = Int::from_i64(0);
        Ok(Z3Term::Int(right.eq(&zero).ite(&zero, &left.modulo(right))))
    }

    fn lt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(expect_int(left)?.lt(expect_int(right)?)))
    }

    fn le(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(expect_int(left)?.le(expect_int(right)?)))
    }

    fn gt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(expect_int(left)?.gt(expect_int(right)?)))
    }

    fn ge(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Bool(expect_int(left)?.ge(expect_int(right)?)))
    }

    fn const_array(&self, domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term> {
        let domain_sort = z3_sort(domain);
        let ast = match value {
            Z3Term::Bool(value) => Array::const_array(&domain_sort, value),
            Z3Term::Int(value) => Array::const_array(&domain_sort, value),
            Z3Term::Array { ast, .. } => Array::const_array(&domain_sort, ast),
        };
        Ok(Z3Term::Array {
            ast,
            sort: Sort::array(domain.clone(), value.sort()),
        })
    }

    fn select(&self, array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term> {
        let (array, domain, range) = expect_array(array)?;
        if *domain != index.sort() {
            return Err(SolverError::new("array index sort mismatch"));
        }
        term_from_dynamic(&array.select(&index.dynamic()), range)
    }

    fn store(
        &self,
        array: &Self::Term,
        index: &Self::Term,
        value: &Self::Term,
    ) -> SolverResult<Self::Term> {
        let (ast, domain, range) = expect_array(array)?;
        if *domain != index.sort() || *range != value.sort() {
            return Err(SolverError::new("array store sort mismatch"));
        }
        Ok(Z3Term::Array {
            ast: ast.store(&index.dynamic(), &value.dynamic()),
            sort: array.sort(),
        })
    }

    fn substitute(
        &self,
        term: &Self::Term,
        replacements: &[(Self::Term, Self::Term)],
    ) -> SolverResult<Self::Term> {
        if replacements
            .iter()
            .any(|(from, to)| from.sort() != term.sort() || to.sort() != term.sort())
        {
            return Err(SolverError::new("substitution sort mismatch"));
        }
        match term {
            Z3Term::Bool(ast) => {
                let pairs = replacements
                    .iter()
                    .map(|(from, to)| Ok((expect_bool(from)?, expect_bool(to)?)))
                    .collect::<SolverResult<Vec<_>>>()?;
                Ok(Z3Term::Bool(ast.substitute(&pairs)))
            }
            Z3Term::Int(ast) => {
                let pairs = replacements
                    .iter()
                    .map(|(from, to)| Ok((expect_int(from)?, expect_int(to)?)))
                    .collect::<SolverResult<Vec<_>>>()?;
                Ok(Z3Term::Int(ast.substitute(&pairs)))
            }
            Z3Term::Array { ast, sort } => {
                let pairs = replacements
                    .iter()
                    .map(|(from, to)| {
                        let (from, _, _) = expect_array(from)?;
                        let (to, _, _) = expect_array(to)?;
                        Ok((from, to))
                    })
                    .collect::<SolverResult<Vec<_>>>()?;
                Ok(Z3Term::Array {
                    ast: ast.substitute(&pairs),
                    sort: sort.clone(),
                })
            }
        }
    }

    fn push(&mut self) {
        self.moved();
        self.solver.push();
        self.stack_depth += 1;
        self.tracker_marks.push(self.trackers.len());
    }

    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        if levels > self.stack_depth {
            return Err(SolverError::new("solver stack underflow"));
        }
        self.moved();
        self.solver.pop(levels);
        self.stack_depth -= levels;
        let open = self.tracker_marks.len() - levels as usize;
        self.trackers.truncate(self.tracker_marks[open]);
        self.tracker_marks.truncate(open);
        Ok(())
    }

    fn reset(&mut self) -> SolverResult<()> {
        self.moved();
        self.assumptions.clear();
        self.trackers.clear();
        self.tracker_marks.clear();
        self.solver.reset();
        self.stack_depth = 0;
        Ok(())
    }

    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.moved();
        self.solver.assert(expect_bool(term)?);
        Ok(())
    }

    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        let (term, tracker) = (expect_bool(term)?, expect_bool(tracker)?);
        self.moved();
        self.solver.assert_and_track(term.clone(), tracker);
        self.trackers.push(tracker.clone());
        Ok(())
    }

    fn check(&mut self) -> CheckFuture<'_> {
        self.moved();
        self.assumptions.clear();
        self.perturb_history();
        Box::pin(async move {
            let started = std::time::Instant::now();
            let result = map_sat(self.solver.check());
            self.metrics.record_check(
                started.elapsed().as_secs_f64(),
                backend_statistics(&self.solver),
            );
            Ok(result)
        })
    }

    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        let assumptions = assumptions
            .iter()
            .map(|term| expect_bool(term).cloned())
            .collect::<SolverResult<Vec<_>>>();
        self.moved();
        self.assumptions.clear();
        self.perturb_history();
        Box::pin(async move {
            self.assumptions = assumptions?;
            let started = std::time::Instant::now();
            let result = map_sat(self.solver.check_assumptions(&self.assumptions));
            self.metrics.record_check(
                started.elapsed().as_secs_f64(),
                backend_statistics(&self.solver),
            );
            Ok(result)
        })
    }

    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>> {
        Ok(self
            .solver
            .get_unsat_core()
            .into_iter()
            .map(Z3Term::Bool)
            .collect())
    }

    /// Re-solve the current assertions, the last check's assumptions and
    /// the trackers in scope on a new Z3 context, and read the following
    /// model values from its model (#1940). A solver made
    /// [`Z3Solver::verdicts_only`] keeps the check's model.
    fn resolve_output_model(&mut self) -> ModelFuture<'_> {
        Box::pin(async move {
            self.output_model = None;
            if !self.resolves_output || resolve_disabled() {
                return Ok(());
            }
            // `get_assertions` returns a tracked assertion as
            // `tracker => term`; the check assumed its tracker.
            let mut assertions = self.solver.get_assertions();
            assertions.extend(self.assumptions.iter().cloned());
            assertions.extend(self.trackers.iter().cloned());
            self.output_model = resolve_in_fresh_context(&assertions, self.timeout_ms)?;
            Ok(())
        })
    }

    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
        if let Some(model) = &self.output_model {
            return evaluate_model(model, term);
        }
        let model = self
            .solver
            .get_model()
            .ok_or_else(|| SolverError::new("solver model is unavailable"))?;
        evaluate_model(&model, term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on_ready(mut future: CheckFuture<'_>) -> SolverResult<SatResult> {
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => Err(SolverError::new("native Z3 future unexpectedly pending")),
        }
    }

    #[test]
    fn pinned_backend_checks_and_projects_a_model() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        assert!(
            solver
                .version()
                .starts_with(&format!("Z3 {REQUIRED_Z3_VERSION}."))
        );
        assert_eq!(solver.version(), version());
        let x = solver.constant("x", &Sort::Int)?;
        let four = solver.int_value(4);
        let seven = solver.int_value(7);
        let lower = solver.ge(&x, &four)?;
        let upper = solver.lt(&x, &seven)?;
        let bounds = solver.and(&[lower, upper])?;
        solver.assert(&bounds)?;
        solver.set_query_context("invariant", "Bounds");
        let result = block_on_ready(solver.check())?;
        assert_eq!(result, SatResult::Sat);
        let statistics = solver.statistics();
        assert_eq!(statistics.solver.checks, 1);
        assert!(statistics.solver.memory_mb.is_some());
        assert_eq!(statistics.properties[0].kind, "invariant");
        assert_eq!(statistics.properties[0].name, "Bounds");
        let Some(ModelValue::Int(value)) = solver.model_eval(&x)? else {
            return Err(SolverError::new("integer model value was not available"));
        };
        assert!((4..7).contains(&value));
        Ok(())
    }

    /// DESIGN-divmod.md §2.1: `a / 0 = 0` and `a % 0 = 0` must hold for every
    /// integer `a`, not merely by chance for the specific queries a caller
    /// happens to issue. Reverting the `ite(right == 0, 0, ...)` pin in
    /// `div`/`modulo` back to a bare `Int::div`/`Int::modulo` call leaves
    /// zero-divisor results uninterpreted, so the negation below becomes
    /// satisfiable and this assertion fails (issue #477).
    #[test]
    fn div_and_modulo_totalize_the_zero_divisor_for_every_integer() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        let a = solver.constant("a", &Sort::Int)?;
        let zero = solver.int_value(0);
        let quotient = solver.div(&a, &zero)?;
        let remainder = solver.modulo(&a, &zero)?;
        let totalized = solver.and(&[
            solver.equal(&quotient, &zero)?,
            solver.equal(&remainder, &zero)?,
        ])?;
        solver.assert(&solver.not(&totalized)?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Unsat);
        Ok(())
    }

    #[test]
    fn linked_version_uses_the_public_z3_prefix() {
        assert!(version().starts_with("Z3 "));
        assert!(version().starts_with(&format!("Z3 {REQUIRED_Z3_VERSION}.")));
    }

    #[test]
    fn backend_contract_covers_arrays_substitution_stack_and_unsat_core() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        let zero = solver.int_value(0);
        let two = solver.int_value(2);
        let nine = solver.int_value(9);
        let array = solver.const_array(&Sort::Int, &zero)?;
        let array = solver.store(&array, &two, &nine)?;
        let selected = solver.select(&array, &two)?;
        solver.assert(&solver.equal(&selected, &nine)?)?;

        let x = solver.constant("substitution_x", &Sort::Int)?;
        let x_plus_two = solver.add(&x, &two)?;
        let three = solver.int_value(3);
        let substituted = solver.substitute(&x_plus_two, &[(x, three)])?;
        solver.assert(&solver.equal(&substituted, &solver.int_value(5))?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);

        solver.push();
        solver.assert(&solver.bool_value(false))?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Unsat);
        solver.pop(1)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);

        let assumption = solver.constant("assumption", &Sort::Bool)?;
        solver.assert(&solver.not(&assumption)?)?;
        assert_eq!(
            block_on_ready(solver.check_assuming(std::slice::from_ref(&assumption)))?,
            SatResult::Unsat
        );
        assert_eq!(solver.unsat_core()?.len(), 1);
        Ok(())
    }

    #[test]
    fn reset_drops_assertions_and_scopes_but_keeps_terms() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        let x = solver.constant("reset_x", &Sort::Int)?;
        let one = solver.int_value(1);
        solver.assert(&solver.equal(&x, &one)?)?;
        solver.push();
        solver.assert(&solver.equal(&x, &solver.int_value(2))?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Unsat);

        solver.reset()?;
        assert!(solver.pop(1).is_err(), "reset must clear the scope stack");
        solver.assert(&solver.equal(&x, &solver.int_value(2))?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
        assert_eq!(solver.model_eval(&x)?, Some(ModelValue::Int(2)));
        Ok(())
    }
    /// The value one session reads for an under-determined `x`, on its own
    /// context: optionally after a scoped query that pinned `x = 777`, and
    /// optionally re-solved for output (#1940).
    fn chosen_x(history: bool, resolve: bool) -> SolverResult<Option<ModelValue>> {
        with_fresh_context(|| {
            let solver = Z3Solver::new()?;
            let mut solver = if resolve {
                solver
            } else {
                solver.verdicts_only()
            };
            let x = solver.constant("resolve_x", &Sort::Int)?;
            solver.assert(&solver.ge(&x, &solver.int_value(0))?)?;
            solver.assert(&solver.le(&x, &solver.int_value(1000))?)?;
            if history {
                solver.push();
                solver.assert(&solver.equal(&x, &solver.int_value(777))?)?;
                assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
                solver.pop(1)?;
            }
            assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
            block_on_ready_unit(solver.resolve_output_model())?;
            solver.model_eval(&x)
        })
    }

    fn block_on_ready_unit(mut future: ModelFuture<'_>) -> SolverResult<()> {
        use std::task::{Context, Poll, Waker};

        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => Err(SolverError::new("native Z3 future unexpectedly pending")),
        }
    }

    /// A re-solved model does not depend on an earlier scoped query; the
    /// check's own model does (the control, with `verdicts_only`).
    #[test]
    fn resolved_output_model_ignores_the_session_history() -> SolverResult<()> {
        assert_eq!(chosen_x(true, true)?, chosen_x(false, true)?);
        assert_ne!(
            chosen_x(true, false)?,
            chosen_x(false, false)?,
            "control: the scoped query no longer moves the check's model"
        );
        Ok(())
    }

    /// The re-solved model is read only until the session moves: after the
    /// scope is popped and the session checks again, reads come from that
    /// check, not from the model re-solved inside the scope.
    #[test]
    fn the_next_check_drops_the_resolved_model() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        let x = solver.constant("dropped_x", &Sort::Int)?;
        solver.push();
        solver.assert(&solver.equal(&x, &solver.int_value(3))?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
        block_on_ready_unit(solver.resolve_output_model())?;
        assert_eq!(solver.model_eval(&x)?, Some(ModelValue::Int(3)));
        solver.pop(1)?;
        solver.assert(&solver.equal(&x, &solver.int_value(4))?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
        assert_eq!(solver.model_eval(&x)?, Some(ModelValue::Int(4)));
        Ok(())
    }

    /// A tracked assertion reaches the fresh context as `tracker => term`;
    /// the re-solve assumes the tracker, as the check did, so the model still
    /// satisfies the term. A tracker popped with its scope is not assumed.
    #[test]
    fn resolved_output_model_satisfies_tracked_assertions() -> SolverResult<()> {
        let mut solver = Z3Solver::new()?;
        let x = solver.constant("tracked_x", &Sort::Int)?;
        let y = solver.constant("tracked_y", &Sort::Int)?;
        let kept = solver.constant("tracked_kept", &Sort::Bool)?;
        let popped = solver.constant("tracked_popped", &Sort::Bool)?;
        solver.assert_and_track(&solver.equal(&x, &solver.int_value(5))?, &kept)?;
        solver.push();
        solver.assert_and_track(&solver.equal(&y, &solver.int_value(6))?, &popped)?;
        solver.pop(1)?;
        solver.assert(&solver.not(&solver.equal(&y, &solver.int_value(6))?)?)?;
        assert_eq!(block_on_ready(solver.check())?, SatResult::Sat);
        block_on_ready_unit(solver.resolve_output_model())?;
        // Without the tracker assumed, the fresh context sets it false.
        assert_eq!(solver.model_eval(&kept)?, Some(ModelValue::Bool(true)));
        assert_eq!(solver.model_eval(&x)?, Some(ModelValue::Int(5)));
        assert_ne!(solver.model_eval(&y)?, Some(ModelValue::Int(6)));
        Ok(())
    }
}
