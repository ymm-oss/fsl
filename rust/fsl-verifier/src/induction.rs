// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FslValue, KernelModel, TypeDef, TypeRef};
use fsl_solver::{ModelValue, SatResult, SmtSolver};

use crate::VerifyError;
use crate::bmc::{leadsto_bindings, leadsto_condition, project_trace};
use crate::eval::eval;
use crate::transition::{action_instances, transition_constraint};
use crate::value::{
    Bindings, SymbolicState, bool_term, bounds, i64_index, int_term, symbolic_state_with_suffix,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InductionCti {
    pub kind: String,
    pub name: String,
    pub k: usize,
    pub trace: Vec<fsl_core::TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InductionResult {
    pub k_used: BTreeMap<String, usize>,
    pub cti: Option<InductionCti>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankProof {
    pub name: String,
    pub measure: fsl_core::KernelExpr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankFailure {
    pub name: String,
    pub bindings: BTreeMap<String, FslValue>,
    pub measure: fsl_core::KernelExpr,
    pub kind: String,
    pub measure_value: Option<i64>,
    pub measure_before: Option<i64>,
    pub measure_after: Option<i64>,
    pub action: Option<String>,
    pub trace: Vec<fsl_core::TraceStep>,
    pub hint: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankedLeadstoResult {
    pub proofs: Vec<RankProof>,
    pub failure: Option<RankFailure>,
}

#[derive(Clone, Copy)]
enum Property<'a> {
    Bound(&'a str),
    Invariant(usize),
}

impl Property<'_> {
    fn name(self, model: &KernelModel) -> String {
        match self {
            Self::Bound(name) => format!("_bounds_{name}"),
            Self::Invariant(index) => model.invariants[index].name.clone(),
        }
    }
}

fn has_bounds(model: &KernelModel, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int | TypeRef::Bool | TypeRef::Relation(_, _) => false,
        TypeRef::Range(_, _) | TypeRef::Set(_) | TypeRef::Seq(_, _) => true,
        TypeRef::Option(inner) => has_bounds(model, inner),
        TypeRef::Map(_, value) => has_bounds(model, value),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
            Some(TypeDef::Struct { fields }) => fields.iter().any(|(_, ty)| has_bounds(model, ty)),
            None => false,
        },
    }
}

fn properties(model: &KernelModel) -> Vec<Property<'_>> {
    let mut properties = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| Property::Bound(name.as_str()))
        .collect::<Vec<_>>();
    properties.extend((0..model.invariants.len()).map(Property::Invariant));
    properties
}

fn property_condition<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    property: Property<'_>,
    state: &SymbolicState<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<S::Term, VerifyError> {
    match property {
        Property::Bound(name) => Ok(bounds(
            solver,
            model,
            state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
        )?),
        Property::Invariant(index) => {
            let mut bindings = Bindings::new();
            let value = eval(
                solver,
                model,
                &model.invariants[index].expr,
                state,
                &mut bindings,
                old_state,
            )?;
            Ok(bool_term(&value)?.clone())
        }
    }
}

