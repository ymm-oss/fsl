// SPDX-License-Identifier: Apache-2.0

//! Backend-neutral SMT vocabulary used by both native and browser verifiers.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

/// An SMT sort required by the FSL verifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sort {
    Bool,
    Int,
    Array { domain: Box<Self>, range: Box<Self> },
}

impl Sort {
    #[must_use]
    pub fn array(domain: Self, range: Self) -> Self {
        Self::Array {
            domain: Box::new(domain),
            range: Box::new(range),
        }
    }
}

/// A scalar value projected from a solver model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelValue {
    Bool(bool),
    Int(i64),
}

/// Result of a satisfiability query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatResult {
    Sat,
    Unsat,
    Unknown,
}

/// A backend-independent solver failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolverError {
    message: String,
}

impl SolverError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SolverError {}

pub type SolverResult<T> = Result<T, SolverError>;
pub type CheckFuture<'a> = Pin<Box<dyn Future<Output = SolverResult<SatResult>> + 'a>>;

/// Minimal solver contract shared by native Z3 and browser Worker backends.
///
/// Term construction is synchronous. Only satisfiability checks may cross an
/// asynchronous boundary, which is the boundary needed by the Worker backend.
///
/// # Errors
///
/// Fallible methods reject ill-sorted terms, invalid stack operations, backend
/// failures, or unavailable solver results.
#[allow(clippy::missing_errors_doc)]
pub trait SmtSolver {
    type Term: Clone + fmt::Debug;

    fn version(&self) -> &str;
    fn sort(&self, term: &Self::Term) -> Sort;

    fn bool_value(&self, value: bool) -> Self::Term;
    fn int_value(&self, value: i64) -> Self::Term;
    fn constant(&self, name: &str, sort: &Sort) -> SolverResult<Self::Term>;

    fn not(&self, term: &Self::Term) -> SolverResult<Self::Term>;
    fn and(&self, terms: &[Self::Term]) -> SolverResult<Self::Term>;
    fn or(&self, terms: &[Self::Term]) -> SolverResult<Self::Term>;
    fn implies(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn equal(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn ite(
        &self,
        condition: &Self::Term,
        then_term: &Self::Term,
        else_term: &Self::Term,
    ) -> SolverResult<Self::Term>;

    fn neg(&self, term: &Self::Term) -> SolverResult<Self::Term>;
    fn add(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn sub(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn mul(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn div(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn modulo(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn lt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn le(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn gt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
    fn ge(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;

    fn const_array(&self, domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term>;
    fn select(&self, array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term>;
    fn store(
        &self,
        array: &Self::Term,
        index: &Self::Term,
        value: &Self::Term,
    ) -> SolverResult<Self::Term>;
    fn substitute(
        &self,
        term: &Self::Term,
        replacements: &[(Self::Term, Self::Term)],
    ) -> SolverResult<Self::Term>;

    fn push(&mut self);
    fn pop(&mut self, levels: u32) -> SolverResult<()>;
    fn assert(&mut self, term: &Self::Term) -> SolverResult<()>;
    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()>;
    fn check(&mut self) -> CheckFuture<'_>;
    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_>;
    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>>;
    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>>;
}
