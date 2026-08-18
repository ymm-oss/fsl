// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{FslValue, KernelModel, LeadsToDef, TraceAction, TraceStep};
use fsl_solver::{SatResult, SmtSolver};

use crate::VerifyError;
use crate::eval::{eval, evaluation_status, property_evaluation_status};
use crate::liveness::{LeadstoBinding, leadsto_bindings, leadsto_condition};
use crate::symmetry::canonical_constraint;
use crate::trace::project_trace;
use crate::transition::{
    ActionInstance, action_guard_definedness, action_guards,
    action_has_partial_operation_candidate, action_instances, action_statements_evaluation_status,
    init_constraints, transition_constraint,
};
use crate::vacuity::{VacuityFinding, retain_covered, static_findings};
use crate::value::{
    Bindings, SymbolicState, bool_term, bounds, concrete_value, i64_index, logical_equal,
    symbolic_state, symbolic_state_with_suffix,
};
use crate::violation_kind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmcViolation {
    pub kind: String,
    pub name: String,
    pub step: usize,
    pub last_action: Option<String>,
    pub trace: Vec<TraceStep>,
    pub leads_to: Option<LeadsToViolation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeadsToViolation {
    pub bindings: BTreeMap<String, FslValue>,
    pub pending_since: usize,
    pub loop_start: Option<usize>,
    pub deadline: Option<usize>,
    pub within: Option<i64>,
    pub stutter: bool,
    pub hint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReachableWitness {
    pub step: usize,
    pub trace: Vec<TraceStep>,
}

/// One static constraint in an irreducible explanation for an unreachable
/// state predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReachableBlocker {
    pub kind: String,
    pub name: String,
}

/// A depth-independent diagnosis for a reachable target that is
/// unsatisfiable under the model's type bounds and invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReachableDiagnosis {
    pub blocking: Vec<ReachableBlocker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmcResult {
    pub spec: String,
    pub depth: usize,
    pub violation: Option<BmcViolation>,
    pub leadsto_violation: Option<BmcViolation>,
    pub reachables: BTreeMap<String, Option<ReachableWitness>>,
    /// Entries exist only for targets proved statically unsatisfiable. An
    /// unreached target absent from this map is classified as depth-limited.
    pub reachable_diagnostics: BTreeMap<String, ReachableDiagnosis>,
    pub deadlock_step: Option<usize>,
    pub deadlock_trace: Option<Vec<TraceStep>>,
    pub action_coverage: BTreeMap<String, bool>,
    pub frontier_progress: bool,
    /// Solver-dependent vacuity facts (`docs/DESIGN-vacuity.md` §2 lanes 4–7).
    /// Depth-independent by construction; see [`crate::vacuity`].
    pub vacuity: Vec<VacuityFinding>,
}

#[derive(Clone)]
struct StaticReachableConstraint<T> {
    blocker: ReachableBlocker,
    term: T,
}

/// Classify reachable predicates against a fresh symbolic state, without any
/// init or transition constraints. A target that is satisfiable here may only
/// be depth-limited; an UNSAT target is over-constrained by the returned
/// irreducible set of type bounds/invariants.
///
/// # Errors
///
/// Returns [`VerifyError`] when symbolic-state construction, expression
/// evaluation, or a solver query fails.
pub async fn diagnose_reachables<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
) -> Result<BTreeMap<String, ReachableDiagnosis>, VerifyError> {
    if model.reachables.is_empty() {
        return Ok(BTreeMap::new());
    }
    let state = symbolic_state_with_suffix(solver, model, "reachable_diagnosis")?;
    let mut constraints = Vec::new();
    for (name, _) in &model.state {
        constraints.push(StaticReachableConstraint {
            blocker: ReachableBlocker {
                kind: "type_bound".to_owned(),
                name: format!("_bounds_{name}"),
            },
            term: bounds(
                solver,
                model,
                state
                    .get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
            )?,
        });
    }
    for invariant in &model.invariants {
        let mut bindings = Bindings::new();
        let value = eval(solver, model, &invariant.expr, &state, &mut bindings, None)?;
        constraints.push(StaticReachableConstraint {
            blocker: ReachableBlocker {
                kind: "invariant".to_owned(),
                name: invariant.name.clone(),
            },
            term: bool_term(&value)?.clone(),
        });
    }

    let mut diagnoses = BTreeMap::new();
    for reachable in &model.reachables {
        let mut bindings = Bindings::new();
        let target = eval(solver, model, &reachable.expr, &state, &mut bindings, None)?;
        let target = bool_term(&target)?.clone();
        solver.set_query_context("reachable_diagnosis", &reachable.name);
        let mut active = (0..constraints.len()).collect::<Vec<_>>();
        if !terms_are_unsat(solver, &constraints, &active, &target).await? {
            continue;
        }

        // A deterministic deletion pass turns the full contradiction into an
        // irreducible core without relying on backend-specific term identity.
        let mut cursor = 0;
        while cursor < active.len() {
            let mut without = active.clone();
            without.remove(cursor);
            if terms_are_unsat(solver, &constraints, &without, &target).await? {
                active = without;
            } else {
                cursor += 1;
            }
        }
        diagnoses.insert(
            reachable.name.clone(),
            ReachableDiagnosis {
                blocking: active
                    .into_iter()
                    .map(|index| constraints[index].blocker.clone())
                    .collect(),
            },
        );
    }
    Ok(diagnoses)
}