/// Prove kernel invariants and transitions by k-induction after a successful
/// bounded base case.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported symbolic expressions or solver
/// failures.
pub async fn prove_induction<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    k_ind: usize,
) -> Result<InductionResult, VerifyError> {
    let instances = action_instances(solver, model)?;
    if instances.is_empty() {
        return Err(VerifyError::new("spec declares no action instances"));
    }
    let properties = properties(model);
    let mut remaining = properties.clone();
    let mut k_used = BTreeMap::new();
    let mut last_cti = None;
    let mut states = Vec::new();
    let mut choices = Vec::new();

    for k in 1..=k_ind {
        if k == 1 {
            states.push(symbolic_state_with_suffix(solver, model, "ind0")?);
        }
        let previous = &states[k - 1];
        for property in &properties {
            let assumption = property_condition(solver, model, *property, previous, None)?;
            solver.assert(&assumption)?;
        }
        let next = symbolic_state_with_suffix(solver, model, &format!("ind{k}"))?;
        let choice = solver.constant(&format!("__ind_choice@{}", k - 1), &fsl_solver::Sort::Int)?;
        solver.assert(&solver.ge(&choice, &solver.int_value(0))?)?;
        solver.assert(&solver.lt(&choice, &solver.int_value(i64_index(instances.len())?))?)?;
        solver.assert(&transition_constraint(
            solver, model, &instances, previous, &next, &choice,
        )?)?;
        states.push(next);
        choices.push(choice);

        let mut still_remaining = Vec::new();
        for property in remaining {
            let condition = property_condition(solver, model, property, &states[k], None)?;
            solver.push();
            solver.assert(&solver.not(&condition)?)?;
            match solver.check().await? {
                SatResult::Unsat => {
                    k_used.insert(property.name(model), k);
                }
                SatResult::Sat => {
                    let trace = project_trace(solver, model, &states, &choices, &instances, k)?;
                    last_cti = Some(InductionCti {
                        kind: "invariant".to_owned(),
                        name: property.name(model),
                        k,
                        trace,
                    });
                    still_remaining.push(property);
                }
                SatResult::Unknown => {
                    solver.pop(1)?;
                    return Err(VerifyError::new("solver returned unknown in induction"));
                }
            }
            solver.pop(1)?;
        }

        if k == 1 {
            for transition in &model.transitions {
                let mut bindings = Bindings::new();
                let value = eval(
                    solver,
                    model,
                    &transition.expr,
                    &states[1],
                    &mut bindings,
                    Some(&states[0]),
                )?;
                solver.push();
                solver.assert(&solver.not(bool_term(&value)?)?)?;
                match solver.check().await? {
                    SatResult::Sat => {
                        let trace = project_trace(solver, model, &states, &choices, &instances, 1)?;
                        solver.pop(1)?;
                        return Ok(InductionResult {
                            k_used,
                            cti: Some(InductionCti {
                                kind: "trans".to_owned(),
                                name: transition.name.clone(),
                                k: 1,
                                trace,
                            }),
                        });
                    }
                    SatResult::Unsat => solver.pop(1)?,
                    SatResult::Unknown => {
                        solver.pop(1)?;
                        return Err(VerifyError::new("solver returned unknown in induction"));
                    }
                }
            }
        }

        remaining = still_remaining;
        if remaining.is_empty() {
            return Ok(InductionResult { k_used, cti: None });
        }
    }

    Ok(InductionResult {
        k_used,
        cti: last_cti,
    })
}

fn model_int<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<i64, VerifyError> {
    match solver.model_eval(term)? {
        Some(ModelValue::Int(value)) => Ok(value),
        Some(ModelValue::Bool(_)) => Err(VerifyError::new("ranking measure is Boolean")),
        None => Err(VerifyError::new("ranking measure is unavailable in model")),
    }
}

