// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Level-synchronous explicit-state verification over concrete monitor states.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    FslValue as Value, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue,
    KernelModel, KernelStatement as Statement, TraceAction, TraceStep, TypeDef, TypeRef,
};

use super::trace::{ParentLink, reconstruct_trace, state_changes};
use super::{
    Bindings, Monitor, RuntimeError, State, Violation, eval, runtime_error, with_total_division,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplicitViolation {
    pub violation: Violation,
    pub trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplicitReachableWitness {
    pub step: usize,
    pub trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplicitResult {
    pub spec: String,
    pub depth: usize,
    pub depth_reached: usize,
    pub states_explored: usize,
    pub max_frontier_width: usize,
    pub closure: bool,
    pub budget_exceeded: bool,
    pub violation: Option<ExplicitViolation>,
    pub reachables: BTreeMap<String, Option<ExplicitReachableWitness>>,
    pub deadlock_step: Option<usize>,
    pub deadlock_trace: Option<Vec<TraceStep>>,
    pub action_coverage: BTreeMap<String, bool>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum InitWriteKey {
    Root(String),
    ConcreteIndex(String, Value),
}

/// Verify a finite kernel model using level-synchronous concrete BFS.
///
/// # Errors
///
/// Returns [`RuntimeError`] when initialization is not deterministic, a
/// concrete expression cannot be evaluated, or the model uses an unsupported
/// explicit-engine feature.
pub fn verify_explicit(
    model: KernelModel,
    depth: usize,
    max_states: usize,
) -> Result<ExplicitResult, RuntimeError> {
    verify_explicit_selected(model, depth, max_states, None)
}

/// Fail-closed gate: `Some(reason)` when the explicit engine cannot verify
/// this model at all (checked statically, before any exploration starts).
///
/// Used both by [`verify_explicit_selected`] (to reject unsupported models
/// the same way it always has) and by the `--engine auto` dispatcher (to
/// decide, before spending any exploration budget, whether to fall back to
/// the symbolic engine).
#[must_use]
pub fn explicit_unsupported_reason(model: &KernelModel) -> Option<String> {
    if let Err(error) = check_deterministic_init(model) {
        return Some(error.message);
    }
    if !model.leadstos.is_empty() {
        return Some(
            "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property"
                .to_owned(),
        );
    }
    None
}

/// Build the concrete initial state only when init definitely assigns every state component.
///
/// # Errors
///
/// Returns [`RuntimeError`] when init is nondeterministic, partially assigned, or cannot be
/// evaluated concretely.
pub fn deterministic_initial_state(model: &KernelModel) -> Result<State, RuntimeError> {
    // `Monitor::new` itself now runs this same deterministic-init gate, so
    // there is no separate check to run here.
    Ok(Monitor::new(model.clone())?.state)
}

/// Verify with an optional set of selected implicit state-bound properties.
///
/// # Errors
///
/// Returns the same errors as [`verify_explicit`].
#[allow(clippy::too_many_lines)]
pub fn verify_explicit_selected(
    model: KernelModel,
    depth: usize,
    max_states: usize,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<ExplicitResult, RuntimeError> {
    if max_states == 0 {
        return Err(runtime_error("explicit state budget must be at least 1"));
    }
    if let Some(reason) = explicit_unsupported_reason(&model) {
        return Err(runtime_error(reason));
    }

    // The frontier carries `State` only, not `Monitor` -- a full `Monitor`
    // clone per child, on top of a `BTreeMap<State, Monitor>` frontier
    // holding one whole `KernelModel` per live state, duplicated the model
    // at both layers (issue #730). `scratch` is re-pointed at each state
    // instead; `parents` (already used for trace reconstruction) is the
    // only per-state bookkeeping kept.
    let mut scratch = Monitor::new(model)?;
    let initial_state = scratch.state.clone();
    let mut result = ExplicitResult {
        spec: scratch.model.name.clone(),
        depth,
        depth_reached: 0,
        states_explored: 1,
        max_frontier_width: 1,
        closure: false,
        budget_exceeded: false,
        violation: None,
        reachables: scratch
            .model
            .reachables
            .iter()
            .map(|property| (property.name.clone(), None))
            .collect(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: scratch
            .model
            .actions
            .iter()
            .map(|action| (action.name.clone(), false))
            .collect(),
    };
    let mut frontier = BTreeSet::from([initial_state.clone()]);
    let mut seen = BTreeSet::from([initial_state]);
    let mut parents = BTreeMap::<State, ParentLink>::new();

    for level in 0..=depth {
        result.depth_reached = level;
        result.max_frontier_width = result.max_frontier_width.max(frontier.len());

        for state in &frontier {
            scratch.state = state.clone();
            scratch.step = level;
            if let Some(violation) = scratch.current_violation_selected(checked_bounds)? {
                result.violation = Some(ExplicitViolation {
                    trace: reconstruct_trace(state, &parents),
                    violation,
                });
                return Ok(result);
            }
            if let Some(violation) = record_reachables(&scratch, level, &parents, &mut result)? {
                result.violation = Some(ExplicitViolation {
                    trace: reconstruct_trace(state, &parents),
                    violation,
                });
                return Ok(result);
            }
        }

        let mut enabled_by_state = BTreeMap::new();
        for state in &frontier {
            scratch.state = state.clone();
            scratch.step = level;
            let enabled = scratch.enabled()?;
            for instance in &enabled {
                result.action_coverage.insert(instance.action.clone(), true);
            }
            if enabled.is_empty() && result.deadlock_step.is_none() {
                let terminal = match terminal_holds(&scratch) {
                    Ok(value) => value,
                    Err(error) if super::is_partial_operation_error(&error.message) => {
                        result.violation = Some(ExplicitViolation {
                            trace: reconstruct_trace(state, &parents),
                            violation: Violation {
                                kind: "partial_op".to_owned(),
                                name: "_partial_property_terminal".to_owned(),
                                step: level,
                            },
                        });
                        return Ok(result);
                    }
                    Err(error) => return Err(error),
                };
                if !terminal {
                    result.deadlock_step = Some(level);
                    result.deadlock_trace = Some(reconstruct_trace(state, &parents));
                }
            }
            enabled_by_state.insert(state.clone(), enabled);
        }

        if level == depth {
            break;
        }

        let mut next = BTreeSet::new();
        for state in &frontier {
            for instance in &enabled_by_state[state] {
                scratch.state = state.clone();
                scratch.step = level;
                let stepped = scratch.step_selected(instance, checked_bounds)?;
                if let Some(violation) = stepped.violation {
                    // The Monitor rolls back on violation (`state` is the pre-step
                    // state); the trace must show the attempted post-state.
                    let after = stepped.attempted_state.as_ref().unwrap_or(&stepped.state);
                    let mut trace = reconstruct_trace(state, &parents);
                    trace.push(edge_trace_step(level + 1, state, instance, after));
                    result.depth_reached = level + 1;
                    result.violation = Some(ExplicitViolation { violation, trace });
                    return Ok(result);
                }
                let child_state = scratch.state.clone();
                if seen.contains(&child_state) {
                    continue;
                }
                if seen.len() >= max_states {
                    result.states_explored = seen.len();
                    result.budget_exceeded = true;
                    return Ok(result);
                }
                seen.insert(child_state.clone());
                parents.insert(
                    child_state.clone(),
                    ParentLink {
                        parent: state.clone(),
                        action: TraceAction {
                            name: instance.action.clone(),
                            params: instance.params.clone(),
                        },
                    },
                );
                next.insert(child_state);
            }
        }
        result.states_explored = seen.len();
        if next.is_empty() {
            result.closure = true;
            return Ok(result);
        }
        frontier = next;
    }

    result.states_explored = seen.len();
    Ok(result)
}

fn terminal_holds(monitor: &Monitor) -> Result<bool, RuntimeError> {
    let Some(terminal) = &monitor.model.terminal else {
        return Ok(false);
    };
    with_total_division(|| {
        match eval(
            terminal,
            &monitor.state,
            &mut Bindings::new(),
            &monitor.model,
            None,
        )? {
            Value::Bool(value) => Ok(value),
            _ => Err(runtime_error("terminal expression must be Boolean")),
        }
    })
}

fn record_reachables(
    monitor: &Monitor,
    level: usize,
    parents: &BTreeMap<State, ParentLink>,
    result: &mut ExplicitResult,
) -> Result<Option<Violation>, RuntimeError> {
    with_total_division(|| {
        for property in &monitor.model.reachables {
            if result.reachables[&property.name].is_some() {
                continue;
            }
            let value = match eval(
                &property.expr,
                &monitor.state,
                &mut Bindings::new(),
                &monitor.model,
                None,
            ) {
                Ok(value) => value,
                Err(error) if super::is_partial_operation_error(&error.message) => {
                    return Ok(Some(Violation {
                        kind: "partial_op".to_owned(),
                        name: format!("_partial_property_{}", property.name),
                        step: level,
                    }));
                }
                Err(error) => return Err(error),
            };
            match value {
                Value::Bool(true) => {
                    result.reachables.insert(
                        property.name.clone(),
                        Some(ExplicitReachableWitness {
                            step: level,
                            trace: reconstruct_trace(&monitor.state, parents),
                        }),
                    );
                }
                Value::Bool(false) => {}
                _ => return Err(runtime_error("reachable expression must be Boolean")),
            }
        }
        Ok(None)
    })
}

fn edge_trace_step(
    step: usize,
    before: &State,
    instance: &super::EnabledAction,
    state: &State,
) -> TraceStep {
    TraceStep {
        step,
        state: state.clone(),
        action: Some(TraceAction {
            name: instance.action.clone(),
            params: instance.params.clone(),
        }),
        changes: state_changes(before, state),
    }
}

/// Per-root definite-assignment coverage tracked at component granularity.
///
/// A `Map` root is fully assigned only once every concrete key in its
/// key-type domain has been written; a `Named` struct root is fully assigned
/// only once every declared field has been (recursively) fully assigned.
/// Anything else stays binary (`Full` or unassigned) because there is no
/// finer component structure to track.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Coverage {
    Full,
    Keys(BTreeSet<Value>),
    Fields(BTreeMap<String, Coverage>),
}

/// Names and declared types of state roots that `model`'s init never
/// assigns on *any* path — fully free / unconstrained by init, per the same
/// symbolic-free-variable semantics `fsl-verifier`'s BMC init lowering gives
/// an omitted assignment (DESIGN-init-if.md; issue #493).
///
/// This is deliberately simpler than — and not layered on — the
/// [`check_deterministic_init`]/[`walk_init`] coverage gate: that walk
/// rejects a condition/index expression reading a not-yet-assigned root as
/// an error (`init references state variable '...' before it is
/// assigned`), which is exactly the pattern a nondeterministic `init if`
/// legitimately uses (the motivating case for #493). This scan instead only
/// asks whether a root is ever an assignment *target* anywhere in init,
/// regardless of read order or which branch is taken; it never errors and
/// never rejects a read.
///
/// A root assigned on *some* but not all paths (e.g. only inside an `if`
/// with no `else`) is left out — treated as "assigned", not free — and
/// keeps the prior default-filled behavior for the remaining path. Fully
/// characterizing that partial-coverage case belongs to `Monitor`
/// construction generally (issue #519), not this refinement-local
/// enumeration.
pub(crate) fn unassigned_init_state_vars(model: &KernelModel) -> Vec<(String, TypeRef)> {
    let mut assigned = BTreeSet::new();
    collect_assigned_init_roots(&model.init, &mut assigned);
    model
        .state
        .iter()
        .filter(|(name, _)| !assigned.contains(name.as_str()))
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect()
}

fn collect_assigned_init_roots(statements: &[Statement], assigned: &mut BTreeSet<String>) {
    for statement in statements {
        match statement {
            Statement::Assign { target, .. } => {
                if let Some(name) = logical_var(target) {
                    assigned.insert(name.to_owned());
                }
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_assigned_init_roots(then_statements, assigned);
                collect_assigned_init_roots(else_statements, assigned);
            }
            Statement::ForAll { statements, .. } => {
                collect_assigned_init_roots(statements, assigned);
            }
        }
    }
}

pub(crate) fn check_deterministic_init(model: &KernelModel) -> Result<(), RuntimeError> {
    let (assigned, _) = walk_init(
        &model.init,
        BTreeMap::new(),
        BTreeSet::new(),
        false,
        &BTreeMap::new(),
        model,
    )?;
    let mut missing_is_partial = false;
    let mut missing = model
        .state
        .iter()
        .filter(|(name, ty)| {
            let coverage = assigned.get(name);
            if coverage_is_full(coverage, ty, model) {
                false
            } else {
                missing_is_partial = missing_is_partial || coverage.is_some();
                true
            }
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    missing.sort();
    if missing.is_empty() {
        Ok(())
    } else {
        let suffix = if missing_is_partial {
            " (partial component initialization is rejected by the explicit engine)"
        } else {
            ""
        };
        Err(runtime_error(format!(
            "init does not assign state variable(s): {}{suffix}",
            missing.join(", ")
        )))
    }
}

/// Whether `coverage` amounts to a complete definite assignment of `ty`.
fn coverage_is_full(coverage: Option<&Coverage>, ty: &TypeRef, model: &KernelModel) -> bool {
    match coverage {
        Some(Coverage::Full) => true,
        Some(Coverage::Keys(keys)) => match ty {
            TypeRef::Map(key_ty, _) => model
                .map_key_values(key_ty)
                .is_ok_and(|domain| domain.iter().all(|value| keys.contains(value))),
            _ => false,
        },
        Some(Coverage::Fields(fields)) => match ty {
            TypeRef::Named(name) => match model.types.get(name) {
                Some(TypeDef::Struct { fields: declared }) => {
                    declared.iter().all(|(field_name, field_ty)| {
                        fields.get(field_name).is_some_and(|field_coverage| {
                            coverage_is_full(Some(field_coverage), field_ty, model)
                        })
                    })
                }
                _ => false,
            },
            _ => false,
        },
        None => false,
    }
}

/// Join the coverage contributed by one more assignment into what a root
/// already had within the same straight-line branch. `Full` absorbs
/// anything; same-kind coverage unions (`Keys`) or merges per-field
/// (`Fields`); a kind mismatch keeps whatever was already recorded rather
/// than erroring (component tracking degrades to "no worse than before").
fn join_coverage(existing: Option<Coverage>, addition: Coverage) -> Coverage {
    let Some(existing) = existing else {
        return addition;
    };
    match (existing, addition) {
        (Coverage::Full, _) | (_, Coverage::Full) => Coverage::Full,
        (Coverage::Keys(mut a), Coverage::Keys(b)) => {
            a.extend(b);
            Coverage::Keys(a)
        }
        (Coverage::Fields(mut a), Coverage::Fields(b)) => {
            for (field, addition_coverage) in b {
                let merged = match a.remove(&field) {
                    Some(existing_coverage) => {
                        join_coverage(Some(existing_coverage), addition_coverage)
                    }
                    None => addition_coverage,
                };
                a.insert(field, merged);
            }
            Coverage::Fields(a)
        }
        (existing, _) => existing,
    }
}

/// Intersect the coverage two `if` branches leave behind. `Full` yields the
/// other side; same-kind coverage intersects (`Keys`) or meets per-field
/// (`Fields`, dropping fields missing on either side); a kind mismatch drops
/// the root entirely, matching the old set-intersection behavior for roots
/// only assigned on one side.
fn meet_coverage(a: &Coverage, b: &Coverage) -> Option<Coverage> {
    match (a, b) {
        (Coverage::Full, other) | (other, Coverage::Full) => Some(other.clone()),
        (Coverage::Keys(a), Coverage::Keys(b)) => {
            Some(Coverage::Keys(a.intersection(b).cloned().collect()))
        }
        (Coverage::Fields(a), Coverage::Fields(b)) => {
            let mut merged = BTreeMap::new();
            for (field, coverage_a) in a {
                if let Some(coverage_b) = b.get(field)
                    && let Some(meet) = meet_coverage(coverage_a, coverage_b)
                {
                    merged.insert(field.clone(), meet);
                }
            }
            Some(Coverage::Fields(merged))
        }
        _ => None,
    }
}

fn meet_branches(
    then_map: BTreeMap<String, Coverage>,
    else_map: &BTreeMap<String, Coverage>,
) -> BTreeMap<String, Coverage> {
    let mut merged = BTreeMap::new();
    for (name, then_coverage) in then_map {
        if let Some(else_coverage) = else_map.get(&name)
            && let Some(meet) = meet_coverage(&then_coverage, else_coverage)
        {
            merged.insert(name, meet);
        }
    }
    merged
}

/// The finite domain a `forall` binder ranges over, when it is knowable
/// without evaluating against a concrete state — i.e. everything except a
/// `where`-filtered domain, a `Collection` binder, or `Range` bounds that
/// are not compile-time constants. `None` means the binder's body cannot be
/// used to prove full coverage (a `where` filter might skip every
/// iteration).
fn compute_binder_values(
    binder: &Binder,
    model: &KernelModel,
) -> Result<Option<Vec<Value>>, RuntimeError> {
    match binder {
        Binder::Typed {
            type_name,
            where_expr: None,
            ..
        } => {
            let ty = TypeRef::Named(super::qualified_type(type_name)?);
            Ok(Some(model.domain_values(&ty)?))
        }
        Binder::Range {
            lo,
            hi,
            where_expr: None,
            ..
        } => match (const_int_bound(lo, model), const_int_bound(hi, model)) {
            (Some(lo), Some(hi)) => Ok(Some((lo..=hi).map(Value::Int).collect())),
            _ => Ok(None),
        },
        Binder::Typed { .. } | Binder::Range { .. } | Binder::Collection { .. } => Ok(None),
    }
}

fn const_int_bound(expr: &Expr, model: &KernelModel) -> Option<i64> {
    match expr {
        Expr::Num(value) => Some(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

/// Resolve `name` as a member of the enum backing `key_type`, if any.
fn resolve_enum_key(model: &KernelModel, key_type: &TypeRef, name: &str) -> Option<Value> {
    model
        .domain_values(key_type)
        .ok()?
        .into_iter()
        .find(|value| matches!(value, Value::Enum { member, .. } if member == name))
}

/// Resolve `name` as an integer constant only when `key_type` is integer-valued.
///
/// Map-key coverage is typed: an integer constant must not stand in for an enum
/// member merely because they share a spelling.
fn resolve_integer_const_key(model: &KernelModel, key_type: &TypeRef, name: &str) -> Option<Value> {
    let is_integer_key = match key_type {
        TypeRef::Int | TypeRef::Range(_, _) => true,
        TypeRef::Named(type_name) => {
            matches!(model.types.get(type_name), Some(TypeDef::Domain { .. }))
        }
        TypeRef::Bool
        | TypeRef::Map(_, _)
        | TypeRef::Relation(_, _)
        | TypeRef::Set(_)
        | TypeRef::Seq(_, _)
        | TypeRef::Option(_) => false,
    };
    if !is_integer_key {
        return None;
    }
    match model.consts.get(name) {
        Some(Value::Int(value)) => Some(Value::Int(*value)),
        _ => None,
    }
}

/// The coverage a single assignment statement contributes to its logical
/// root, independent of whatever the root already had. Returns `None` when
/// the target's key/field shape carries no provable component information
/// (an unresolved dynamic map key, or a nested lvalue).
fn assignment_coverage(
    target: &LValue,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
) -> Option<Coverage> {
    match target {
        LValue::Var(_) => Some(Coverage::Full),
        LValue::Index(name, key_expr) => match key_expr {
            Expr::Num(value) => Some(Coverage::Keys(BTreeSet::from([Value::Int(*value)]))),
            Expr::Var(key) => {
                if let Some(binder_values) = bound_names.get(key) {
                    binder_values
                        .clone()
                        .map(|values| Coverage::Keys(values.into_iter().collect()))
                } else if let Some(TypeRef::Map(key_ty, _)) = model.state_type(name) {
                    resolve_enum_key(model, key_ty, key)
                        .or_else(|| resolve_integer_const_key(model, key_ty, key))
                        .map(|value| Coverage::Keys(BTreeSet::from([value])))
                } else {
                    None
                }
            }
            _ => None,
        },
        LValue::Field(base, field) => match base.as_ref() {
            LValue::Var(_) => Some(Coverage::Fields(BTreeMap::from([(
                field.clone(),
                Coverage::Full,
            )]))),
            LValue::Index(_, _) | LValue::Field(_, _) => None,
        },
    }
}

#[allow(clippy::too_many_lines)]
fn walk_init(
    statements: &[Statement],
    mut definitely_assigned: BTreeMap<String, Coverage>,
    mut possibly_assigned: BTreeSet<InitWriteKey>,
    in_forall: bool,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
) -> Result<(BTreeMap<String, Coverage>, BTreeSet<InitWriteKey>), RuntimeError> {
    for statement in statements {
        match statement {
            Statement::Assign { target, value, .. } => {
                let logical = logical_var(target)
                    .ok_or_else(|| runtime_error("invalid init assignment target"))?;
                let contribution = assignment_coverage(target, bound_names, model);
                let keys = init_write_keys(target, contribution.as_ref());
                if keys.iter().any(|key| possibly_assigned.contains(key)) {
                    let scope = if in_forall { "init forall" } else { "init" };
                    return Err(runtime_error(format!(
                        "state variable '{logical}' assigned more than once in {scope}"
                    )));
                }
                if let LValue::Index(_, key_expr) = target {
                    check_init_expr(key_expr, &definitely_assigned, model)?;
                }
                check_init_expr(value, &definitely_assigned, model)?;
                if let Some(contribution) = contribution {
                    let previous = definitely_assigned.remove(logical);
                    definitely_assigned
                        .insert(logical.to_owned(), join_coverage(previous, contribution));
                }
                possibly_assigned.extend(keys);
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                if in_forall {
                    return Err(runtime_error("nested forall in init is not supported"));
                }
                match binder {
                    Binder::Range { lo, hi, .. } => {
                        let mut references = state_references(lo, model);
                        references.extend(state_references(hi, model));
                        if let Some(name) = references.first() {
                            return Err(runtime_error(format!(
                                "init forall range bounds must be compile-time constants; state variable '{name}' is not allowed"
                            )));
                        }
                    }
                    Binder::Collection { collection, .. } => {
                        if let Some(name) = state_references(collection, model).first() {
                            return Err(runtime_error(format!(
                                "init forall over a state collection is not supported; state variable '{name}' is not allowed"
                            )));
                        }
                    }
                    Binder::Typed { .. } => {}
                }
                let where_expr = match binder {
                    Binder::Typed { where_expr, .. }
                    | Binder::Range { where_expr, .. }
                    | Binder::Collection { where_expr, .. } => where_expr,
                };
                if let Some(where_expr) = where_expr {
                    check_init_expr(where_expr, &definitely_assigned, model)?;
                }
                let binder_values = compute_binder_values(binder, model)?;
                let mut nested_bound = bound_names.clone();
                nested_bound.insert(binder_name(binder).to_owned(), binder_values);
                (definitely_assigned, possibly_assigned) = walk_init(
                    statements,
                    definitely_assigned,
                    possibly_assigned,
                    true,
                    &nested_bound,
                    model,
                )?;
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                check_init_expr(condition, &definitely_assigned, model)?;
                let (then_definite, then_possible) = walk_init(
                    then_statements,
                    definitely_assigned.clone(),
                    possibly_assigned.clone(),
                    in_forall,
                    bound_names,
                    model,
                )?;
                let (else_definite, else_possible) = walk_init(
                    else_statements,
                    definitely_assigned,
                    possibly_assigned,
                    in_forall,
                    bound_names,
                    model,
                )?;
                definitely_assigned = meet_branches(then_definite, &else_definite);
                possibly_assigned = then_possible.union(&else_possible).cloned().collect();
            }
        }
    }
    Ok((definitely_assigned, possibly_assigned))
}

fn check_init_expr(
    expr: &Expr,
    definitely_assigned: &BTreeMap<String, Coverage>,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    let references = state_references(expr, model);
    if let Some(name) = references.iter().find(|name| {
        let ty = model.state_type(name.as_str());
        !ty.is_some_and(|ty| coverage_is_full(definitely_assigned.get(name.as_str()), ty, model))
    }) {
        return Err(runtime_error(format!(
            "init references state variable '{name}' before it is assigned"
        )));
    }
    Ok(())
}

fn state_references(expr: &Expr, model: &KernelModel) -> BTreeSet<String> {
    let state_names = model
        .state
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    let mut references = BTreeSet::new();
    collect_state_references(expr, &state_names, &mut references);
    references
}

fn collect_state_references(
    expr: &Expr,
    state_names: &BTreeSet<&str>,
    output: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Var(name) => {
            if state_names.contains(name.as_str()) {
                output.insert(name.clone());
            }
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::EnumMember { .. } => {}
        Expr::Some(value)
        | Expr::Neg(value)
        | Expr::Not(value)
        | Expr::Field(value, _)
        | Expr::Stage { entity: value, .. }
        | Expr::UnaryNamed { expr: value, .. }
        | Expr::Is { expr: value, .. } => {
            collect_state_references(value, state_names, output);
        }
        Expr::Set(items) | Expr::Seq(items) => {
            for item in items {
                collect_state_references(item, state_names, output);
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                collect_state_references(value, state_names, output);
            }
        }
        Expr::Call { args, .. } => {
            for argument in args {
                collect_state_references(argument, state_names, output);
            }
        }
        Expr::Index(base, index)
        | Expr::Binary {
            left: base,
            right: index,
            ..
        }
        | Expr::BinaryNamed {
            left: base,
            right: index,
            ..
        } => {
            collect_state_references(base, state_names, output);
            collect_state_references(index, state_names, output);
        }
        Expr::Method { receiver, args, .. } => {
            collect_state_references(receiver, state_names, output);
            for argument in args {
                collect_state_references(argument, state_names, output);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            collect_state_references(condition, state_names, output);
            collect_state_references(then_expr, state_names, output);
            collect_state_references(else_expr, state_names, output);
        }
        Expr::Quantified { binder, body, .. } => {
            collect_binder_references(binder, state_names, output);
            collect_state_references(body, state_names, output);
        }
        Expr::Aggregate { binder, value, .. } => {
            collect_binder_references(binder, state_names, output);
            if let Some(value) = value {
                collect_state_references(value, state_names, output);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            collect_state_references(first, state_names, output);
            collect_state_references(second, state_names, output);
            collect_state_references(third, state_names, output);
        }
    }
}

fn collect_binder_references(
    binder: &Binder,
    state_names: &BTreeSet<&str>,
    output: &mut BTreeSet<String>,
) {
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(where_expr) = where_expr {
                collect_state_references(where_expr, state_names, output);
            }
        }
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            collect_state_references(lo, state_names, output);
            collect_state_references(hi, state_names, output);
            if let Some(where_expr) = where_expr {
                collect_state_references(where_expr, state_names, output);
            }
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            collect_state_references(collection, state_names, output);
            if let Some(where_expr) = where_expr {
                collect_state_references(where_expr, state_names, output);
            }
        }
    }
}

fn logical_var(target: &LValue) -> Option<&str> {
    match target {
        LValue::Var(name) | LValue::Index(name, _) => Some(name),
        LValue::Field(base, _) => match base.as_ref() {
            LValue::Var(name) | LValue::Index(name, _) => Some(name),
            LValue::Field(_, _) => None,
        },
    }
}

/// Concrete per-key write identities a single init assignment touches, for
/// duplicate-write detection at the granularity `docs/LANGUAGE.md` §12
/// requires: "assign exactly once" is per concrete key, not per variable.
///
/// This reuses `assignment_coverage`'s resolution rather than computing a
/// second, independent classification (issue #821): a `forall i { m[i] =
/// ... }` whose index is the binder itself resolves to `Coverage::Keys` of
/// every binder value, so it collides key-for-key with a later concrete
/// write to any of those same keys, and an enum-member key resolves to the
/// same `Value` a numeric literal denoting it would.
///
/// Falls back to `InitWriteKey::Root` (whole-variable) whenever `coverage`
/// is not `Coverage::Keys` for an `Index` target — including `Full`,
/// `Fields`, and unresolved (`None`, e.g. a dynamic key or a nested lvalue).
/// That fallback never *accepts* something the coarser, pre-#821
/// `Root`-only classification would have rejected: every write this
/// function buckets under `Root` was already bucketed there before. It is
/// not, however, "as strict as touching the entire variable" in the
/// collision sense: `Root(name)` never collides with
/// `ConcreteIndex(name, _)`, so a `None`-coverage target is simply not
/// collision-checked against any concrete key at all. A `forall`-bound
/// index used through a non-`Var` key expression (e.g. `m[i - i]`) is one
/// such target; two iterations of it aliasing each other, or it aliasing a
/// separate `m[K] = ...`, can still go undetected here when the values
/// agree. That gap is pre-existing (identical on `origin/main`) and is not
/// fixed by this change.
fn init_write_keys(target: &LValue, coverage: Option<&Coverage>) -> BTreeSet<InitWriteKey> {
    if let (LValue::Index(name, _), Some(Coverage::Keys(keys))) = (target, coverage) {
        return keys
            .iter()
            .cloned()
            .map(|key| InitWriteKey::ConcreteIndex(name.clone(), key))
            .collect();
    }
    BTreeSet::from([InitWriteKey::Root(
        logical_var(target)
            .expect("kernel lvalue has a logical root")
            .to_owned(),
    )])
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}