async fn terms_are_unsat<S: SmtSolver>(
    solver: &mut S,
    constraints: &[StaticReachableConstraint<S::Term>],
    active: &[usize],
    target: &S::Term,
) -> Result<bool, VerifyError> {
    solver.push();
    let outcome = async {
        for index in active {
            solver.assert(&constraints[*index].term)?;
        }
        solver.assert(target)?;
        solver.check().await.map_err(VerifyError::from)
    }
    .await;
    let pop = solver.pop(1).map_err(VerifyError::from);
    let result = outcome?;
    pop?;
    Ok(result == SatResult::Unsat)
}

/// Explore all symbolic executions up to `depth` using a backend-neutral SMT solver.
///
/// The result intentionally mirrors the independent solver-free BFS decision
/// surface. Rich CLI diagnostics and traces are layered on after this semantic
/// core agrees with the BFS and Python oracles.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported symbolic expressions, ill-typed
/// kernel values, inconsistent init, or an unknown/backend solver result.
pub async fn verify_bounded<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    depth: usize,
) -> Result<BmcResult, VerifyError> {
    verify_bounded_selected(model, solver, depth, None).await
}

/// Verify with an optional explicit set of implicit type-bound property names.
/// `None` checks every state bound; `Some` is used by CLI property selection.
///
/// # Errors
///
/// Returns [`VerifyError`] for the same solver, model, and projection failures as
/// [`verify_bounded`].
#[allow(clippy::too_many_lines)]
pub async fn verify_bounded_selected<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    depth: usize,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<BmcResult, VerifyError> {
    verify_bounded_config(model, solver, depth, checked_bounds, None).await
}

/// Verify from a complete concrete logical-state snapshot instead of spec init.
///
/// # Errors
///
/// Returns [`VerifyError`] when the snapshot is incomplete, contains unknown
/// state, has an incompatible value, or the ordinary verifier fails.
pub async fn verify_bounded_from_state<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    depth: usize,
    checked_bounds: Option<&BTreeSet<String>>,
    initial_state: &BTreeMap<String, FslValue>,
) -> Result<BmcResult, VerifyError> {
    verify_bounded_config(model, solver, depth, checked_bounds, Some(initial_state)).await
}

