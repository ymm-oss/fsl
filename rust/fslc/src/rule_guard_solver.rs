// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `check`'s solver for the rule-guard problems the core's enumeration
//! leaves undecided (decisions/048 J3).
//!
//! The lowering decides whether the guards rules add to an event can hold
//! together by enumerating the event's finite type domains
//! ([`fsl_core::GuardProblem`]). Past [`fsl_core::ENUMERATION_BOUND`]
//! assignments it reports nothing, and `check` asks a solver the same
//! question here: the same variables over the same domains, the same guards,
//! the same minimal conflicting subset. Enum members are numbered, so two
//! members compare equal only when they are spelled the same, as in the
//! enumeration.

use std::collections::BTreeMap;

use fsl_core::{FiniteDomain, FiniteValue, GuardDecision, GuardProblem};
use fsl_solver::{SatResult, SmtSolver, SolverResult, Sort};
use fsl_syntax::{Expr, Span};

/// The first event whose rule guards a solver finds unsatisfiable, among the
/// problems beyond the enumeration bound: the diagnostic and its span.
///
/// # Errors
///
/// Returns the solver's failure.
pub async fn unsatisfiable_rule_guard<S: SmtSolver>(
    problems: &[GuardProblem],
    solver: &mut S,
) -> SolverResult<Option<(String, Span)>> {
    for problem in problems {
        if problem.decide_by_enumeration(fsl_core::ENUMERATION_BOUND) != GuardDecision::BeyondBound
        {
            continue;
        }
        solver.push();
        let core = problem_core(problem, solver).await;
        solver.pop(1)?;
        if let Some(core) = core? {
            return Ok(Some((problem.unsatisfiable_message(&core), problem.span)));
        }
    }
    Ok(None)
}

struct Encoder<'a, S: SmtSolver> {
    solver: &'a S,
    variables: BTreeMap<&'a str, S::Term>,
    problem: &'a GuardProblem,
    members: BTreeMap<String, i64>,
}

impl<S: SmtSolver> Encoder<'_, S> {
    fn member(&mut self, name: &str) -> S::Term {
        let next = i64::try_from(self.members.len()).unwrap_or(i64::MAX);
        let id = *self.members.entry(name.to_owned()).or_insert(next);
        self.solver.int_value(id)
    }

    fn value(&mut self, value: &FiniteValue) -> S::Term {
        match value {
            FiniteValue::Bool(value) => self.solver.bool_value(*value),
            FiniteValue::Int(value) => self.solver.int_value(*value),
            FiniteValue::Member(name) => self.member(name),
        }
    }

    /// The term of `expression` and the divisors it divides by, which the
    /// enumeration treats as undefined at zero.
    fn term(&mut self, expression: &Expr, divisors: &mut Vec<S::Term>) -> SolverResult<S::Term> {
        let solver = self.solver;
        Ok(match expression {
            Expr::Num(value) => solver.int_value(*value),
            Expr::Bool(value) => solver.bool_value(*value),
            Expr::Var(name) => {
                if let Some(term) = self.variables.get(name.as_str()) {
                    term.clone()
                } else if let Some(value) = self.problem.constants.get(name) {
                    self.value(&value.clone())
                } else {
                    self.member(name)
                }
            }
            Expr::Binary { op, left, right } => {
                let left = self.term(left, divisors)?;
                let right = self.term(right, divisors)?;
                match op.as_str() {
                    "and" => solver.and(&[left, right])?,
                    "or" => solver.or(&[left, right])?,
                    "=>" => solver.implies(&left, &right)?,
                    "==" => solver.equal(&left, &right)?,
                    "!=" => solver.not(&solver.equal(&left, &right)?)?,
                    "<" => solver.lt(&left, &right)?,
                    "<=" => solver.le(&left, &right)?,
                    ">" => solver.gt(&left, &right)?,
                    ">=" => solver.ge(&left, &right)?,
                    "+" => solver.add(&left, &right)?,
                    "-" => solver.sub(&left, &right)?,
                    "*" => solver.mul(&left, &right)?,
                    "/" | "%" => {
                        divisors.push(right.clone());
                        if op == "/" {
                            solver.div(&left, &right)?
                        } else {
                            solver.modulo(&left, &right)?
                        }
                    }
                    other => {
                        return Err(fsl_solver::SolverError::new(format!(
                            "operator '{other}' is outside the finite fragment"
                        )));
                    }
                }
            }
            Expr::Not(value) => {
                let value = self.term(value, divisors)?;
                solver.not(&value)?
            }
            Expr::Neg(value) => {
                let value = self.term(value, divisors)?;
                solver.neg(&value)?
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                let condition = self.term(condition, divisors)?;
                let then_term = self.term(then_expr, divisors)?;
                let else_term = self.term(else_expr, divisors)?;
                solver.ite(&condition, &then_term, &else_term)?
            }
            _ => {
                return Err(fsl_solver::SolverError::new(
                    "expression outside the finite fragment",
                ));
            }
        })
    }
}

