// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelExpr, KernelModel};
use fsl_solver::{SatResult, SmtSolver};

use crate::VerifyError;
use crate::eval::eval;
use crate::value::{
    Bindings, bool_term, bounds, concrete_value, logical_equal, project_state,
    symbolic_state_with_suffix,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImplicationResult {
    Implied,
    Counterexample(BTreeMap<String, FslValue>),
}

/// Pin a symbolic state to one concrete Monitor state and prove that evaluating
/// `expression` cannot differ from the concrete evaluator's `expected` value.
///
/// This is the reusable half of the dual-evaluator agreement gate. Keeping the
/// concrete evaluator outside this crate preserves the solver-free runtime
/// dependency boundary.
///
/// # Errors
///
/// Returns [`VerifyError`] for incomplete states, ill-typed values/expressions,
/// or an unknown solver result.
pub async fn expression_matches_value<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    expression: &KernelExpr,
    state: &BTreeMap<String, FslValue>,
    expected: &FslValue,
) -> Result<bool, VerifyError> {
    let symbolic = symbolic_state_with_suffix(solver, model, "agreement")?;
    for (name, ty) in &model.state {
        let value = state
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("agreement state is missing '{name}'")))?;
        let concrete = concrete_value(solver, model, ty, value)?;
        let symbolic_value = symbolic
            .get(name)
            .ok_or_else(|| VerifyError::new(format!("symbolic state is missing '{name}'")))?;
        solver.assert(&logical_equal(solver, model, symbolic_value, &concrete)?)?;
    }
    if state.len() != model.state.len() {
        return Err(VerifyError::new(
            "agreement state contains an unknown state variable",
        ));
    }
    let actual = eval(
        solver,
        model,
        expression,
        &symbolic,
        &mut Bindings::new(),
        None,
    )?;
    let ty = actual
        .ty()
        .ok_or_else(|| VerifyError::new("agreement expression has no concrete type"))?;
    let expected = concrete_value(solver, model, ty, expected)?;
    let equal = logical_equal(solver, model, &actual, &expected)?;
    solver.assert(&solver.not(&equal)?)?;
    match solver.check().await? {
        SatResult::Unsat => Ok(true),
        SatResult::Sat => Ok(false),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in expression agreement",
        )),
    }
}

/// Decide whether every type-correct state satisfying `antecedent` invariants
/// also satisfies `consequent` invariants, returning a concrete counterexample
/// when implication fails.
///
/// # Errors
///
/// Returns [`VerifyError`] for incompatible state schemas, symbolic evaluation
/// failures, or an unknown solver result.
pub async fn invariant_implication<S: SmtSolver>(
    antecedent: &KernelModel,
    consequent: &KernelModel,
    solver: &mut S,
) -> Result<ImplicationResult, VerifyError> {
    if antecedent.state != consequent.state {
        return Err(VerifyError::new("invariant state schemas differ"));
    }
    let state = symbolic_state_with_suffix(solver, consequent, "implication")?;
    for (name, _) in &consequent.state {
        solver.assert(&bounds(
            solver,
            consequent,
            state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
        )?)?;
    }
    for property in &antecedent.invariants {
        let value = eval(
            solver,
            antecedent,
            &property.expr,
            &state,
            &mut Bindings::new(),
            None,
        )?;
        solver.assert(bool_term(&value)?)?;
    }
    let mut consequents = Vec::new();
    for property in &consequent.invariants {
        let value = eval(
            solver,
            consequent,
            &property.expr,
            &state,
            &mut Bindings::new(),
            None,
        )?;
        consequents.push(bool_term(&value)?.clone());
    }
    let conjunction = solver.and(&consequents)?;
    solver.assert(&solver.not(&conjunction)?)?;
    match solver.check().await? {
        SatResult::Unsat => Ok(ImplicationResult::Implied),
        SatResult::Sat => Ok(ImplicationResult::Counterexample(project_state(
            solver, consequent, &state,
        )?)),
        SatResult::Unknown => Err(VerifyError::new(
            "solver returned unknown in invariant implication",
        )),
    }
}
