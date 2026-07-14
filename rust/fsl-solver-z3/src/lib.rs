// SPDX-License-Identifier: Apache-2.0

//! Native Z3 implementation of the backend-neutral FSL SMT contract.

use fsl_solver::{CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort};
use z3::ast::{Array, Ast, Bool, Dynamic, Int};
use z3::{Model, Solver};

const REQUIRED_Z3_VERSION: &str = "4.16.0";

/// Return the version reported by the linked Z3 library.
#[must_use]
pub fn version() -> &'static str {
    z3::full_version()
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
}

impl Z3Solver {
    /// Construct a backend, rejecting a library that does not satisfy the pin.
    ///
    /// # Errors
    ///
    /// Returns an error when the loaded Z3 library is not version 4.16.0.
    pub fn new() -> SolverResult<Self> {
        let version = version().to_owned();
        let required_prefix = format!("Z3 {REQUIRED_Z3_VERSION}.");
        if !version.starts_with(&required_prefix) {
            return Err(SolverError::new(format!(
                "expected Z3 {REQUIRED_Z3_VERSION}, loaded {version}"
            )));
        }
        Ok(Self {
            solver: Solver::new(),
            version,
            stack_depth: 0,
        })
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

    fn version(&self) -> &str {
        &self.version
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
        Ok(Z3Term::Int(expect_int(left)?.div(expect_int(right)?)))
    }

    fn modulo(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        Ok(Z3Term::Int(expect_int(left)?.modulo(expect_int(right)?)))
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
        self.solver.push();
        self.stack_depth += 1;
    }

    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        if levels > self.stack_depth {
            return Err(SolverError::new("solver stack underflow"));
        }
        self.solver.pop(levels);
        self.stack_depth -= levels;
        Ok(())
    }

    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.solver.assert(expect_bool(term)?);
        Ok(())
    }

    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        self.solver
            .assert_and_track(expect_bool(term)?.clone(), expect_bool(tracker)?);
        Ok(())
    }

    fn check(&mut self) -> CheckFuture<'_> {
        Box::pin(async move { Ok(map_sat(self.solver.check())) })
    }

    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        let assumptions = assumptions
            .iter()
            .map(|term| expect_bool(term).cloned())
            .collect::<SolverResult<Vec<_>>>();
        Box::pin(async move { Ok(map_sat(self.solver.check_assumptions(&assumptions?))) })
    }

    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>> {
        Ok(self
            .solver
            .get_unsat_core()
            .into_iter()
            .map(Z3Term::Bool)
            .collect())
    }

    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
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
        let result = block_on_ready(solver.check())?;
        assert_eq!(result, SatResult::Sat);
        let Some(ModelValue::Int(value)) = solver.model_eval(&x)? else {
            return Err(SolverError::new("integer model value was not available"));
        };
        assert!((4..7).contains(&value));
        Ok(())
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
}
