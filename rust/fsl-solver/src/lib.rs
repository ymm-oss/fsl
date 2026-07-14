// SPDX-License-Identifier: Apache-2.0

//! Backend-neutral SMT vocabulary used by both native and browser verifiers.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

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

/// Backend-neutral statistics observed across satisfiability checks.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SolverStatistics {
    pub checks: u64,
    pub check_elapsed_s: f64,
    pub conflicts: Option<u64>,
    pub decisions: Option<u64>,
    pub propagations: Option<u64>,
    pub memory_mb: Option<f64>,
}

/// Solver-check cost attributed to one semantic verification concern.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PropertyStatistics {
    pub kind: String,
    pub name: String,
    pub checks: u64,
    pub elapsed_s: f64,
}

/// Complete solver cost with deterministic property ordering.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct VerificationStatistics {
    pub solver: SolverStatistics,
    pub properties: Vec<PropertyStatistics>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct VerificationCost<'a> {
    pub elapsed_s: f64,
    pub solver: &'a SolverStatistics,
    pub properties: &'a [PropertyStatistics],
}

impl VerificationStatistics {
    #[must_use]
    pub fn with_elapsed(&self, elapsed_s: f64) -> VerificationCost<'_> {
        VerificationCost {
            elapsed_s,
            solver: &self.solver,
            properties: &self.properties,
        }
    }

    /// Merge independent solver sessions into one verification cost.
    pub fn merge(&mut self, other: &Self) {
        self.solver.checks += other.solver.checks;
        self.solver.check_elapsed_s += other.solver.check_elapsed_s;
        self.solver.conflicts = option_max(self.solver.conflicts, other.solver.conflicts);
        self.solver.decisions = option_max(self.solver.decisions, other.solver.decisions);
        self.solver.propagations = option_max(self.solver.propagations, other.solver.propagations);
        self.solver.memory_mb = option_max_f64(self.solver.memory_mb, other.solver.memory_mb);

        let mut properties = self
            .properties
            .drain(..)
            .map(|property| ((property.kind.clone(), property.name.clone()), property))
            .collect::<std::collections::BTreeMap<_, _>>();
        for property in &other.properties {
            let entry = properties
                .entry((property.kind.clone(), property.name.clone()))
                .or_insert_with(|| PropertyStatistics {
                    kind: property.kind.clone(),
                    name: property.name.clone(),
                    ..PropertyStatistics::default()
                });
            entry.checks += property.checks;
            entry.elapsed_s += property.elapsed_s;
        }
        self.properties = properties.into_values().collect();
    }
}

fn option_max(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

fn option_max_f64(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

/// Raw common statistics extracted from one backend check.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BackendStatistics {
    pub conflicts: Option<u64>,
    pub decisions: Option<u64>,
    pub propagations: Option<u64>,
    pub memory_mb: Option<f64>,
}

/// Shared metrics accumulator used by native and browser solver adapters.
#[derive(Clone, Debug)]
pub struct SolverMetrics {
    current: (String, String),
    solver: SolverStatistics,
    properties: std::collections::BTreeMap<(String, String), PropertyStatistics>,
}

impl Default for SolverMetrics {
    fn default() -> Self {
        Self {
            current: ("solver".to_owned(), "unattributed".to_owned()),
            solver: SolverStatistics::default(),
            properties: std::collections::BTreeMap::new(),
        }
    }
}

impl SolverMetrics {
    pub fn set_context(&mut self, kind: &str, name: &str) {
        self.current = (kind.to_owned(), name.to_owned());
    }

    pub fn record_check(&mut self, elapsed_s: f64, backend: BackendStatistics) {
        self.solver.checks += 1;
        self.solver.check_elapsed_s += elapsed_s;
        self.solver.conflicts = option_max(self.solver.conflicts, backend.conflicts);
        self.solver.decisions = option_max(self.solver.decisions, backend.decisions);
        self.solver.propagations = option_max(self.solver.propagations, backend.propagations);
        self.solver.memory_mb = option_max_f64(self.solver.memory_mb, backend.memory_mb);
        let entry = self
            .properties
            .entry(self.current.clone())
            .or_insert_with(|| PropertyStatistics {
                kind: self.current.0.clone(),
                name: self.current.1.clone(),
                ..PropertyStatistics::default()
            });
        entry.checks += 1;
        entry.elapsed_s += elapsed_s;
    }

    #[must_use]
    pub fn statistics(&self) -> VerificationStatistics {
        VerificationStatistics {
            solver: self.solver.clone(),
            properties: self.properties.values().cloned().collect(),
        }
    }
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
    fn set_query_context(&mut self, kind: &str, name: &str);
    fn statistics(&self) -> VerificationStatistics;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_keep_a_fixed_shape_and_deterministic_property_order() {
        let mut metrics = SolverMetrics::default();
        metrics.set_context("invariant", "Zulu");
        metrics.record_check(
            0.2,
            BackendStatistics {
                conflicts: Some(3),
                decisions: None,
                propagations: Some(7),
                memory_mb: Some(4.5),
            },
        );
        metrics.set_context("invariant", "Alpha");
        metrics.record_check(
            0.1,
            BackendStatistics {
                conflicts: Some(5),
                decisions: Some(11),
                propagations: Some(2),
                memory_mb: Some(4.0),
            },
        );

        let statistics = metrics.statistics();
        assert_eq!(statistics.solver.checks, 2);
        assert_eq!(statistics.solver.conflicts, Some(5));
        assert_eq!(statistics.solver.decisions, Some(11));
        assert_eq!(statistics.solver.propagations, Some(7));
        assert_eq!(statistics.solver.memory_mb, Some(4.5));
        assert_eq!(statistics.properties[0].name, "Alpha");
        assert_eq!(statistics.properties[1].name, "Zulu");
        assert_eq!(
            statistics
                .properties
                .iter()
                .map(|property| property.checks)
                .sum::<u64>(),
            statistics.solver.checks
        );
    }

    #[test]
    fn independent_solver_sessions_merge_by_property_and_maximum_observation() {
        let mut left = VerificationStatistics {
            solver: SolverStatistics {
                checks: 1,
                check_elapsed_s: 0.1,
                conflicts: Some(2),
                ..SolverStatistics::default()
            },
            properties: vec![PropertyStatistics {
                kind: "invariant".to_owned(),
                name: "Safe".to_owned(),
                checks: 1,
                elapsed_s: 0.1,
            }],
        };
        let right = VerificationStatistics {
            solver: SolverStatistics {
                checks: 2,
                check_elapsed_s: 0.2,
                conflicts: Some(5),
                ..SolverStatistics::default()
            },
            properties: vec![PropertyStatistics {
                kind: "invariant".to_owned(),
                name: "Safe".to_owned(),
                checks: 2,
                elapsed_s: 0.2,
            }],
        };

        left.merge(&right);

        assert_eq!(left.solver.checks, 3);
        assert_eq!(left.solver.conflicts, Some(5));
        assert_eq!(left.properties[0].checks, 3);
    }
}