#[allow(clippy::too_many_lines)]
async fn verify_bounded_config<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    depth: usize,
    checked_bounds: Option<&BTreeSet<String>>,
    initial_state: Option<&BTreeMap<String, FslValue>>,
) -> Result<BmcResult, VerifyError> {
    if model.actions.is_empty() {
        return Err(VerifyError::new("spec has no actions"));
    }
    let instances = action_instances(solver, model)?;
    let initial = symbolic_state(solver, model, 0)?;
    if let Some(snapshot) = initial_state {
        for name in snapshot.keys() {
            if !model.state.iter().any(|(candidate, _)| candidate == name) {
                return Err(VerifyError::new(format!("unknown state variable '{name}'")));
            }
        }
        for (name, ty) in &model.state {
            let value = snapshot
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing state variable '{name}'")))?;
            let symbolic = initial
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing symbolic state '{name}'")))?;
            let concrete = concrete_value(solver, model, ty, value)?;
            solver.assert(&logical_equal(solver, model, symbolic, &concrete)?)?;
        }
    } else {
        for constraint in init_constraints(solver, model, &initial)? {
            solver.assert(&constraint)?;
        }
    }
    solver.set_query_context("init", "initial_state");
    match solver.check().await? {
        SatResult::Sat => {}
        SatResult::Unsat => return Err(VerifyError::new("init constraints are unsatisfiable")),
        SatResult::Unknown => return Err(VerifyError::new("solver returned unknown for init")),
    }

    let mut result = BmcResult {
        spec: model.name.clone(),
        depth,
        violation: None,
        leadsto_violation: None,
        reachables: model
            .reachables
            .iter()
            .map(|property| (property.name.clone(), None))
            .collect(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: model
            .actions
            .iter()
            .map(|action| (action.name.clone(), false))
            .collect(),
        frontier_progress: false,
        vacuity: Vec::new(),
    };
    let mut pending_reachables = model
        .reachables
        .iter()
        .map(|property| property.name.clone())
        .collect::<BTreeSet<_>>();
    let mut states = vec![initial];
    let mut choices = Vec::new();
    let has_action_partial_operation_candidates = model
        .actions
        .iter()
        .any(action_has_partial_operation_candidate);

    for step in 0..=depth {
        let property_checks = StatePropertyChecks {
            checked_bounds,
            pending_reachables: &pending_reachables,
        };
        if let Some(violation) = check_state_properties(
            solver,
            model,
            &states,
            &choices,
            &instances,
            step,
            property_checks,
        )
        .await?
        {
            result.violation = Some(violation);
            return Ok(result);
        }

        if step < depth
            && has_action_partial_operation_candidates
            && let Some(violation) =
                check_action_partial_operations(solver, model, &states, &choices, &instances, step)
                    .await?
        {
            result.violation = Some(violation);
            return Ok(result);
        }

        record_reachables(
            solver,
            model,
            &states,
            &choices,
            &instances,
            step,
            &mut pending_reachables,
            &mut result,
        )
        .await?;

        let enabled = enabled_terms(solver, model, &instances, &states[step])?;
        record_coverage(solver, &instances, &enabled, step, &mut result).await?;
        if result.deadlock_step.is_none() {
            solver.set_query_context("deadlock", "deadlock");
            let mut deadlock = solver.not(&solver.or(&enabled)?)?;
            if let Some(terminal) = &model.terminal {
                let mut bindings = Bindings::new();
                let evaluation = property_evaluation_status(
                    solver,
                    model,
                    terminal,
                    &states[step],
                    &bindings,
                    None,
                )?;
                let partial = solver.and(&[deadlock.clone(), evaluation.first_partial.clone()])?;
                solver.set_query_context("partial_op", "terminal");
                if evaluation.has_partial_operation && probe(solver, &partial).await? {
                    result.violation = Some(
                        make_violation(
                            solver,
                            model,
                            "partial_op",
                            "_partial_property_terminal".to_owned(),
                            &partial,
                            &states,
                            &choices,
                            &instances,
                            step,
                        )
                        .await?,
                    );
                    return Ok(result);
                }
                let undefined =
                    solver.and(&[deadlock.clone(), solver.not(&evaluation.fully_defined)?])?;
                if probe(solver, &undefined).await? {
                    return Err(VerifyError::new(
                        "terminal property is undefined for a reachable deadlock state",
                    ));
                }
                let terminal = eval(solver, model, terminal, &states[step], &mut bindings, None)?;
                deadlock = solver.and(&[
                    deadlock,
                    evaluation.fully_defined,
                    solver.not(bool_term(&terminal)?)?,
                ])?;
            }
            if probe(solver, &deadlock).await? {
                result.deadlock_step = Some(step);
                result.deadlock_trace = Some(
                    build_witness(
                        solver, model, &deadlock, &states, &choices, &instances, step,
                    )
                    .await?,
                );
            }
        }

        if result.leadsto_violation.is_none() && !model.leadstos.is_empty() {
            result.leadsto_violation = check_leadsto_stagnation(
                solver, model, &states, &choices, &instances, step, &enabled,
            )
            .await?;
        }

        if result.leadsto_violation.is_none() && !model.leadstos.is_empty() {
            result.leadsto_violation =
                check_leadsto_deadlines(solver, model, &states, &choices, &instances, step).await?;
        }

        if step == depth {
            continue;
        }
        if instances.is_empty() {
            break;
        }
        let next = symbolic_state(solver, model, step + 1)?;
        let choice = solver.constant(&format!("__choice@{step}"), &fsl_solver::Sort::Int)?;
        let lower = solver.ge(&choice, &solver.int_value(0))?;
        let upper = solver.lt(&choice, &solver.int_value(i64_index(instances.len())?))?;
        solver.assert(&lower)?;
        solver.assert(&upper)?;
        let transition =
            transition_constraint(solver, model, &instances, &states[step], &next, &choice)?;
        solver.assert(&transition)?;
        states.push(next);
        choices.push(choice);
    }
    if result.leadsto_violation.is_none() {
        let unrolled_depth = states.len() - 1;
        result.leadsto_violation =
            check_leadstos(solver, model, &states, &choices, &instances, unrolled_depth).await?;
    }
    // The solver-dependent vacuity lanes run last, after every witness,
    // reachable, and deadlock trace has been projected. They ask nothing about
    // the unrolled states — each lane quantifies over freshly named ones — but
    // a query still moves the backend's internal state, and two Z3 builds may
    // then resolve an under-determined model differently. The native and
    // browser evidence contract is byte-compared, so no new query may run
    // before the evidence it could perturb.
    //
    // By this point the unrolling has asserted a mandatory forward transition
    // out of every state, which a spec that deadlocks on every path makes
    // globally unsatisfiable. In such a session every query answers `unsat`
    // and every lane would fire on nothing. Report no vacuity rather than a
    // fabricated one.
    if session_satisfiable(solver).await? {
        result.vacuity = static_findings(model, solver, &instances).await?;
        retain_covered(&mut result.vacuity, &result.action_coverage);
    }
    Ok(result)
}

struct StatePropertyChecks<'a> {
    checked_bounds: Option<&'a BTreeSet<String>>,
    pending_reachables: &'a BTreeSet<String>,
}

