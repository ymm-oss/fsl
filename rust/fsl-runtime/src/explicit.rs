// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Level-synchronous explicit-state verification over concrete monitor states.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    FslValue as Value, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue,
    KernelModel, KernelStatement as Statement, TraceAction, TraceChange, TraceStep, TypeDef,
    TypeRef,
};

use super::{Bindings, Monitor, RuntimeError, State, Violation, eval, runtime_error};

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
    ConcreteIndex(String, String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParentLink {
    parent: State,
    action: TraceAction,
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

    let initial = Monitor::new(model)?;
    let initial_state = initial.state.clone();
    let mut result = ExplicitResult {
        spec: initial.model.name.clone(),
        depth,
        depth_reached: 0,
        states_explored: 1,
        max_frontier_width: 1,
        closure: false,
        budget_exceeded: false,
        violation: None,
        reachables: initial
            .model
            .reachables
            .iter()
            .map(|property| (property.name.clone(), None))
            .collect(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: initial
            .model
            .actions
            .iter()
            .map(|action| (action.name.clone(), false))
            .collect(),
    };
    let mut frontier = BTreeMap::from([(initial_state.clone(), initial)]);
    let mut seen = BTreeSet::from([initial_state.clone()]);
    let mut parents = BTreeMap::<State, ParentLink>::new();

    for level in 0..=depth {
        result.depth_reached = level;
        result.max_frontier_width = result.max_frontier_width.max(frontier.len());

        for monitor in frontier.values() {
            if let Some(violation) = monitor.current_violation_selected(checked_bounds)? {
                result.violation = Some(ExplicitViolation {
                    trace: reconstruct_trace(&initial_state, &monitor.state, &parents),
                    violation,
                });
                return Ok(result);
            }
            record_reachables(monitor, level, &initial_state, &parents, &mut result)?;
        }

        let mut enabled_by_state = BTreeMap::new();
        for (state, monitor) in &frontier {
            let enabled = monitor.enabled()?;
            for instance in &enabled {
                result.action_coverage.insert(instance.action.clone(), true);
            }
            if enabled.is_empty() && result.deadlock_step.is_none() && !terminal_holds(monitor)? {
                result.deadlock_step = Some(level);
                result.deadlock_trace = Some(reconstruct_trace(&initial_state, state, &parents));
            }
            enabled_by_state.insert(state.clone(), enabled);
        }

        if level == depth {
            break;
        }

        let mut next = BTreeMap::new();
        for (state, monitor) in &frontier {
            for instance in &enabled_by_state[state] {
                let mut child = monitor.clone();
                let stepped = child.step_selected(instance, checked_bounds)?;
                if let Some(violation) = stepped.violation {
                    // The Monitor rolls back on violation (`state` is the pre-step
                    // state); the trace must show the attempted post-state.
                    let after = stepped.attempted_state.as_ref().unwrap_or(&stepped.state);
                    let mut trace = reconstruct_trace(&initial_state, state, &parents);
                    trace.push(edge_trace_step(level + 1, state, instance, after));
                    result.depth_reached = level + 1;
                    result.violation = Some(ExplicitViolation { violation, trace });
                    return Ok(result);
                }
                if seen.contains(&child.state) {
                    continue;
                }
                if seen.len() >= max_states {
                    result.states_explored = seen.len();
                    result.budget_exceeded = true;
                    return Ok(result);
                }
                let child_state = child.state.clone();
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
                next.insert(child_state, child);
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
}

fn record_reachables(
    monitor: &Monitor,
    level: usize,
    initial_state: &State,
    parents: &BTreeMap<State, ParentLink>,
    result: &mut ExplicitResult,
) -> Result<(), RuntimeError> {
    for property in &monitor.model.reachables {
        if result.reachables[&property.name].is_some() {
            continue;
        }
        match eval(
            &property.expr,
            &monitor.state,
            &mut Bindings::new(),
            &monitor.model,
            None,
        )? {
            Value::Bool(true) => {
                result.reachables.insert(
                    property.name.clone(),
                    Some(ExplicitReachableWitness {
                        step: level,
                        trace: reconstruct_trace(initial_state, &monitor.state, parents),
                    }),
                );
            }
            Value::Bool(false) => {}
            _ => return Err(runtime_error("reachable expression must be Boolean")),
        }
    }
    Ok(())
}

fn reconstruct_trace(
    initial_state: &State,
    final_state: &State,
    parents: &BTreeMap<State, ParentLink>,
) -> Vec<TraceStep> {
    let mut cursor = final_state.clone();
    let mut reversed = Vec::<(State, TraceAction)>::new();
    while let Some(link) = parents.get(&cursor) {
        reversed.push((cursor, link.action.clone()));
        cursor = link.parent.clone();
    }
    reversed.reverse();
    let mut trace = vec![TraceStep {
        step: 0,
        state: initial_state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut before = initial_state.clone();
    for (index, (state, action)) in reversed.into_iter().enumerate() {
        trace.push(TraceStep {
            step: index + 1,
            changes: state_changes(&before, &state),
            state: state.clone(),
            action: Some(action),
        });
        before = state;
    }
    trace
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

fn state_changes(before: &State, after: &State) -> BTreeMap<String, TraceChange> {
    after
        .iter()
        .filter_map(|(name, value)| {
            let old = &before[name];
            (old != value).then(|| {
                (
                    name.clone(),
                    TraceChange {
                        from: old.clone(),
                        to: value.clone(),
                    },
                )
            })
        })
        .collect()
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

fn check_deterministic_init(model: &KernelModel) -> Result<(), RuntimeError> {
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
        Binder::Range { lo, hi, .. } => {
            match (const_int_bound(lo, model), const_int_bound(hi, model)) {
                (Some(lo), Some(hi)) => Ok(Some((lo..=hi).map(Value::Int).collect())),
                _ => Ok(None),
            }
        }
        Binder::Typed { .. } | Binder::Collection { .. } => Ok(None),
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
                let key = init_write_key(target, bound_names);
                if possibly_assigned.contains(&key) {
                    let scope = if in_forall { "init forall" } else { "init" };
                    return Err(runtime_error(format!(
                        "state variable '{logical}' assigned more than once in {scope}"
                    )));
                }
                if let LValue::Index(_, key_expr) = target {
                    check_init_expr(key_expr, &definitely_assigned, model)?;
                }
                check_init_expr(value, &definitely_assigned, model)?;
                if let Some(contribution) = assignment_coverage(target, bound_names, model) {
                    let previous = definitely_assigned.remove(logical);
                    definitely_assigned
                        .insert(logical.to_owned(), join_coverage(previous, contribution));
                }
                possibly_assigned.insert(key);
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
                if let Binder::Typed { where_expr, .. } | Binder::Collection { where_expr, .. } =
                    binder
                    && let Some(where_expr) = where_expr
                {
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
        Expr::Num(_) | Expr::Bool(_) | Expr::None => {}
        Expr::Some(value)
        | Expr::Neg(value)
        | Expr::Not(value)
        | Expr::Field(value, _)
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
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_state_references(condition, state_names, output);
            collect_state_references(then_expr, state_names, output);
            collect_state_references(else_expr, state_names, output);
        }
        Expr::Quantified { binder, body, .. } => {
            collect_binder_references(binder, state_names, output);
            collect_state_references(body, state_names, output);
        }
        Expr::Count { condition, .. } => {
            collect_state_references(condition, state_names, output);
        }
        Expr::Sum {
            body, condition, ..
        } => {
            collect_state_references(body, state_names, output);
            if let Some(condition) = condition {
                collect_state_references(condition, state_names, output);
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
        Expr::BinderNamed { binder, .. } => {
            collect_binder_references(binder, state_names, output);
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
        Binder::Range { lo, hi, .. } => {
            collect_state_references(lo, state_names, output);
            collect_state_references(hi, state_names, output);
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

fn init_write_key(
    target: &LValue,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
) -> InitWriteKey {
    if let LValue::Index(name, index) = target {
        match index {
            Expr::Var(key) if !bound_names.contains_key(key) => {
                return InitWriteKey::ConcreteIndex(name.clone(), format!("var:{key}"));
            }
            Expr::Num(key) => {
                return InitWriteKey::ConcreteIndex(name.clone(), format!("num:{key}"));
            }
            _ => {}
        }
    }
    InitWriteKey::Root(
        logical_var(target)
            .expect("kernel lvalue has a logical root")
            .to_owned(),
    )
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}
