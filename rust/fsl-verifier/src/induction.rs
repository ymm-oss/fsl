// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::recursion;
use fsl_core::{FslValue, HelpfulAction, KernelExpr, KernelModel, LeadsToDef, TypeDef, TypeRef};
use fsl_solver::{ModelValue, SatResult, SmtSolver};

use crate::VerifyError;
use crate::eval::{eval, evaluation_status, property_evaluation_status};
use crate::liveness::{leadsto_bindings, leadsto_condition};
use crate::trace::project_trace;
use crate::transition::{
    ActionInstance, action_guard_definedness, action_has_partial_operation_candidate,
    action_instances, action_statements_evaluation_status, transition_constraint,
};
use crate::value::{
    Bindings, SymbolicState, bool_term, bounds, i64_index, int_term, symbolic_state_with_suffix,
};
use crate::violation_kind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InductionCti {
    pub kind: String,
    pub name: String,
    pub k: usize,
    pub trace: Vec<fsl_core::TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct InductionResult {
    pub k_used: BTreeMap<String, usize>,
    pub cti: Option<InductionCti>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankProof {
    pub name: String,
    pub measure: fsl_core::KernelExpr,
    /// The leadsTo's declared `helpful` action metadata, carried through
    /// unformatted so the CLI can render display labels.
    pub helpful: Vec<HelpfulAction>,
}

/// One `helpful`-matching or `helpful`-blocking action instance, named for
/// JSON rendering by the caller (mirrors the frozen Python reference's
/// `helpful_actions` display list).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelpfulActionWitness {
    pub action: String,
    pub params: BTreeMap<String, FslValue>,
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
    /// The leadsTo's declared `helpful` action metadata (empty unless the
    /// property declares `helpful`).
    pub helpful: Vec<HelpfulAction>,
    /// The matched/blocked helpful action instance(s) relevant to this
    /// failure (empty unless `kind` is one of the `helpful_*`/
    /// `progress_action_not_fair`/`non_helpful_action_increases_measure`
    /// kinds).
    pub helpful_actions: Vec<HelpfulActionWitness>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
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

    fn kind(self) -> &'static str {
        match self {
            Self::Bound(_) => "type_bound",
            Self::Invariant(_) => "invariant",
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

fn attributed_property_condition<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    property: Property<'_>,
    state: &SymbolicState<S::Term>,
) -> Result<S::Term, VerifyError> {
    solver.set_query_context(property.kind(), &property.name(model));
    property_condition(solver, model, property, state, None)
}

/// Prove kernel invariants and transitions by k-induction after a successful
/// bounded base case.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported symbolic expressions or solver
/// failures.
#[allow(clippy::too_many_lines)]
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
            let condition = attributed_property_condition(solver, model, property, &states[k])?;
            solver.push();
            solver.assert(&solver.not(&condition)?)?;
            match solver.check().await? {
                SatResult::Unsat => {
                    k_used.insert(property.name(model), k);
                }
                SatResult::Sat => {
                    let trace = project_trace(solver, model, &states, &choices, &instances, k)?;
                    last_cti = Some(InductionCti {
                        kind: violation_kind::INVARIANT.to_owned(),
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
                solver.set_query_context("trans", &transition.name);
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
                                kind: violation_kind::TRANS.to_owned(),
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
            let cti = definedness_cti(solver, model, &instances).await?;
            return Ok(InductionResult { k_used, cti });
        }
    }

    Ok(InductionResult {
        k_used,
        cti: last_cti,
    })
}

/// Probe one partial-operation condition; on `sat`, project the witness
/// trace over `states[..=upto]` before the scope is popped.
async fn partial_witness<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    condition: &S::Term,
    states: &[SymbolicState<S::Term>],
    choices: &[S::Term],
    instances: &[ActionInstance<S::Term>],
    upto: usize,
) -> Result<Option<Vec<fsl_core::TraceStep>>, VerifyError> {
    solver.push();
    solver.assert(condition)?;
    let outcome = match solver.check().await {
        Ok(SatResult::Sat) => {
            project_trace(solver, model, states, choices, instances, upto).map(Some)
        }
        Ok(SatResult::Unsat) => Ok(None),
        Ok(SatResult::Unknown) => Err(VerifyError::new("solver returned unknown in induction")),
        Err(error) => Err(error.into()),
    };
    solver.pop(1)?;
    outcome
}

fn partial_cti(name: String, k: usize, trace: Vec<fsl_core::TraceStep>) -> InductionCti {
    InductionCti {
        kind: violation_kind::PARTIAL_OP.to_owned(),
        name,
        k,
        trace,
    }
}

/// Append the attempted (rolled-back) action step to a one-state witness,
/// mirroring BMC's `make_action_partial_operation_violation`.
fn with_attempted_action<T>(
    mut trace: Vec<fsl_core::TraceStep>,
    instance: &ActionInstance<T>,
) -> Vec<fsl_core::TraceStep> {
    if let Some(last) = trace.last().cloned() {
        trace.push(fsl_core::TraceStep {
            step: last.step + 1,
            state: last.state,
            action: Some(fsl_core::TraceAction {
                name: instance.action.clone(),
                params: instance.concrete_params.clone(),
            }),
            changes: BTreeMap::new(),
        });
    }
    trace
}

/// Definedness obligation of the induction step (#1196).
///
/// The step case evaluates guards, bodies, and properties with the
/// totalizing symbolic evaluator, so on its own it never sees the
/// `partial_op` failures that BMC and the explicit engine report when a
/// reachable state reaches one of LANGUAGE.md §6's six partial operations.
/// After every invariant and transition property is proved, this asks, for
/// a free state `s` (and a free successor `s'`) under the proved invariants
/// only — never under `init` — whether any of the per-step partial-operation
/// checks BMC performs (`bmc::check_state_properties` /
/// `bmc::check_action_partial_operations`) can fire:
///
/// - `Inv(s) ∧ first_partial(P, s)` for every invariant and every `leadsTo`
///   trigger/goal (property context: division is total, sequence access is
///   not — the same `property_evaluation_status` BMC uses);
/// - `Inv(s) ∧ first_partial(guards_a, s)` for every action instance;
/// - `Inv(s) ∧ enabled_a(s) ∧ first_partial(body_a, s)`;
/// - `Inv(s) ∧ T(s, s') ∧ Inv(s') ∧ Trans(s, s') ∧ first_partial(Q, s, s')`
///   for every `transition` property and every reached `ensures`.
///
/// Every reachable state satisfies the proved invariants, and every real
/// step is a step of the totalized `T` (a defined path evaluates
/// identically), so `unsat` for all of them shows no reachable state or step
/// reaches a partial operation. A `sat` answer is returned as an
/// `unknown_cti` of kind [`violation_kind::PARTIAL_OP`]: the witness
/// satisfies the invariants but may be unreachable, exactly like an
/// invariant CTI. The checks short-circuit on the same `and`/`or`/`=>`/`if`
/// and guard-order reachability as BMC (`evaluation_status`), so a guard such
/// as `d != 0 and x / d < 100` stays defined.
#[allow(clippy::too_many_lines)]
async fn definedness_cti<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    instances: &[ActionInstance<S::Term>],
) -> Result<Option<InductionCti>, VerifyError> {
    let has_action_candidates = model
        .actions
        .iter()
        .any(action_has_partial_operation_candidate);
    let has_property_candidates = model
        .invariants
        .iter()
        .chain(&model.transitions)
        .any(|property| fsl_core::expression_has_partial_operation_candidate(&property.expr))
        || model.leadstos.iter().any(|property| {
            fsl_core::expression_has_partial_operation_candidate(&property.before)
                || fsl_core::expression_has_partial_operation_candidate(&property.after)
        });
    if !has_action_candidates && !has_property_candidates {
        // Nothing can be undefined: declare no terms and issue no queries,
        // so the solver session later engines share is untouched.
        return Ok(None);
    }
    let properties = properties(model);
    let def_states = vec![
        symbolic_state_with_suffix(solver, model, "def0")?,
        symbolic_state_with_suffix(solver, model, "def1")?,
    ];
    let choice = solver.constant("__def_choice@0", &fsl_solver::Sort::Int)?;
    let choices = vec![choice.clone()];

    solver.push();
    let result = async {
        for property in &properties {
            let assumption = property_condition(solver, model, *property, &def_states[0], None)?;
            solver.assert(&assumption)?;
        }

        // State-level property definedness.
        for property in &model.invariants {
            let status = property_evaluation_status(
                solver,
                model,
                &property.expr,
                &def_states[0],
                &Bindings::new(),
                None,
            )?;
            if !status.has_partial_operation {
                continue;
            }
            solver.set_query_context("partial_op", &property.name);
            if let Some(trace) = partial_witness(
                solver,
                model,
                &status.first_partial,
                &def_states,
                &choices,
                instances,
                0,
            )
            .await?
            {
                return Ok(Some(partial_cti(
                    format!("_partial_property_{}", property.name),
                    0,
                    trace,
                )));
            }
        }
        for property in &model.leadstos {
            for binding in leadsto_bindings(solver, model, property)? {
                for expression in [&property.before, &property.after] {
                    let status = property_evaluation_status(
                        solver,
                        model,
                        expression,
                        &def_states[0],
                        &binding.symbolic,
                        None,
                    )?;
                    if !status.has_partial_operation {
                        continue;
                    }
                    solver.set_query_context("partial_op", &property.name);
                    if let Some(trace) = partial_witness(
                        solver,
                        model,
                        &status.first_partial,
                        &def_states,
                        &choices,
                        instances,
                        0,
                    )
                    .await?
                    {
                        return Ok(Some(partial_cti(
                            format!("_partial_property_{}", property.name),
                            0,
                            trace,
                        )));
                    }
                }
            }
        }

        // Action guard and body definedness in the pre-state.
        if has_action_candidates {
            for instance in instances {
                let action = &model.actions[instance.action_index];
                let guard = action_guard_definedness(
                    solver,
                    model,
                    action,
                    &def_states[0],
                    &instance.params,
                )?;
                let body = action_statements_evaluation_status(
                    solver,
                    model,
                    action,
                    &def_states[0],
                    &guard.bindings,
                )?;
                if !guard.has_partial_operation && !body.has_partial_operation {
                    continue;
                }
                solver.set_query_context("partial_op", &action.name);
                let body_failure = solver.and(&[guard.enabled.clone(), body.first_partial])?;
                for failure in [guard.first_partial, body_failure] {
                    if let Some(trace) = partial_witness(
                        solver,
                        model,
                        &failure,
                        &def_states,
                        &choices,
                        instances,
                        0,
                    )
                    .await?
                    {
                        return Ok(Some(partial_cti(
                            format!("_partial_{}", action.name),
                            1,
                            with_attempted_action(trace, instance),
                        )));
                    }
                }
            }
        }

        // Two-state definedness: transition properties and reached ensures.
        let has_ensures_candidates = model.actions.iter().any(|action| {
            action
                .ensures
                .iter()
                .any(fsl_core::expression_has_partial_operation_candidate)
        });
        let mut transition_statuses = Vec::new();
        for property in &model.transitions {
            let status = property_evaluation_status(
                solver,
                model,
                &property.expr,
                &def_states[1],
                &Bindings::new(),
                Some(&def_states[0]),
            )?;
            if status.has_partial_operation {
                transition_statuses.push((property.name.clone(), status.first_partial));
            }
        }
        if transition_statuses.is_empty() && !has_ensures_candidates {
            return Ok(None);
        }
        solver.assert(&solver.ge(&choice, &solver.int_value(0))?)?;
        solver.assert(&solver.lt(&choice, &solver.int_value(i64_index(instances.len())?))?)?;
        solver.assert(&transition_constraint(
            solver,
            model,
            instances,
            &def_states[0],
            &def_states[1],
            &choice,
        )?)?;
        for property in &properties {
            let assumption = property_condition(solver, model, *property, &def_states[1], None)?;
            solver.assert(&assumption)?;
        }
        for transition in &model.transitions {
            let value = eval(
                solver,
                model,
                &transition.expr,
                &def_states[1],
                &mut Bindings::new(),
                Some(&def_states[0]),
            )?;
            solver.assert(bool_term(&value)?)?;
        }
        for (name, first_partial) in transition_statuses {
            solver.set_query_context("partial_op", &name);
            if let Some(trace) = partial_witness(
                solver,
                model,
                &first_partial,
                &def_states,
                &choices,
                instances,
                1,
            )
            .await?
            {
                return Ok(Some(partial_cti(
                    format!("_partial_property_{name}"),
                    1,
                    trace,
                )));
            }
        }
        if !has_ensures_candidates {
            return Ok(None);
        }
        for (instance_index, instance) in instances.iter().enumerate() {
            let action = &model.actions[instance.action_index];
            if action.ensures.is_empty() {
                continue;
            }
            let guard =
                action_guard_definedness(solver, model, action, &def_states[0], &instance.params)?;
            let mut bindings = guard.bindings;
            let body = action_statements_evaluation_status(
                solver,
                model,
                action,
                &def_states[0],
                &bindings,
            )?;
            let selected = solver.equal(&choice, &solver.int_value(i64_index(instance_index)?))?;
            let mut reached = solver.and(&[selected, guard.enabled, body.fully_defined])?;
            for ensure in &action.ensures {
                let status = evaluation_status(
                    solver,
                    model,
                    ensure,
                    &def_states[1],
                    &bindings,
                    Some(&def_states[0]),
                )?;
                if status.has_partial_operation {
                    solver.set_query_context("partial_op", &action.name);
                    let partial = solver.and(&[reached.clone(), status.first_partial])?;
                    if let Some(trace) = partial_witness(
                        solver,
                        model,
                        &partial,
                        &def_states,
                        &choices,
                        instances,
                        1,
                    )
                    .await?
                    {
                        return Ok(Some(partial_cti(
                            format!("_partial_{}", action.name),
                            1,
                            trace,
                        )));
                    }
                }
                let value = eval(
                    solver,
                    model,
                    ensure,
                    &def_states[1],
                    &mut bindings,
                    Some(&def_states[0]),
                )?;
                reached =
                    solver.and(&[reached, status.fully_defined, bool_term(&value)?.clone()])?;
            }
        }
        Ok(None)
    }
    .await;
    solver.pop(1)?;
    result
}

fn model_int<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<i64, VerifyError> {
    match solver.model_eval(term)? {
        Some(ModelValue::Int(value)) => Ok(value),
        Some(ModelValue::Bool(_)) => Err(VerifyError::new("ranking measure is Boolean")),
        None => Err(VerifyError::new("ranking measure is unavailable in model")),
    }
}

fn model_bool<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<bool, VerifyError> {
    match solver.model_eval(term)? {
        Some(ModelValue::Bool(value)) => Ok(value),
        Some(ModelValue::Int(_)) => Err(VerifyError::new("ranking condition is an integer")),
        None => Err(VerifyError::new(
            "ranking condition is unavailable in model",
        )),
    }
}

const HELPFUL_ARG_HINT: &str = "helpful action arguments must be state-independent leadsto \
    binders, constants, enum members, or arithmetic (+, -, *) over those values";

const HELPFUL_PROGRESS_HINT: &str = "helpful marks which action instance is responsible for \
    progress. The matching action must be declared `fair action`, must be enabled whenever the \
    obligation is pending, and must strictly decrease the rank when it fires; other actions must \
    keep the pending obligation true (unless they make Q true) and must not increase the measure \
    -- an unbounded increase between helpful firings can outpace the guaranteed decrease and \
    prevent Q from ever being reached";

/// Evaluate one `helpful` action argument expression to a concrete value.
///
/// `helpful` arguments select a per-binding action instance; they must be
/// state-independent (leadsTo binders, constants, enum members, or `+`/`-`/`*`
/// over those values), matching the frozen Python reference's
/// `_helpful_arg_value`.
///
/// Recurses on itself through `Neg` and both `Binary` operands, so it is its own
/// cycle entry for `recursion::guard` (#1164). Crash-witnessed by `verify
/// --engine induction` on a `helpful step(c + 0 + ... + 0)` argument: 4000
/// terms on a debug aarch64 build, 20000 on release.
///
/// # Errors
///
/// Returns [`VerifyError`] when the expression references anything other than
/// a bound leadsTo binder or a constant, or does not fold to an integer,
/// Boolean, or enum-member value.
fn eval_state_independent(
    expr: &KernelExpr,
    binder_env: &BTreeMap<String, FslValue>,
) -> Result<FslValue, VerifyError> {
    recursion::guard(|| eval_state_independent_inner(expr, binder_env))
}

fn eval_state_independent_inner(
    expr: &KernelExpr,
    binder_env: &BTreeMap<String, FslValue>,
) -> Result<FslValue, VerifyError> {
    match expr {
        KernelExpr::Num(value) => Ok(FslValue::Int(*value)),
        KernelExpr::Bool(value) => Ok(FslValue::Bool(*value)),
        KernelExpr::EnumMember { type_name, member } => Ok(FslValue::Enum {
            type_name: type_name.clone(),
            member: member.clone(),
        }),
        KernelExpr::Var(name) => binder_env
            .get(name)
            .cloned()
            .ok_or_else(|| VerifyError::new(format!("{HELPFUL_ARG_HINT}: unknown name '{name}'"))),
        KernelExpr::Neg(inner) => match eval_state_independent(inner, binder_env)? {
            FslValue::Int(value) => Ok(FslValue::Int(-value)),
            _ => Err(VerifyError::new(HELPFUL_ARG_HINT)),
        },
        KernelExpr::Binary { op, left, right } => {
            let left = eval_state_independent(left, binder_env)?;
            let right = eval_state_independent(right, binder_env)?;
            match (op.as_str(), left, right) {
                ("+", FslValue::Int(a), FslValue::Int(b)) => Ok(FslValue::Int(a + b)),
                ("-", FslValue::Int(a), FslValue::Int(b)) => Ok(FslValue::Int(a - b)),
                ("*", FslValue::Int(a), FslValue::Int(b)) => Ok(FslValue::Int(a * b)),
                _ => Err(VerifyError::new(HELPFUL_ARG_HINT)),
            }
        }
        _ => Err(VerifyError::new(HELPFUL_ARG_HINT)),
    }
}

/// Resolve a leadsTo's declared `helpful` action entries against the
/// enumerated action instances, for one concrete leadsTo binding.
///
/// Mirrors the frozen Python reference's `_helpful_matches`: a `helpful`
/// entry naming an undeclared action is skipped rather than erroring here,
/// since `fslc check` (`validate_model_expression_types`) already rejects
/// that at the static-check stage.
///
/// # Errors
///
/// Returns [`VerifyError`] when a `helpful` argument expression is not
/// state-independent (see [`eval_state_independent`]).
fn helpful_matches<S: SmtSolver>(
    model: &fsl_core::KernelModel,
    leadsto: &LeadsToDef,
    binder_concrete: &BTreeMap<String, FslValue>,
    instances: &[ActionInstance<S::Term>],
) -> Result<Vec<usize>, VerifyError> {
    let mut out = Vec::new();
    for helper in &leadsto.helpful {
        let Some(action) = model
            .actions
            .iter()
            .find(|action| action.name == helper.action)
        else {
            continue;
        };
        let param_names = action
            .params
            .iter()
            .map(fsl_core::ParamDef::name)
            .collect::<Vec<_>>();
        let expected = helper
            .args
            .iter()
            .map(|arg| eval_state_independent(arg, binder_concrete))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, instance) in instances.iter().enumerate() {
            if instance.action != helper.action {
                continue;
            }
            let matches = param_names
                .iter()
                .zip(&expected)
                .all(|(name, value)| instance.concrete_params.get(*name) == Some(value));
            if matches {
                out.push(index);
            }
        }
    }
    Ok(out)
}