/// Prove explicitly ranked `leadsTo` properties over one arbitrary transition.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported measures, symbolic expressions, or
/// solver failures.
#[allow(clippy::too_many_lines)]
pub async fn prove_ranked_leadstos<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
) -> Result<RankedLeadstoResult, VerifyError> {
    let instances = action_instances(solver, model)?;
    let state0 = symbolic_state_with_suffix(solver, model, "rank0")?;
    let state1 = symbolic_state_with_suffix(solver, model, "rank1")?;
    for property in properties(model) {
        solver.assert(&property_condition(solver, model, property, &state0, None)?)?;
    }
    let choice = solver.constant("__rank_choice", &fsl_solver::Sort::Int)?;
    solver.assert(&solver.ge(&choice, &solver.int_value(0))?)?;
    solver.assert(&solver.lt(&choice, &solver.int_value(i64_index(instances.len())?))?)?;
    solver.assert(&transition_constraint(
        solver, model, &instances, &state0, &state1, &choice,
    )?)?;

    let mut proofs = Vec::new();
    for property in &model.leadstos {
        let Some(measure_expr) = &property.decreases else {
            continue;
        };
        for binding in leadsto_bindings(solver, model, property)? {
            let p0 =
                leadsto_condition(solver, model, &property.before, &state0, &binding.symbolic)?;
            let q0 = leadsto_condition(solver, model, &property.after, &state0, &binding.symbolic)?;
            let pending = solver.and(&[p0, solver.not(&q0)?])?;
            let mut measure_bindings = binding.symbolic.clone();
            let measure0 = eval(
                solver,
                model,
                measure_expr,
                &state0,
                &mut measure_bindings,
                None,
            )?;
            let measure0 = int_term(&measure0)?.clone();

            solver.push();
            solver.assert(&pending)?;
            solver.assert(&solver.lt(&measure0, &solver.int_value(0))?)?;
            match solver.check().await? {
                SatResult::Sat => {
                    let measure_value = model_int(solver, &measure0)?;
                    let trace = project_trace(solver, model, &[state0], &[], &instances, 0)?;
                    solver.pop(1)?;
                    return Ok(RankedLeadstoResult {
                        proofs,
                        failure: Some(RankFailure {
                            name: property.name.clone(),
                            bindings: binding.concrete,
                            measure: measure_expr.clone(),
                            kind: "unbounded_below".to_owned(),
                            measure_value: Some(measure_value),
                            measure_before: None,
                            measure_after: None,
                            action: None,
                            trace,
                            hint: "the decreases measure must be non-negative whenever the leadsTo trigger is pending (P holds and Q is false); add an invariant or use a bounded domain that proves the measure is >= 0".to_owned(),
                            message: format!(
                                "leadsTo '{}' decreases measure can be negative while P holds and Q is false",
                                property.name
                            ),
                        }),
                    });
                }
                SatResult::Unsat => solver.pop(1)?,
                SatResult::Unknown => {
                    solver.pop(1)?;
                    return Err(VerifyError::new("solver returned unknown in ranking proof"));
                }
            }

            let p1 =
                leadsto_condition(solver, model, &property.before, &state1, &binding.symbolic)?;
            let q1 = leadsto_condition(solver, model, &property.after, &state1, &binding.symbolic)?;
            let mut next_bindings = binding.symbolic.clone();
            let measure1 = eval(
                solver,
                model,
                measure_expr,
                &state1,
                &mut next_bindings,
                None,
            )?;
            let measure1 = int_term(&measure1)?.clone();
            let decreases = solver.lt(&measure1, &measure0)?;
            let keeps_pending = solver.and(&[p1, decreases])?;
            let progresses = solver.or(&[q1, keeps_pending])?;

            for (index, instance) in instances.iter().enumerate() {
                let selected = solver.equal(&choice, &solver.int_value(i64_index(index)?))?;
                let failure = solver.and(&[pending.clone(), selected, solver.not(&progresses)?])?;
                solver.push();
                solver.assert(&failure)?;
                match solver.check().await? {
                    SatResult::Sat => {
                        let before = model_int(solver, &measure0)?;
                        let after = model_int(solver, &measure1)?;
                        let trace = project_trace(
                            solver,
                            model,
                            &[state0, state1],
                            std::slice::from_ref(&choice),
                            &instances,
                            1,
                        )?;
                        solver.pop(1)?;
                        return Ok(RankedLeadstoResult {
                            proofs,
                            failure: Some(RankFailure {
                                name: property.name.clone(),
                                bindings: binding.concrete,
                                measure: measure_expr.clone(),
                                kind: "non_decreasing_action".to_owned(),
                                measure_value: None,
                                measure_before: Some(before),
                                measure_after: Some(after),
                                action: Some(instance.action.clone()),
                                trace,
                                hint: "from every state where P holds and Q is false, each enabled action must either make Q true, or keep P true and strictly decrease the measure".to_owned(),
                                message: format!(
                                    "enabled action '{}' can leave leadsTo '{}' pending without strictly decreasing the measure",
                                    instance.action, property.name
                                ),
                            }),
                        });
                    }
                    SatResult::Unsat => solver.pop(1)?,
                    SatResult::Unknown => {
                        solver.pop(1)?;
                        return Err(VerifyError::new("solver returned unknown in ranking proof"));
                    }
                }
            }
        }
        proofs.push(RankProof {
            name: property.name.clone(),
            measure: measure_expr.clone(),
        });
    }
    Ok(RankedLeadstoResult {
        proofs,
        failure: None,
    })
}