#[allow(clippy::too_many_lines)]
async fn check_state_properties<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
    checks: StatePropertyChecks<'_>,
) -> Result<Option<BmcViolation>, VerifyError> {
    for (name, _) in &model.state {
        let property_name = format!("_bounds_{name}");
        if checks
            .checked_bounds
            .is_some_and(|selected| !selected.contains(&property_name))
        {
            continue;
        }
        let valid = bounds(
            solver,
            model,
            states[step]
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
        )?;
        solver.set_query_context("type_bound", &property_name);
        if probe_not(solver, &valid).await? {
            let failure = solver.not(&valid)?;
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "type_bound",
                    property_name,
                    &failure,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        solver.assert(&valid)?;
    }
    for property in &model.invariants {
        let mut bindings = Bindings::new();
        let evaluation = property_evaluation_status(
            solver,
            model,
            &property.expr,
            &states[step],
            &bindings,
            None,
        )?;
        solver.set_query_context("partial_op", &property.name);
        if evaluation.has_partial_operation && probe(solver, &evaluation.first_partial).await? {
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "partial_op",
                    format!("_partial_property_{}", property.name),
                    &evaluation.first_partial,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        if probe_not(solver, &evaluation.fully_defined).await? {
            return Err(VerifyError::new(format!(
                "invariant '{}' is undefined for a reachable state",
                property.name
            )));
        }
        solver.assert(&evaluation.fully_defined)?;
        let value = eval(
            solver,
            model,
            &property.expr,
            &states[step],
            &mut bindings,
            None,
        )?;
        let condition = bool_term(&value)?.clone();
        solver.set_query_context("invariant", &property.name);
        if probe_not(solver, &condition).await? {
            let failure = solver.not(&condition)?;
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "invariant",
                    property.name.clone(),
                    &failure,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        solver.assert(&condition)?;
    }
    for property in &model.reachables {
        if !checks.pending_reachables.contains(&property.name) {
            continue;
        }
        let bindings = Bindings::new();
        let evaluation = property_evaluation_status(
            solver,
            model,
            &property.expr,
            &states[step],
            &bindings,
            None,
        )?;
        solver.set_query_context("partial_op", &property.name);
        if evaluation.has_partial_operation && probe(solver, &evaluation.first_partial).await? {
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "partial_op",
                    format!("_partial_property_{}", property.name),
                    &evaluation.first_partial,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        if probe_not(solver, &evaluation.fully_defined).await? {
            return Err(VerifyError::new(format!(
                "reachable property '{}' is undefined for a reachable state",
                property.name
            )));
        }
        solver.assert(&evaluation.fully_defined)?;
    }
    for property in &model.leadstos {
        for binding in leadsto_bindings(solver, model, property)? {
            for expression in [&property.before, &property.after] {
                let evaluation = property_evaluation_status(
                    solver,
                    model,
                    expression,
                    &states[step],
                    &binding.symbolic,
                    None,
                )?;
                solver.set_query_context("partial_op", &property.name);
                if evaluation.has_partial_operation
                    && probe(solver, &evaluation.first_partial).await?
                {
                    return Ok(Some(
                        make_violation(
                            solver,
                            model,
                            "partial_op",
                            format!("_partial_property_{}", property.name),
                            &evaluation.first_partial,
                            states,
                            choices,
                            instances,
                            step,
                        )
                        .await?,
                    ));
                }
                if probe_not(solver, &evaluation.fully_defined).await? {
                    return Err(VerifyError::new(format!(
                        "leadsTo property '{}' is undefined for a reachable state",
                        property.name
                    )));
                }
                solver.assert(&evaluation.fully_defined)?;
            }
        }
    }
    if step == 0 {
        return Ok(None);
    }
    for property in &model.transitions {
        let mut bindings = Bindings::new();
        let evaluation = property_evaluation_status(
            solver,
            model,
            &property.expr,
            &states[step],
            &bindings,
            Some(&states[step - 1]),
        )?;
        solver.set_query_context("partial_op", &property.name);
        if evaluation.has_partial_operation && probe(solver, &evaluation.first_partial).await? {
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "partial_op",
                    format!("_partial_property_{}", property.name),
                    &evaluation.first_partial,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        if probe_not(solver, &evaluation.fully_defined).await? {
            return Err(VerifyError::new(format!(
                "transition property '{}' is undefined for a reachable state",
                property.name
            )));
        }
        solver.assert(&evaluation.fully_defined)?;
        let value = eval(
            solver,
            model,
            &property.expr,
            &states[step],
            &mut bindings,
            Some(&states[step - 1]),
        )?;
        let condition = bool_term(&value)?.clone();
        solver.set_query_context("trans", &property.name);
        if probe_not(solver, &condition).await? {
            let failure = solver.not(&condition)?;
            return Ok(Some(
                make_violation(
                    solver,
                    model,
                    "trans",
                    property.name.clone(),
                    &failure,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        solver.assert(&condition)?;
    }
    for (instance_index, instance) in instances.iter().enumerate() {
        let action = &model.actions[instance.action_index];
        if action.ensures.is_empty() {
            continue;
        }
        let guard_status =
            action_guard_definedness(solver, model, action, &states[step - 1], &instance.params)?;
        let mut bindings = guard_status.bindings;
        let body_status = action_statements_evaluation_status(
            solver,
            model,
            action,
            &states[step - 1],
            &bindings,
        )?;
        let selected = solver.equal(
            &choices[step - 1],
            &solver.int_value(i64_index(instance_index)?),
        )?;
        let mut reached =
            solver.and(&[selected, guard_status.enabled, body_status.fully_defined])?;
        for ensure in &action.ensures {
            solver.set_query_context("ensures", &action.name);
            let ensure_status = evaluation_status(
                solver,
                model,
                ensure,
                &states[step],
                &bindings,
                Some(&states[step - 1]),
            )?;
            let partial = solver.and(&[reached.clone(), ensure_status.first_partial])?;
            if probe(solver, &partial).await? {
                return Ok(Some(
                    make_violation(
                        solver,
                        model,
                        "partial_op",
                        format!("_partial_{}", action.name),
                        &partial,
                        states,
                        choices,
                        instances,
                        step,
                    )
                    .await?,
                ));
            }
            let undefined =
                solver.and(&[reached.clone(), solver.not(&ensure_status.fully_defined)?])?;
            if probe(solver, &undefined).await? {
                return Err(VerifyError::new(format!(
                    "action '{}' ensures evaluation has a non-partial failure",
                    action.name
                )));
            }
            let value = eval(
                solver,
                model,
                ensure,
                &states[step],
                &mut bindings,
                Some(&states[step - 1]),
            )?;
            let failure = solver.and(&[
                reached.clone(),
                ensure_status.fully_defined.clone(),
                solver.not(bool_term(&value)?)?,
            ])?;
            if probe(solver, &failure).await? {
                return Ok(Some(
                    make_violation(
                        solver,
                        model,
                        "ensures",
                        action.name.clone(),
                        &failure,
                        states,
                        choices,
                        instances,
                        step,
                    )
                    .await?,
                ));
            }
            reached = solver.and(&[
                reached,
                ensure_status.fully_defined,
                bool_term(&value)?.clone(),
            ])?;
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn check_action_partial_operations<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
) -> Result<Option<BmcViolation>, VerifyError> {
    for instance in instances {
        let action = &model.actions[instance.action_index];
        let guard_evaluation =
            action_guard_definedness(solver, model, action, &states[step], &instance.params)?;
        let body_status = action_statements_evaluation_status(
            solver,
            model,
            action,
            &states[step],
            &guard_evaluation.bindings,
        )?;
        let mut ensures_have_partial_operation = false;
        for ensure in &action.ensures {
            ensures_have_partial_operation |= evaluation_status(
                solver,
                model,
                ensure,
                &states[step],
                &guard_evaluation.bindings,
                Some(&states[step]),
            )?
            .has_partial_operation;
        }
        if !guard_evaluation.has_partial_operation
            && !body_status.has_partial_operation
            && !ensures_have_partial_operation
        {
            continue;
        }
        let guard_failure = guard_evaluation.first_partial;
        solver.set_query_context("partial_op", &action.name);
        if probe(solver, &guard_failure).await? {
            return Ok(Some(
                make_action_partial_operation_violation(
                    solver,
                    model,
                    action,
                    instance,
                    &guard_failure,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        let guard_undefined = solver.not(&guard_evaluation.defined)?;
        if probe(solver, &guard_undefined).await? {
            return Err(VerifyError::new(format!(
                "action '{}' guard evaluation has a non-partial failure",
                action.name
            )));
        }

        let body_failure =
            solver.and(&[guard_evaluation.enabled.clone(), body_status.first_partial])?;
        if probe(solver, &body_failure).await? {
            return Ok(Some(
                make_action_partial_operation_violation(
                    solver,
                    model,
                    action,
                    instance,
                    &body_failure,
                    states,
                    choices,
                    instances,
                    step,
                )
                .await?,
            ));
        }
        let body_undefined = solver.and(&[
            guard_evaluation.enabled.clone(),
            solver.not(&body_status.fully_defined)?,
        ])?;
        if probe(solver, &body_undefined).await? {
            return Err(VerifyError::new(format!(
                "action '{}' body evaluation has a non-partial failure",
                action.name
            )));
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn make_action_partial_operation_violation<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    action: &fsl_core::ActionDef,
    instance: &ActionInstance<S::Term>,
    condition: &S::Term,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
) -> Result<BmcViolation, VerifyError> {
    let mut trace =
        build_witness(solver, model, condition, states, choices, instances, step).await?;
    let state = trace
        .last()
        .ok_or_else(|| VerifyError::new("partial-operation witness is empty"))?
        .state
        .clone();
    trace.push(TraceStep {
        step: step + 1,
        state,
        action: Some(TraceAction {
            name: action.name.clone(),
            params: instance.concrete_params.clone(),
        }),
        changes: BTreeMap::new(),
    });
    Ok(BmcViolation {
        kind: "partial_op".to_owned(),
        name: format!("_partial_{}", action.name),
        step: step + 1,
        last_action: Some(action.name.clone()),
        trace,
        leads_to: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn record_reachables<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
    pending: &mut BTreeSet<String>,
    result: &mut BmcResult,
) -> Result<(), VerifyError> {
    let mut witnessed = Vec::new();
    for property in &model.reachables {
        if !pending.contains(&property.name) {
            continue;
        }
        let mut bindings = Bindings::new();
        solver.set_query_context("reachable", &property.name);
        let value = eval(
            solver,
            model,
            &property.expr,
            &states[step],
            &mut bindings,
            None,
        )?;
        if probe(solver, bool_term(&value)?).await? {
            let trace = build_witness(
                solver,
                model,
                bool_term(&value)?,
                states,
                choices,
                instances,
                step,
            )
            .await?;
            result.reachables.insert(
                property.name.clone(),
                Some(ReachableWitness { step, trace }),
            );
            if step == result.depth && result.depth > 0 {
                result.frontier_progress = true;
            }
            witnessed.push(property.name.clone());
        }
    }
    for name in witnessed {
        pending.remove(&name);
    }
    Ok(())
}

fn enabled_terms<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    instances: &[ActionInstance<S::Term>],
    state: &SymbolicState<S::Term>,
) -> Result<Vec<S::Term>, VerifyError> {
    instances
        .iter()
        .map(|instance| {
            let action = &model.actions[instance.action_index];
            let (guards, _) = action_guards(solver, model, action, state, &instance.params)?;
            Ok(solver.and(&guards)?)
        })
        .collect()
}

async fn record_coverage<S: SmtSolver>(
    solver: &mut S,
    instances: &[ActionInstance<S::Term>],
    enabled: &[S::Term],
    step: usize,
    result: &mut BmcResult,
) -> Result<(), VerifyError> {
    for (instance, enabled) in instances.iter().zip(enabled) {
        if result.action_coverage[&instance.action] {
            continue;
        }
        solver.set_query_context("action_coverage", &instance.action);
        if probe(solver, enabled).await? {
            result.action_coverage.insert(instance.action.clone(), true);
            if step == result.depth && result.depth > 0 {
                result.frontier_progress = true;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn make_violation<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    kind: &str,
    name: String,
    condition: &S::Term,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
) -> Result<BmcViolation, VerifyError> {
    let trace = build_witness(solver, model, condition, states, choices, instances, step).await?;
    let last_action = trace
        .last()
        .and_then(|entry| entry.action.as_ref().map(|action| action.name.clone()));
    Ok(BmcViolation {
        kind: kind.to_owned(),
        name,
        step,
        last_action,
        trace,
        leads_to: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn build_witness<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    condition: &S::Term,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    upto: usize,
) -> Result<Vec<TraceStep>, VerifyError> {
    solver.push();
    if let Err(error) = solver.assert(condition) {
        solver.pop(1)?;
        return Err(error.into());
    }
    let checked = solver.check().await;
    let projected = match checked {
        Ok(SatResult::Sat) => project_trace(solver, model, states, choices, instances, upto),
        Ok(SatResult::Unsat) => Err(VerifyError::new("witness condition became unsatisfiable")),
        Ok(SatResult::Unknown) => Err(VerifyError::new("solver returned unknown for witness")),
        Err(error) => Err(error.into()),
    };
    let popped = solver.pop(1);
    popped?;
    projected
}

fn states_equal<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    left: &SymbolicState<S::Term>,
    right: &SymbolicState<S::Term>,
) -> Result<S::Term, VerifyError> {
    let equalities = model
        .state
        .iter()
        .map(|(name, _)| {
            logical_equal(
                solver,
                model,
                left.get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
                right
                    .get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(solver.and(&equalities)?)
}

fn fairness_condition<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    start: usize,
    end: usize,
) -> Result<S::Term, VerifyError> {
    let mut fair = Vec::new();
    for (index, instance) in instances.iter().enumerate() {
        let action = &model.actions[instance.action_index];
        if !action.fair {
            continue;
        }
        let disabled = (start..end)
            .map(|step| {
                let (guards, _) =
                    action_guards(solver, model, action, &states[step], &instance.params)?;
                Ok(solver.not(&solver.and(&guards)?)?)
            })
            .collect::<Result<Vec<_>, VerifyError>>()?;
        let executed = (start..end)
            .map(|step| Ok(solver.equal(&choices[step], &solver.int_value(i64_index(index)?))?))
            .collect::<Result<Vec<_>, VerifyError>>()?;
        fair.push(solver.or(&[solver.or(&disabled)?, solver.or(&executed)?])?);
    }
    Ok(solver.and(&fair)?)
}

#[allow(clippy::too_many_arguments)]
async fn leadsto_violation<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    property: &LeadsToDef,
    binding: &LeadstoBinding<S::Term>,
    condition: &S::Term,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    upto: usize,
    details: LeadsToViolation,
) -> Result<BmcViolation, VerifyError> {
    let trace = build_witness(solver, model, condition, states, choices, instances, upto).await?;
    let last_action = trace
        .last()
        .and_then(|entry| entry.action.as_ref().map(|action| action.name.clone()));
    let mut details = details;
    details.bindings = binding.concrete.clone();
    Ok(BmcViolation {
        kind: violation_kind::LEADS_TO.to_owned(),
        name: property.name.clone(),
        step: upto,
        last_action,
        trace,
        leads_to: Some(details),
    })
}

/// Check whether `states[step]` is a deadlock with a pending leadsTo obligation.
///
/// Must run before the BMC unrolling loop asserts a mandatory forward
/// transition out of `states[step]` (see `verify_bounded_config`); once that
/// assertion is committed, a deadlock at `step` becomes globally unsatisfiable
/// for any later query in the same solver session, regardless of whether the
/// deadlock is actually reachable. Running this inline, per step, mirrors the
/// timing of the general deadlock probe in the same loop and the frozen
/// Python reference's `_check_leadsto_stutter_at_step`.
#[allow(clippy::too_many_arguments)]
async fn check_leadsto_stagnation<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
    enabled: &[S::Term],
) -> Result<Option<BmcViolation>, VerifyError> {
    let deadlock = solver.not(&solver.or(enabled)?)?;
    let canonical = canonical_constraint(solver, model, &states[step])?;
    for property in &model.leadstos {
        solver.set_query_context("leadsTo", &property.name);
        for binding in leadsto_bindings(solver, model, property)? {
            for pending in 0..=step {
                let mut terms = vec![
                    deadlock.clone(),
                    canonical.clone(),
                    leadsto_condition(
                        solver,
                        model,
                        &property.before,
                        &states[pending],
                        &binding.symbolic,
                    )?,
                ];
                for state in states.iter().take(step + 1).skip(pending) {
                    terms.push(solver.not(&leadsto_condition(
                        solver,
                        model,
                        &property.after,
                        state,
                        &binding.symbolic,
                    )?)?);
                }
                let condition = solver.and(&terms)?;
                if probe(solver, &condition).await? {
                    return Ok(Some(
                        leadsto_violation(
                            solver,
                            model,
                            property,
                            &binding,
                            &condition,
                            states,
                            choices,
                            instances,
                            step,
                            LeadsToViolation {
                                bindings: BTreeMap::new(),
                                pending_since: pending,
                                loop_start: None,
                                deadline: None,
                                within: property.within,
                                stutter: true,
                                hint: format!(
                                    "P held at step {pending} but execution deadlocks at step {step} without Q"
                                ),
                            },
                        )
                        .await?,
                    ));
                }
            }
        }
    }
    Ok(None)
}

/// Check whether a `within` deadline expires unmet at `states[step]`.
///
/// At step `t`, the only window whose deadline lands on `t` starts at
/// `pending = t - within`; the probe asks whether P can hold at `pending`
/// with Q failing on every state through `t`. Like
/// `check_leadsto_stagnation`, this must run before the BMC unrolling loop
/// asserts a mandatory forward transition out of `states[step]`: a path that
/// deadlocks after a missed deadline becomes globally unsatisfiable once the
/// deadlocked step's forced transition is committed, so a post-loop probe
/// misses exactly the missed-deadline-then-deadlock combination (issue #266).
async fn check_leadsto_deadlines<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    step: usize,
) -> Result<Option<BmcViolation>, VerifyError> {
    for property in &model.leadstos {
        let Some(within) = property.within else {
            continue;
        };
        solver.set_query_context("leadsTo", &property.name);
        let within = usize::try_from(within)
            .map_err(|_| VerifyError::new("leadsTo within must be non-negative"))?;
        let Some(pending) = step.checked_sub(within) else {
            continue;
        };
        for binding in leadsto_bindings(solver, model, property)? {
            let mut terms = vec![leadsto_condition(
                solver,
                model,
                &property.before,
                &states[pending],
                &binding.symbolic,
            )?];
            for state in states.iter().take(step + 1).skip(pending) {
                terms.push(solver.not(&leadsto_condition(
                    solver,
                    model,
                    &property.after,
                    state,
                    &binding.symbolic,
                )?)?);
            }
            let condition = solver.and(&terms)?;
            if probe(solver, &condition).await? {
                return Ok(Some(
                    leadsto_violation(
                        solver,
                        model,
                        property,
                        &binding,
                        &condition,
                        states,
                        choices,
                        instances,
                        step,
                        LeadsToViolation {
                            bindings: BTreeMap::new(),
                            pending_since: pending,
                            loop_start: None,
                            deadline: Some(step),
                            within: property.within,
                            stutter: false,
                            hint: format!(
                                "leadsTo deadline missed: P holds at step {pending}, but Q does not hold within {within} step(s)"
                            ),
                        },
                    )
                    .await?,
                ));
            }
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_lines)]
async fn check_leadstos<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    depth: usize,
) -> Result<Option<BmcViolation>, VerifyError> {
    let canonical = states
        .iter()
        .take(depth + 1)
        .map(|state| canonical_constraint(solver, model, state))
        .collect::<Result<Vec<_>, _>>()?;
    for property in &model.leadstos {
        solver.set_query_context("leadsTo", &property.name);
        for binding in leadsto_bindings(solver, model, property)? {
            for loop_start in 0..depth {
                for loop_end in (loop_start + 1)..=depth {
                    let loop_equal =
                        states_equal(solver, model, &states[loop_start], &states[loop_end])?;
                    let fair = fairness_condition(
                        solver, model, states, choices, instances, loop_start, loop_end,
                    )?;
                    for pending in 0..loop_end {
                        let mut terms = vec![
                            loop_equal.clone(),
                            canonical[loop_start].clone(),
                            fair.clone(),
                            leadsto_condition(
                                solver,
                                model,
                                &property.before,
                                &states[pending],
                                &binding.symbolic,
                            )?,
                        ];
                        for state in states.iter().take(loop_end).skip(loop_start.min(pending)) {
                            terms.push(solver.not(&leadsto_condition(
                                solver,
                                model,
                                &property.after,
                                state,
                                &binding.symbolic,
                            )?)?);
                        }
                        let condition = solver.and(&terms)?;
                        if probe(solver, &condition).await? {
                            return Ok(Some(
                                leadsto_violation(
                                    solver,
                                    model,
                                    property,
                                    &binding,
                                    &condition,
                                    states,
                                    choices,
                                    instances,
                                    loop_end,
                                    LeadsToViolation {
                                        bindings: BTreeMap::new(),
                                        pending_since: pending,
                                        loop_start: Some(loop_start),
                                        deadline: None,
                                        within: property.within,
                                        stutter: false,
                                        hint: format!(
                                            "P held at step {pending} but the loop from step {loop_start} can repeat forever without Q; if progress relies on some action being taken eventually, annotate it with `fair action ...`"
                                        ),
                                    },
                                )
                                .await?,
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Whether the accumulated unrolling session still has a model at all.
async fn session_satisfiable<S: SmtSolver>(solver: &mut S) -> Result<bool, VerifyError> {
    solver.set_query_context("vacuity", "session");
    Ok(matches!(solver.check().await?, SatResult::Sat))
}

async fn probe_not<S: SmtSolver>(solver: &mut S, condition: &S::Term) -> Result<bool, VerifyError> {
    probe(solver, &solver.not(condition)?).await
}

async fn probe<S: SmtSolver>(solver: &mut S, condition: &S::Term) -> Result<bool, VerifyError> {
    solver.push();
    if let Err(error) = solver.assert(condition) {
        solver.pop(1)?;
        return Err(error.into());
    }
    let checked = solver.check().await;
    let popped = solver.pop(1);
    let result = checked?;
    popped?;
    match result {
        SatResult::Sat => Ok(true),
        SatResult::Unsat => Ok(false),
        SatResult::Unknown => Err(VerifyError::new("solver returned unknown")),
    }
}