fn helpful_witnesses<S: SmtSolver>(
    indices: &[usize],
    instances: &[ActionInstance<S::Term>],
) -> Vec<HelpfulActionWitness> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for &index in indices {
        if !seen.insert(index) {
            continue;
        }
        let instance = &instances[index];
        out.push(HelpfulActionWitness {
            action: instance.action.clone(),
            params: instance.concrete_params.clone(),
        });
    }
    out
}

/// Prove explicitly ranked `leadsTo` properties over one arbitrary transition.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported measures, symbolic expressions, or
/// solver failures.
pub async fn prove_ranked_leadstos<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
) -> Result<RankedLeadstoResult, VerifyError> {
    prove_ranked_leadstos_assuming(model, solver, None).await
}

/// The positions in `model.leadstos` of the ranked `leadsTo` properties whose
/// bounded fair-lasso search a BMC run may skip (#1149), because the ranking
/// obligations of [`prove_ranked_leadstos`] hold for them over every state
/// that satisfies exactly the properties that same BMC run checks at every
/// unrolled step.
///
/// Properties are identified by position, never by name: `check` accepts two
/// `leadsTo` blocks with the same name, and a name key would let one block's
/// ranking proof withdraw the other block's search.
///
/// `checked_bounds` must be the implicit type-bound selection the BMC run
/// uses (`None` = every `_bounds_*`); a bound the run does not check is not
/// assumed here, because an unrolled state may violate it. The ranking stops
/// at its first failing property, so only the properties proved before it
/// are returned. Any ranking error (unsupported measure, solver `unknown` --
/// including a solver timeout -- or fail-closed filters) returns the empty
/// set: the fast path never reports anything itself, it only withdraws probes
/// whose answer it has already shown to be `unsat`. See
/// `docs/design/DESIGN-induction.md` §2.5.
pub async fn ranked_leadsto_lasso_discharges<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    checked_bounds: Option<&BTreeSet<String>>,
) -> BTreeSet<usize> {
    let ranked = model
        .leadstos
        .iter()
        .enumerate()
        .filter(|(_, property)| property.decreases.is_some())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if ranked.is_empty() {
        return BTreeSet::new();
    }
    let Ok(result) = prove_ranked_leadstos_assuming(model, solver, checked_bounds).await else {
        return BTreeSet::new();
    };
    // The ranking walks `model.leadstos` in order, skips the unranked ones,
    // and pushes one proof per ranked property until its first failure, so
    // its proofs are the ranked properties' prefix, position for position.
    // The length and by-name check below therefore cannot fail by
    // construction (names may repeat, but position i's proof carries
    // position i's name); it exists only so that a future reordering of that
    // loop discharges nothing instead of the wrong property.
    if result.proofs.len() > ranked.len()
        || result
            .proofs
            .iter()
            .zip(&ranked)
            .any(|(proof, &index)| proof.name != model.leadstos[index].name)
    {
        return BTreeSet::new();
    }
    ranked.into_iter().take(result.proofs.len()).collect()
}