/// A minimal unsatisfiable subset of `problem`'s guards, or `None` when they
/// can hold together (or the solver cannot tell).
async fn problem_core<S: SmtSolver>(
    problem: &GuardProblem,
    solver: &mut S,
) -> SolverResult<Option<Vec<usize>>> {
    let Some(variables) = &problem.variables else {
        return Ok(None);
    };
    let mut encoder = Encoder {
        solver: &*solver,
        variables: BTreeMap::new(),
        problem,
        members: BTreeMap::new(),
    };
    let mut domains = Vec::new();
    for (name, domain) in variables {
        let sort = if *domain == FiniteDomain::Bool {
            Sort::Bool
        } else {
            Sort::Int
        };
        let term = encoder
            .solver
            .constant(&format!("rule_guard_{name}"), &sort)?;
        match domain {
            FiniteDomain::Bool => {}
            FiniteDomain::Int { lo, hi } => {
                let lo = encoder.solver.int_value(*lo);
                let hi = encoder.solver.int_value(*hi);
                domains.push(encoder.solver.le(&lo, &term)?);
                domains.push(encoder.solver.le(&term, &hi)?);
            }
            FiniteDomain::Members(members) => {
                let mut choices = Vec::new();
                for member in members {
                    let id = encoder.member(member);
                    choices.push(encoder.solver.equal(&term, &id)?);
                }
                domains.push(encoder.solver.or(&choices)?);
            }
        }
        encoder.variables.insert(name.as_str(), term);
    }
    // Each guard is tracked by a literal, so a subset is asked about by
    // assuming its literals.
    let mut guards = Vec::new();
    for (index, guard) in problem.guards.iter().enumerate() {
        let mut divisors = Vec::new();
        let term = encoder.term(&guard.condition, &mut divisors)?;
        let mut parts = vec![term];
        for divisor in divisors {
            let zero = encoder.solver.int_value(0);
            parts.push(
                encoder
                    .solver
                    .not(&encoder.solver.equal(&divisor, &zero)?)?,
            );
        }
        let tracker = encoder
            .solver
            .constant(&format!("rule_guard_track_{index}"), &Sort::Bool)?;
        domains.push(
            encoder
                .solver
                .implies(&tracker, &encoder.solver.and(&parts)?)?,
        );
        guards.push(tracker);
    }
    for domain in &domains {
        solver.assert(domain)?;
    }
    let mut kept = (0..guards.len()).collect::<Vec<_>>();
    let assumptions = |kept: &[usize]| {
        kept.iter()
            .map(|index| guards[*index].clone())
            .collect::<Vec<_>>()
    };
    if solver.check_assuming(&assumptions(&kept)).await? != SatResult::Unsat {
        return Ok(None);
    }
    let mut index = 0;
    while index < kept.len() {
        let without = kept
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != index)
            .map(|(_, guard)| *guard)
            .collect::<Vec<_>>();
        if solver.check_assuming(&assumptions(&without)).await? == SatResult::Unsat {
            kept = without;
        } else {
            index += 1;
        }
    }
    Ok(Some(kept))
}