#[allow(clippy::too_many_lines)]
async fn prove_ranked_leadstos_assuming<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<RankedLeadstoResult, VerifyError> {
    let instances = action_instances(solver, model)?;
    let state0 = symbolic_state_with_suffix(solver, model, "rank0")?;
    let state1 = symbolic_state_with_suffix(solver, model, "rank1")?;
    // The no_deadlock obligation ranges over every invariant state, so it is
    // asked about a state the transition below does not constrain: `state0`
    // always has its selected action enabled, which would hide exactly the
    // pending states in which nothing is enabled (#1189).
    let deadlock_state = symbolic_state_with_suffix(solver, model, "rank_deadlock")?;
    for property in properties(model) {
        if let (Property::Bound(_), Some(selected)) = (property, checked_bounds)
            && !selected.contains(&property.name(model))
        {
            continue;
        }
        solver.assert(&property_condition(solver, model, property, &state0, None)?)?;
        solver.assert(&property_condition(
            solver,
            model,
            property,
            &deadlock_state,
            None,
        )?)?;
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
        solver.set_query_context("leadsTo_rank", &property.name);
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
                            kind: violation_kind::UNBOUNDED_BELOW.to_owned(),
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
                            helpful: Vec::new(),
                            helpful_actions: Vec::new(),
                        }),
                    });
                }
                SatResult::Unsat => solver.pop(1)?,
                SatResult::Unknown => {
                    solver.pop(1)?;
                    return Err(VerifyError::new("solver returned unknown in ranking proof"));
                }
            }

            let helpful_idx = helpful_matches::<S>(model, property, &binding.concrete, &instances)?;

            if !property.helpful.is_empty() {
                // helpful_fairness: the matching helpful action instance(s)
                // must be declared `fair action`; helpful metadata alone does
                // not create a fairness assumption.
                let nonfair = helpful_idx
                    .iter()
                    .copied()
                    .filter(|&index| !model.actions[instances[index].action_index].fair)
                    .collect::<Vec<_>>();
                if !nonfair.is_empty() {
                    solver.push();
                    solver.assert(&pending)?;
                    match solver.check().await? {
                        SatResult::Sat => {
                            let measure_value = model_int(solver, &measure0)?;
                            let trace =
                                project_trace(solver, model, &[state0], &[], &instances, 0)?;
                            solver.pop(1)?;
                            return Ok(RankedLeadstoResult {
                                proofs,
                                failure: Some(RankFailure {
                                    name: property.name.clone(),
                                    bindings: binding.concrete,
                                    measure: measure_expr.clone(),
                                    kind: violation_kind::PROGRESS_ACTION_NOT_FAIR.to_owned(),
                                    measure_value: Some(measure_value),
                                    measure_before: None,
                                    measure_after: None,
                                    action: None,
                                    trace,
                                    hint: "helpful only identifies the per-binding progress action. Add `fair` to the lower-layer action instance that must eventually run; helpful does not create a fairness assumption.".to_owned(),
                                    message: format!(
                                        "leadsTo '{}' uses helpful action metadata, but a matching progress action is not declared fair",
                                        property.name
                                    ),
                                    helpful: property.helpful.clone(),
                                    helpful_actions: helpful_witnesses::<S>(&nonfair, &instances),
                                }),
                            });
                        }
                        SatResult::Unsat => solver.pop(1)?,
                        SatResult::Unknown => {
                            solver.pop(1)?;
                            return Err(VerifyError::new(
                                "solver returned unknown in ranking proof",
                            ));
                        }
                    }
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
            let keeps_pending = solver.and(&[p1.clone(), decreases])?;
            let progresses = solver.or(&[q1.clone(), keeps_pending])?;

            // helpful_sticky: with two or more matching helpful instances, a
            // single enabled disjunct is not enough -- once one becomes
            // enabled while pending, it must stay enabled until it fires (or
            // Q resolves), or its `fair` declaration is never obligated to
            // fire because a *different* instance can satisfy the
            // disjunction at every step.
            if helpful_idx.len() >= 2 {
                for &sticky_index in &helpful_idx {
                    let action = &model.actions[instances[sticky_index].action_index];
                    let (guards0, _) = crate::transition::action_guards(
                        solver,
                        model,
                        action,
                        &state0,
                        &instances[sticky_index].params,
                    )?;
                    let enabled0 = solver.and(&guards0)?;
                    let (guards1, _) = crate::transition::action_guards(
                        solver,
                        model,
                        action,
                        &state1,
                        &instances[sticky_index].params,
                    )?;
                    let enabled1 = solver.and(&guards1)?;
                    let not_selected = solver.not(
                        &solver.equal(&choice, &solver.int_value(i64_index(sticky_index)?))?,
                    )?;
                    let flickers = solver.and(&[
                        pending.clone(),
                        enabled0,
                        not_selected,
                        solver.not(&q1)?,
                        solver.not(&enabled1)?,
                    ])?;
                    solver.push();
                    solver.assert(&flickers)?;
                    match solver.check().await? {
                        SatResult::Sat => {
                            let trace = project_trace(
                                solver,
                                model,
                                &[state0, state1],
                                std::slice::from_ref(&choice),
                                &instances,
                                1,
                            )?;
                            let last_action = trace
                                .get(1)
                                .and_then(|step| step.action.as_ref())
                                .map(|action| action.name.clone());
                            solver.pop(1)?;
                            let witnesses = helpful_witnesses::<S>(
                                std::slice::from_ref(&sticky_index),
                                &instances,
                            );
                            let witness_name = witnesses
                                .first()
                                .map_or_else(String::new, |witness| witness.action.clone());
                            return Ok(RankedLeadstoResult {
                                proofs,
                                failure: Some(RankFailure {
                                    name: property.name.clone(),
                                    bindings: binding.concrete,
                                    measure: measure_expr.clone(),
                                    kind: violation_kind::HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY
                                        .to_owned(),
                                    measure_value: None,
                                    measure_before: None,
                                    measure_after: None,
                                    action: last_action,
                                    trace,
                                    hint: "with more than one `helpful` action, each instance's enabledness must not flicker: once a helpful instance becomes enabled while the obligation is pending, it must stay enabled until it fires (or Q holds) -- otherwise its weak fairness is never triggered. Guard the other actions so they cannot disable a pending helpful instance, or split into a leadsTo per helpful action so each owns a stable region".to_owned(),
                                    message: format!(
                                        "helpful action '{witness_name}' can become disabled again while leadsTo '{}' is still pending, without itself having fired",
                                        property.name
                                    ),
                                    helpful: property.helpful.clone(),
                                    helpful_actions: witnesses,
                                }),
                            });
                        }
                        SatResult::Unsat => solver.pop(1)?,
                        SatResult::Unknown => {
                            solver.pop(1)?;
                            return Err(VerifyError::new(
                                "solver returned unknown in ranking proof",
                            ));
                        }
                    }
                }
            }

            // no_deadlock: a pending obligation must not reach a state where
            // no action (with `helpful`: no matching helpful instance) is
            // enabled -- nothing is then obligated to fire, so the ranking
            // argument never runs. Mirrors the frozen Python reference's
            // `_prove_leadsto_rank_no_deadlock`.
            let p_dl = leadsto_condition(
                solver,
                model,
                &property.before,
                &deadlock_state,
                &binding.symbolic,
            )?;
            let q_dl = leadsto_condition(
                solver,
                model,
                &property.after,
                &deadlock_state,
                &binding.symbolic,
            )?;
            let enabled_candidates = if property.helpful.is_empty() {
                (0..instances.len()).collect::<Vec<_>>()
            } else {
                helpful_idx.clone()
            };
            let mut enabled_terms = Vec::with_capacity(enabled_candidates.len());
            for &deadlock_index in &enabled_candidates {
                let action = &model.actions[instances[deadlock_index].action_index];
                let (guards, _) = crate::transition::action_guards(
                    solver,
                    model,
                    action,
                    &deadlock_state,
                    &instances[deadlock_index].params,
                )?;
                enabled_terms.push(solver.and(&guards)?);
            }
            let any_enabled = if enabled_terms.is_empty() {
                solver.bool_value(false)
            } else {
                solver.or(&enabled_terms)?
            };
            let mut deadlock_bindings = binding.symbolic.clone();
            let measure_dl = eval(
                solver,
                model,
                measure_expr,
                &deadlock_state,
                &mut deadlock_bindings,
                None,
            )?;
            let measure_dl = int_term(&measure_dl)?.clone();
            solver.push();
            solver.assert(&p_dl)?;
            solver.assert(&solver.not(&q_dl)?)?;
            solver.assert(&solver.not(&any_enabled)?)?;
            match solver.check().await? {
                SatResult::Sat => {
                    let measure_value = model_int(solver, &measure_dl)?;
                    let trace =
                        project_trace(solver, model, &[deadlock_state], &[], &instances, 0)?;
                    solver.pop(1)?;
                    let failure = if property.helpful.is_empty() {
                        RankFailure {
                            name: property.name.clone(),
                            bindings: binding.concrete,
                            measure: measure_expr.clone(),
                            kind: violation_kind::DEADLOCK.to_owned(),
                            measure_value: Some(measure_value),
                            measure_before: None,
                            measure_after: None,
                            action: None,
                            trace,
                            hint: "a pending leadsTo obligation must not reach a state with no enabled action before Q holds".to_owned(),
                            message: format!(
                                "leadsTo '{}' can be pending in a state with no enabled action",
                                property.name
                            ),
                            helpful: Vec::new(),
                            helpful_actions: Vec::new(),
                        }
                    } else {
                        RankFailure {
                            name: property.name.clone(),
                            bindings: binding.concrete,
                            measure: measure_expr.clone(),
                            kind: violation_kind::HELPFUL_ACTION_NOT_ENABLED.to_owned(),
                            measure_value: Some(measure_value),
                            measure_before: None,
                            measure_after: None,
                            action: None,
                            trace,
                            hint: HELPFUL_PROGRESS_HINT.to_owned(),
                            message: format!(
                                "leadsTo '{}' can be pending while no matching helpful action instance is enabled",
                                property.name
                            ),
                            helpful: property.helpful.clone(),
                            helpful_actions: helpful_witnesses::<S>(&helpful_idx, &instances),
                        }
                    };
                    return Ok(RankedLeadstoResult {
                        proofs,
                        failure: Some(failure),
                    });
                }
                SatResult::Unsat => solver.pop(1)?,
                SatResult::Unknown => {
                    solver.pop(1)?;
                    return Err(VerifyError::new("solver returned unknown in ranking proof"));
                }
            }

            if property.helpful.is_empty() {
                for (index, instance) in instances.iter().enumerate() {
                    let selected = solver.equal(&choice, &solver.int_value(i64_index(index)?))?;
                    let failure =
                        solver.and(&[pending.clone(), selected, solver.not(&progresses)?])?;
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
                                    kind: violation_kind::NON_DECREASING_ACTION.to_owned(),
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
                                    helpful: Vec::new(),
                                    helpful_actions: Vec::new(),
                                }),
                            });
                        }
                        SatResult::Unsat => solver.pop(1)?,
                        SatResult::Unknown => {
                            solver.pop(1)?;
                            return Err(VerifyError::new(
                                "solver returned unknown in ranking proof",
                            ));
                        }
                    }
                }
            } else {
                let helpful_le = solver.le(&measure1, &measure0)?;
                let pending_preserved =
                    solver.or(&[q1.clone(), solver.and(&[p1.clone(), helpful_le])?])?;
                for (index, instance) in instances.iter().enumerate() {
                    let is_helpful = helpful_idx.contains(&index);
                    let allowed = if is_helpful {
                        progresses.clone()
                    } else {
                        pending_preserved.clone()
                    };
                    let selected = solver.equal(&choice, &solver.int_value(i64_index(index)?))?;
                    let failure =
                        solver.and(&[pending.clone(), selected, solver.not(&allowed)?])?;
                    solver.push();
                    solver.assert(&failure)?;
                    match solver.check().await? {
                        SatResult::Sat => {
                            let before = model_int(solver, &measure0)?;
                            let after = model_int(solver, &measure1)?;
                            let q_next_holds = model_bool(solver, &q1)?;
                            let p_next_holds = model_bool(solver, &p1)?;
                            let kind = if !q_next_holds && !p_next_holds {
                                violation_kind::PENDING_NOT_PRESERVED
                            } else if is_helpful && after >= before {
                                violation_kind::NON_DECREASING_HELPFUL_ACTION
                            } else if !is_helpful && after > before {
                                violation_kind::NON_HELPFUL_ACTION_INCREASES_MEASURE
                            } else {
                                violation_kind::NON_DECREASING_ACTION
                            };
                            let message = match kind {
                                violation_kind::PENDING_NOT_PRESERVED => format!(
                                    "enabled action '{}' can make leadsTo '{}' no longer pending without making Q true",
                                    instance.action, property.name
                                ),
                                violation_kind::NON_DECREASING_HELPFUL_ACTION => format!(
                                    "helpful action '{}' can leave leadsTo '{}' pending without strictly decreasing the measure",
                                    instance.action, property.name
                                ),
                                violation_kind::NON_HELPFUL_ACTION_INCREASES_MEASURE => format!(
                                    "non-helpful action '{}' can increase the measure while leadsTo '{}' is still pending, which could outpace the helpful action's guaranteed decrease",
                                    instance.action, property.name
                                ),
                                _ => format!(
                                    "enabled action '{}' can leave leadsTo '{}' pending without strictly decreasing the measure",
                                    instance.action, property.name
                                ),
                            };
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
                                    kind: kind.to_owned(),
                                    measure_value: None,
                                    measure_before: Some(before),
                                    measure_after: Some(after),
                                    action: Some(instance.action.clone()),
                                    trace,
                                    hint: HELPFUL_PROGRESS_HINT.to_owned(),
                                    message,
                                    helpful: property.helpful.clone(),
                                    helpful_actions: helpful_witnesses::<S>(
                                        &helpful_idx,
                                        &instances,
                                    ),
                                }),
                            });
                        }
                        SatResult::Unsat => solver.pop(1)?,
                        SatResult::Unknown => {
                            solver.pop(1)?;
                            return Err(VerifyError::new(
                                "solver returned unknown in ranking proof",
                            ));
                        }
                    }
                }
            }
        }
        proofs.push(RankProof {
            name: property.name.clone(),
            measure: measure_expr.clone(),
            helpful: property.helpful.clone(),
        });
    }
    Ok(RankedLeadstoResult {
        proofs,
        failure: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1164: `eval_state_independent` recursed without the stack guard.
    ///
    /// Tested here rather than through `fslc` because the CLI cannot open this
    /// window reliably: on a debug aarch64 build the unguarded walk aborts a
    /// `helpful step(c + 0 + ... + 0)` argument between 3800 and 3900 terms,
    /// while the derived `Clone` of the parsed tree (#1186) aborts at 4500, so a
    /// CLI witness would sit within a few hundred terms of both. Built directly,
    /// the tree can be far deeper than either. It is leaked rather than dropped
    /// because the derived `Drop` recurses as deeply as the derived `Clone`.
    #[test]
    fn a_deep_helpful_argument_folds_instead_of_overflowing_the_stack() {
        const TERMS: i64 = 100_000;
        let mut expr = KernelExpr::Var("c".to_owned());
        for _ in 0..TERMS {
            expr = KernelExpr::Binary {
                op: "+".to_owned(),
                left: Box::new(expr),
                right: Box::new(KernelExpr::Num(1)),
            };
        }
        let expr = Box::leak(Box::new(expr));
        let binder_env = BTreeMap::from([("c".to_owned(), FslValue::Int(0))]);

        let value = eval_state_independent(expr, &binder_env).expect("folds");

        assert_eq!(value, FslValue::Int(TERMS));
    }
}
