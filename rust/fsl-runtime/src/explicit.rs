// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Level-synchronous explicit-state verification over concrete monitor states.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    FslValue as Value, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue,
    KernelModel, KernelStatement as Statement, TraceAction, TraceStep, TypeDef, TypeRef,
    state_references,
};

use super::trace::{Admission, StateStore, state_changes};
use super::{
    Bindings, Monitor, RuntimeError, State, Violation, check_state_selected, eval, runtime_error,
    runtime_error_at, terminal_holds_at, with_total_division,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ExplicitViolation {
    pub violation: Violation,
    pub trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ExplicitReachableWitness {
    pub step: usize,
    pub trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ActionStats {
    /// Number of unique explored states in which at least one instance of the
    /// action was enabled.
    pub enabled: usize,
    /// Number of unique explored `(state, action instance)` transition edges
    /// that completed without a verification violation.
    pub fired: usize,
    /// Number of fired edges whose successor state is identical to the source.
    pub no_op: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
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
    pub action_profile: BTreeMap<String, ActionStats>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum InitWriteKey {
    Root(String),
    ConcreteIndex(String, Value),
    ConcreteIndexField(String, Value, String),
    RootField(String, String),
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
    explicit_unsupported_error(model).map(|error| error.message)
}

fn explicit_unsupported_error(model: &KernelModel) -> Option<RuntimeError> {
    if let Err(error) = check_deterministic_init(model) {
        return Some(error);
    }
    if !model.leadstos.is_empty() {
        return Some(runtime_error(
            "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property"
                .to_owned(),
        ));
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
///
/// # Panics
///
/// Panics if an action the Monitor reported as enabled, or one it just stepped,
/// is absent from `model.actions`. Both names come from the same model this
/// function was handed, so either would mean the Monitor and the model had
/// disagreed about which actions exist — a state the profile counters cannot be
/// attributed under, and one no input can reach through the public API.
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
    if let Some(error) = explicit_unsupported_error(&model) {
        return Err(error);
    }

    // The frontier carries `State` only, not `Monitor` -- a full `Monitor`
    // clone per child, on top of a `BTreeMap<State, Monitor>` frontier
    // holding one whole `KernelModel` per live state, duplicated the model
    // at both layers (issue #730). `scratch` only lends its model: every
    // state is read and stepped where the frontier holds it.
    //
    // The visited set and the parent links are one packed `StateStore`
    // (issue #1851, as #1247 did for the refinement searches): each reached
    // state is held once, as bytes, and a frontier entry carries its id.
    // A frontier is kept in ascending `State` order, the order a
    // `BTreeSet<State>` frontier was visited in, because that order picks
    // the reported violation, deadlock and witness traces.
    let mut scratch = Monitor::new(model)?;
    let initial_state = std::mem::take(&mut scratch.state);
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
        action_profile: scratch
            .model
            .actions
            .iter()
            .map(|action| {
                (
                    action.name.clone(),
                    ActionStats {
                        enabled: 0,
                        fired: 0,
                        no_op: 0,
                    },
                )
            })
            .collect(),
    };
    let (mut seen, root) = StateStore::with_root(&initial_state);
    let mut frontier = vec![(initial_state, root)];
    // Every stored state but the root passed the state bounds and invariants
    // of the step that stored it, under the same selection; unless an
    // invariant reads `old()`, they would pass again (`Monitor::execute_from`).
    let admits_stored = !super::invariants_read_old(&scratch.model);

    for level in 0..=depth {
        result.depth_reached = level;
        result.max_frontier_width = result.max_frontier_width.max(frontier.len());

        // The reachables still unwitnessed when the level starts. BMC checks
        // each of them for a partial operation over every state of the step
        // before it records the step's witnesses (`check_state_properties_in`,
        // then `record_reachables`), so a witness on one state of this level
        // does not stop the others from being checked.
        let pending = scratch
            .model
            .reachables
            .iter()
            .map(|property| result.reachables[&property.name].is_none())
            .collect::<Vec<_>>();
        for (state, id) in &frontier {
            if (*id == root || !admits_stored)
                && let Some(violation) =
                    check_state_selected(state, None, &scratch.model, level, checked_bounds)?
            {
                result.violation = Some(ExplicitViolation {
                    trace: seen.trace(*id),
                    violation,
                });
                return Ok(result);
            }
            if let Some(violation) = record_reachables(
                &scratch.model,
                &pending,
                (state, *id),
                level,
                &seen,
                &mut result,
            )? {
                result.violation = Some(ExplicitViolation {
                    trace: seen.trace(*id),
                    violation,
                });
                return Ok(result);
            }
        }

        // BMC's order at one step k (`bmc.rs`, `verify_bounded_config`, #1961):
        // the state properties of k, then the action partial operations of
        // the instances leaving k (guards, bodies, `ensures`; only when
        // k < depth), then, while no deadlock is recorded, the `deadlock`
        // query: a dead state whose terminal reaches a partial operation, then
        // one whose terminal is otherwise undefined (an error), then a dead
        // non-terminal state, which is recorded as the deadlock. The other
        // checks of a step from k (bounds, invariants, `trans`, `ensures`)
        // belong to the state properties of k + 1, after all of that.
        //
        // So at this level the terminal is evaluated on every dead state until
        // the first one whose evaluation fails, and that failure is held back
        // until the stepping below has found no action partial operation.
        // Below the depth a state is dead when it has no candidate; at the
        // depth, where BMC checks no action partial operation, a state whose
        // only candidates are guards reaching a partial operation is dead when
        // BMC's reading of those guards can disable them all
        // (`every_partial_guard_may_be_disabled`).
        // Once a deadlock is recorded neither engine evaluates the terminal at
        // a later level.
        let check_terminals = result.deadlock_step.is_none();
        let mut level_deadlock = None;
        let mut terminal_failure = None;
        let mut enabled_by_state = Vec::with_capacity(frontier.len());
        for (state, id) in &frontier {
            // A guard that reaches a partial operation is neither enabled nor
            // disabled: it is stepped below into that action's `partial_op`
            // (#1191).
            let candidates = scratch.candidates_at(state)?;
            let enabled = candidates
                .iter()
                .filter_map(super::ActionCandidate::enabled)
                .collect::<Vec<_>>();
            for instance in &enabled {
                result.action_coverage.insert(instance.action.clone(), true);
            }
            // `enabled` may contain several parameter instances of one action;
            // this counter deliberately measures states, not instances.
            for action in enabled
                .iter()
                .map(|instance| &instance.action)
                .collect::<BTreeSet<_>>()
            {
                result
                    .action_profile
                    .get_mut(action)
                    .expect("enabled action belongs to the model")
                    .enabled += 1;
            }
            // Below the depth a partial-guard candidate is stepped into its
            // action's `partial_op`, which is reported before anything this
            // state's terminal or deadlock could give, unless a state-budget
            // stop comes first; so there only a state without candidates is
            // dead, as before #1961. At the depth BMC's reading decides.
            let dead = if level < depth {
                candidates.is_empty()
            } else {
                enabled.is_empty()
                    && every_partial_guard_may_be_disabled(&scratch.model, state, &candidates)?
            };
            if dead && check_terminals && terminal_failure.is_none() {
                match terminal_holds_at(&scratch.model, state) {
                    Ok(true) => {}
                    Ok(false) => {
                        if level_deadlock.is_none() {
                            level_deadlock = Some(*id);
                        }
                    }
                    Err(error) if super::is_partial_operation_error(&error) => {
                        terminal_failure = Some(TerminalFailure::Partial(*id));
                    }
                    Err(error) => terminal_failure = Some(TerminalFailure::Error(error)),
                }
            }
            enabled_by_state.push(candidates);
        }

        if level == depth {
            if let Some(failure) = terminal_failure {
                return report_terminal_failure(result, failure, &seen, level);
            }
            record_deadlock(&mut result, level_deadlock, &seen, level);
            break;
        }

        // After a held terminal failure or a stepped violation that is not an
        // action partial operation, the stepping only looks for an action
        // partial operation of this level: it expands no successor, and an
        // error or an unchecked Seq overflow on a later instance belongs to a
        // later check than the one already held.
        let mut held_violation = None;
        let mut expanding = terminal_failure.is_none();
        let mut next = Vec::new();
        for ((state, id), candidates) in frontier.iter().zip(&enabled_by_state) {
            for candidate in candidates {
                let instance = candidate.instance();
                let stepped = match scratch.step_enumerated(
                    state,
                    level,
                    candidate,
                    checked_bounds,
                    &mut |next| admits_stored && seen.id_of(next).is_some_and(|id| id != root),
                ) {
                    Ok(stepped) => stepped,
                    Err(_) if !expanding => continue,
                    Err(error) => return Err(error),
                };
                // Match symbolic frames: an omitted Seq bound must not let
                // exploration proceed beyond the frame's representable capacity.
                let after = stepped.attempted_state.as_ref().unwrap_or(&stepped.state);
                for (name, ty) in &scratch.model.state {
                    if expanding
                        && checked_bounds.is_some_and(|selected| {
                            !selected.contains(
                                &fsl_core::SyntheticName::Bounds { state: name }.internal(),
                            )
                        })
                        && let TypeRef::Seq(_, capacity) = ty
                        && let Some(Value::Seq(values)) = after.get(name)
                        && values.len() > *capacity
                    {
                        return Err(runtime_error("model sequence length exceeds capacity"));
                    }
                }
                if let Some(violation) = stepped.violation {
                    // The Monitor rolls back on violation (`state` is the pre-step
                    // state); the trace must show the attempted post-state.
                    let mut trace = seen.trace(*id);
                    trace.push(edge_trace_step(level + 1, state, instance, after));
                    let found = ExplicitViolation { violation, trace };
                    if is_action_partial(&found.violation, &instance.action) {
                        result.depth_reached = level + 1;
                        result.violation = Some(found);
                        return Ok(result);
                    }
                    if expanding {
                        held_violation = Some(found);
                        expanding = false;
                    }
                    continue;
                }
                if !expanding {
                    continue;
                }
                let child_state = stepped.state;
                // A frontier state is unique and `Monitor::enabled` enumerates
                // each action instance once, so this is exactly one unique
                // `(state, action instance)` edge. Record it before child
                // deduplication: distinct edges may converge on one child.
                let stats = result
                    .action_profile
                    .get_mut(&instance.action)
                    .expect("stepped action belongs to the model");
                stats.fired += 1;
                if child_state == *state {
                    stats.no_op += 1;
                }
                match seen.insert_child_within(
                    &child_state,
                    *id,
                    &instance.action,
                    &instance.params,
                    max_states,
                ) {
                    Admission::Stored => {}
                    Admission::Full => {
                        record_deadlock(&mut result, level_deadlock, &seen, level);
                        result.states_explored = seen.len();
                        result.budget_exceeded = true;
                        return Ok(result);
                    }
                    Admission::New(child) => next.push((child_state, child)),
                }
            }
        }
        if let Some(failure) = terminal_failure {
            return report_terminal_failure(result, failure, &seen, level);
        }
        record_deadlock(&mut result, level_deadlock, &seen, level);
        if let Some(found) = held_violation {
            result.depth_reached = level + 1;
            result.violation = Some(found);
            return Ok(result);
        }
        result.states_explored = seen.len();
        if next.is_empty() {
            result.closure = true;
            return Ok(result);
        }
        next.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        frontier = next;
    }

    result.states_explored = seen.len();
    Ok(result)
}

/// Whether BMC's deadlock query can find every instance in `candidates`
/// disabled, for a state at the depth where none is enabled. Each candidate
/// is then a guard that reaches a partial operation.
///
/// BMC's query (`bmc.rs`, the `deadlock` query over `enabled_terms`, whose
/// terms are each instance's guard conjunction from `action_guards_in`) asks
/// whether some valuation of its free terms makes every instance's guard
/// false: `∃ v. ∀ i. ¬guard_i(v)`. For a fixed state the free terms are the
/// values its partial reads produce: `/` and `%` by zero are totalized to zero
/// (DESIGN-divmod.md), so they are not free, and a partial Seq read is an
/// unconstrained value of the solver sort (an unobservable slot,
/// DESIGN-seq-partial-operations.md; for an integer range that sort is the
/// unbounded integers, so the read is not limited to the element type).
///
/// So each guard is read in three values ([`guard_truth`]), with division
/// totalized and every partial Seq read unknown: a guard that is `true` for
/// every value of its reads keeps the state alive, as it does in BMC
/// (`∀ v. guard_i(v)` implies `∀ v. ∃ i. guard_i(v)`). Every other state
/// counts as dead. That is exact when the reads can disable every guard at
/// once, and otherwise reports a dead state that BMC does not: a guard that
/// is true for every value but not shown to be so by the three-valued
/// reading, or instances that are each falsifiable but never all false
/// together (DESIGN-explicit-engine.md §4).
fn every_partial_guard_may_be_disabled(
    model: &KernelModel,
    state: &State,
    candidates: &[super::ActionCandidate],
) -> Result<bool, RuntimeError> {
    for candidate in candidates {
        let instance = candidate.instance();
        let action = model
            .actions
            .iter()
            .find(|action| action.name == instance.action)
            .expect("candidate action belongs to the model");
        let truth =
            with_total_division(|| instance_guard_truth(action, &instance.params, state, model))?;
        if truth == Truth::True {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A guard's value for every value of its partial Seq reads: always true,
/// always false, or not decided by the three-valued reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }

    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        self.not().and(other.not()).not()
    }
}

/// An instance's guards, in order, as BMC's conjunction reads them: a `let`
/// whose value is a partial read, or a `requires` that is not `True`, ends the
/// reading (a later guard can read names only an undecided guard binds).
fn instance_guard_truth(
    action: &fsl_core::ActionDef,
    params: &Bindings,
    state: &State,
    model: &KernelModel,
) -> Result<Truth, RuntimeError> {
    let mut bindings = params.clone();
    for guard in &action.guards {
        match guard {
            fsl_core::ActionGuard::Let(name, expression) => {
                match eval(expression, state, &mut bindings, model, None) {
                    Ok(value) => {
                        bindings.insert(name.clone(), value);
                    }
                    Err(error) if super::is_partial_operation_error(&error) => {
                        return Ok(Truth::Unknown);
                    }
                    Err(error) => return Err(error),
                }
            }
            fsl_core::ActionGuard::Requires(expression) => {
                match guard_truth(expression, state, &mut bindings, model)? {
                    Truth::True => {}
                    // An undecided `requires` may not have bound the pattern
                    // names a later guard reads, so the instance stays
                    // undecided there; only `True` keeps a state alive.
                    undecided => return Ok(undecided),
                }
            }
        }
    }
    Ok(Truth::True)
}

/// One `requires` expression read in three values. The connectives, `not`
/// and conditionals combine their operands' values (Kleene's strong logic),
/// so an operand that does not decide the result may be unknown; a
/// comparison of an expression with itself is decided when both sides read
/// the same slots, as BMC's terms then are equal ([`reads_the_same_slots`]);
/// any other expression is evaluated, and a partial operation makes it
/// unknown. A connective that may bind a pattern name is evaluated whole.
fn guard_truth(
    expr: &Expr,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<Truth, RuntimeError> {
    match expr {
        Expr::Binary { op, left, right }
            if super::is_connective(op) && !super::connective_may_bind(expr) =>
        {
            let left = guard_truth(left, state, bindings, model)?;
            match (op.as_str(), left) {
                ("and", Truth::False) => return Ok(Truth::False),
                ("or", Truth::True) | ("=>", Truth::False) => return Ok(Truth::True),
                _ => {}
            }
            let right = guard_truth(right, state, bindings, model)?;
            Ok(match op.as_str() {
                "and" => left.and(right),
                "or" => left.or(right),
                _ => left.not().or(right),
            })
        }
        Expr::Not(inner) => Ok(guard_truth(inner, state, bindings, model)?.not()),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => match guard_truth(condition, state, bindings, model)? {
            Truth::True => guard_truth(then_expr, state, bindings, model),
            Truth::False => guard_truth(else_expr, state, bindings, model),
            Truth::Unknown => {
                let then_truth = guard_truth(then_expr, state, bindings, model)?;
                let else_truth = guard_truth(else_expr, state, bindings, model)?;
                Ok(if then_truth == else_truth {
                    then_truth
                } else {
                    Truth::Unknown
                })
            }
        },
        Expr::Binary { op, left, right }
            if matches!(op.as_str(), "==" | "!=" | "<=" | ">=")
                && left == right
                && reads_the_same_slots(left, bindings, model) =>
        {
            match eval(left, state, bindings, model, None) {
                Err(error) if super::is_partial_operation_error(&error) => {
                    Ok(Truth::from_bool(op != "!="))
                }
                _ => guard_leaf(expr, state, bindings, model),
            }
        }
        _ => guard_leaf(expr, state, bindings, model),
    }
}

fn guard_leaf(
    expr: &Expr,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<Truth, RuntimeError> {
    match super::eval_with_bindings(expr, state, bindings, model, None) {
        Ok(value) => super::as_bool(value).map(Truth::from_bool),
        Err(error) if super::is_partial_operation_error(&error) => Ok(Truth::Unknown),
        Err(error) => Err(error),
    }
}

/// Whether two evaluations of `expr` read the same symbolic slots in BMC, so
/// that `expr == expr` holds there whatever the slots hold. That holds for
/// reads of state Seqs with a positive capacity, whose slots are state
/// terms; a capacity-zero Seq and a Seq literal have no slot to read and get
/// a fresh unconstrained term for each read (DESIGN-seq-partial-operations.md,
/// "Capacity-zero Seqs"). Only a narrow syntax is accepted; anything else
/// answers `false`, and the comparison is then evaluated like any other.
fn reads_the_same_slots(expr: &Expr, bindings: &Bindings, model: &KernelModel) -> bool {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::EnumMember { .. } => true,
        Expr::Var(name) => match bindings.get(name) {
            Some(value) => !matches!(value, Value::Seq(_)),
            None => model.state.iter().any(|(state, ty)| {
                state == name
                    && (matches!(ty, TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _))
                        || matches!(ty, TypeRef::Seq(_, capacity) if *capacity > 0))
            }),
        },
        Expr::Some(inner) | Expr::Neg(inner) | Expr::Not(inner) | Expr::Field(inner, _) => {
            reads_the_same_slots(inner, bindings, model)
        }
        Expr::Index(left, right) | Expr::Binary { left, right, .. } => {
            reads_the_same_slots(left, bindings, model)
                && reads_the_same_slots(right, bindings, model)
        }
        Expr::Method { receiver, args, .. } => {
            reads_the_same_slots(receiver, bindings, model)
                && args
                    .iter()
                    .all(|arg| reads_the_same_slots(arg, bindings, model))
        }
        _ => false,
    }
}

/// The first failed terminal evaluation of a level, held back until the
/// level's stepping has found no action partial operation.
enum TerminalFailure {
    /// The terminal reached a partial operation on the dead state stored as this id.
    Partial(usize),
    /// The terminal could not be evaluated for another reason.
    Error(RuntimeError),
}

fn report_terminal_failure(
    mut result: ExplicitResult,
    failure: TerminalFailure,
    seen: &StateStore,
    level: usize,
) -> Result<ExplicitResult, RuntimeError> {
    match failure {
        TerminalFailure::Partial(id) => {
            result.depth_reached = level;
            result.violation = Some(ExplicitViolation {
                trace: seen.trace(id),
                violation: Violation {
                    kind: "partial_op".to_owned(),
                    name: fsl_core::SyntheticName::PartialTerminal.internal(),
                    step: level,
                },
            });
            Ok(result)
        }
        TerminalFailure::Error(error) => Err(error),
    }
}

/// Record the level's first dead non-terminal state as the deadlock.
fn record_deadlock(
    result: &mut ExplicitResult,
    level_deadlock: Option<usize>,
    seen: &StateStore,
    level: usize,
) {
    if let Some(id) = level_deadlock {
        result.deadlock_step = Some(level);
        result.deadlock_trace = Some(seen.trace(id));
    }
}

/// Whether a stepped violation is the stepped action's own partial operation
/// (guard, body or `ensures`), which BMC checks before the level's terminal.
fn is_action_partial(violation: &Violation, action: &str) -> bool {
    violation.kind == "partial_op"
        && violation.name == fsl_core::SyntheticName::PartialAction { action }.internal()
}

/// Evaluate each reachable in `pending` (parallel to `model.reachables`) on
/// `state`: a partial operation is that reachable's `partial_op`, and the
/// first state of the level on which it holds is its witness.
fn record_reachables(
    model: &KernelModel,
    pending: &[bool],
    (state, id): (&State, usize),
    level: usize,
    seen: &StateStore,
    result: &mut ExplicitResult,
) -> Result<Option<Violation>, RuntimeError> {
    with_total_division(|| {
        for (property, pending) in model.reachables.iter().zip(pending) {
            if !pending {
                continue;
            }
            let value = match eval(&property.expr, state, &mut Bindings::new(), model, None) {
                Ok(value) => value,
                Err(error) if super::is_partial_operation_error(&error) => {
                    return Ok(Some(Violation {
                        kind: "partial_op".to_owned(),
                        name: fsl_core::SyntheticName::PartialProperty {
                            property: &property.name,
                        }
                        .internal(),
                        step: level,
                    }));
                }
                Err(error) => return Err(error),
            };
            match value {
                // A reachable already witnessed earlier in this level keeps
                // that witness: the first state in frontier order.
                Value::Bool(true) if result.reachables[&property.name].is_none() => {
                    result.reachables.insert(
                        property.name.clone(),
                        Some(ExplicitReachableWitness {
                            step: level,
                            trace: seen.trace(id),
                        }),
                    );
                }
                Value::Bool(_) => {}
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
/// key-type domain has been fully assigned, including every struct field in
/// each entry; a `Named` struct root is fully assigned
/// only once every declared field has been (recursively) fully assigned.
/// Anything else stays binary (`Full` or unassigned) because there is no
/// finer component structure to track.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Coverage {
    Full,
    Keys(BTreeMap<Value, Coverage>),
    Fields(BTreeMap<String, Coverage>),
}

#[derive(Clone, Copy)]
enum InitWalkMode<'a> {
    Concrete,
    LegacyConcrete,
    ScopedFields(&'a BTreeSet<String>),
    ReadOrder(&'a BTreeSet<String>, bool), // Resolve static domains only for refinement.
}

/// Names and declared types of state roots that `model`'s init never
/// assigns on *any* path — fully free / unconstrained by init, per the same
/// symbolic-free-variable semantics `fsl-verifier`'s BMC init lowering gives
/// an omitted assignment (DESIGN-init-if.md; issue #493).
///
/// This is deliberately simpler than — and not layered on — the
/// [`check_deterministic_init`]/[`walk_init`] coverage gate: that walk
/// rejects a condition/index expression reading a not-yet-assigned root,
/// distinguishing unsupported free values from assigned-root early reads.
/// A free condition is exactly the pattern a nondeterministic `init if`
/// legitimately uses (the motivating case for #493). This scan instead only
/// asks whether a root is ever an assignment *target* anywhere in init,
/// regardless of read order or which branch is taken; it never errors and
/// never rejects a read.
///
/// Partially assigned roots are not free; refinement rejects them before enumeration (#1521).
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
    if let Err(error) = check_init_write_ownership(model) {
        // Main's concrete walk may encounter an earlier read/domain failure
        // before a legacy ownership collision. Preserve that error on already
        // refused inputs, without replacing the new read gate's diagnostics.
        if error.message.starts_with("state variable '")
            && let Err(earlier) = walk_init(
                &model.init,
                BTreeMap::new(),
                BTreeSet::new(),
                false,
                &BTreeMap::new(),
                model,
                InitWalkMode::LegacyConcrete,
            )
            && !earlier.message.starts_with("state variable '")
        {
            return Err(earlier);
        }
        return Err(error);
    }
    check_init_coverage(model, None)
}

fn check_init_coverage(
    model: &KernelModel,
    roots: Option<&BTreeSet<String>>,
) -> Result<(), RuntimeError> {
    let (assigned, _) = walk_init(
        &model.init,
        BTreeMap::new(),
        BTreeSet::new(),
        false,
        &BTreeMap::new(),
        model,
        roots.map_or(InitWalkMode::Concrete, |roots| {
            InitWalkMode::ReadOrder(roots, true)
        }),
    )?;
    let mut missing_is_partial = false;
    let mut missing = model
        .state
        .iter()
        .filter(|(name, _)| roots.is_none_or(|roots| roots.contains(name)))
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

pub(crate) fn check_refinement_init(model: &KernelModel) -> Result<(), RuntimeError> {
    let mut roots = BTreeSet::new();
    collect_assigned_init_roots(&model.init, &mut roots);
    check_init_coverage(model, Some(&roots)).map_err(|error| {
        runtime_error(format!(
            "{} bmc and induction return verdicts using free values for this init, but refine refuses it.",
            error.message
        ))
    })
}

/// Whether `coverage` amounts to a complete definite assignment of `ty`.
fn coverage_is_full(coverage: Option<&Coverage>, ty: &TypeRef, model: &KernelModel) -> bool {
    match coverage {
        Some(Coverage::Full) => true,
        Some(Coverage::Keys(keys)) => match ty {
            TypeRef::Map(key_ty, value_ty) => model.map_key_values(key_ty).is_ok_and(|domain| {
                domain
                    .iter()
                    .all(|value| coverage_is_full(keys.get(value), value_ty, model))
            }),
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
/// anything; same-kind coverage merges per-key (`Keys`) or per-field
/// (`Fields`); a kind mismatch keeps whatever was already recorded rather
/// than erroring (component tracking degrades to "no worse than before").
fn join_coverage(existing: Option<Coverage>, addition: Coverage) -> Coverage {
    let Some(existing) = existing else {
        return addition;
    };
    match (existing, addition) {
        (Coverage::Full, _) | (_, Coverage::Full) => Coverage::Full,
        (Coverage::Keys(mut a), Coverage::Keys(b)) => {
            for (key, addition) in b {
                let merged = join_coverage(a.remove(&key), addition);
                a.insert(key, merged);
            }
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
/// other side; same-kind coverage meets per-key (`Keys`) or meets per-field
/// (`Fields`, dropping fields missing on either side); a kind mismatch drops
/// the root entirely, matching the old set-intersection behavior for roots
/// only assigned on one side.
fn meet_coverage(a: &Coverage, b: &Coverage) -> Option<Coverage> {
    match (a, b) {
        (Coverage::Full, other) | (other, Coverage::Full) => Some(other.clone()),
        (Coverage::Keys(a), Coverage::Keys(b)) => {
            let mut merged = BTreeMap::new();
            for (key, coverage_a) in a {
                if let Some(coverage_b) = b.get(key)
                    && let Some(meet) = meet_coverage(coverage_a, coverage_b)
                {
                    merged.insert(key.clone(), meet);
                }
            }
            Some(Coverage::Keys(merged))
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

/// Finite binder values used for binder-key coverage and write ownership,
/// without evaluating against a concrete state. Filtered or collection binders
/// and non-constant range bounds return `None`, so assignments at the binder key
/// cannot prove coverage through this function. Fixed targets are separate:
/// `init_binder_is_nonempty` can prove a static filtered domain nonempty
/// (including a singleton); scoped static supplementation/nonemptiness leaves legacy range resolution unchanged.
fn compute_binder_values(
    binder: &Binder,
    model: &KernelModel,
    outer: Option<&BTreeMap<String, Option<Vec<Value>>>>,
) -> Result<Option<Vec<Value>>, RuntimeError> {
    match binder {
        Binder::Typed {
            type_name,
            where_expr: None,
            ..
        } => Ok(Some(
            model.typed_binder_domain(super::qualified_type(type_name)?)?,
        )),
        Binder::Range {
            lo,
            hi,
            where_expr: None,
            ..
        } => match (
            const_int_bound(lo, model, outer),
            const_int_bound(hi, model, outer),
        ) {
            (Some(lo), Some(hi)) => Ok(Some((lo..=hi).map(Value::Int).collect())),
            _ => Ok(None),
        },
        Binder::Typed { .. } | Binder::Range { .. } | Binder::Collection { .. } => Ok(None),
    }
}

/// Fixed targets only gain coverage if at least one iteration is guaranteed.
/// This does not change ownership or the conservative binder-key coverage of
/// `compute_binder_values`: a state-dependent filter can still skip every write.
fn init_binder_is_nonempty(
    binder: &Binder,
    model: &KernelModel,
    outer: Option<&BTreeMap<String, Option<Vec<Value>>>>,
) -> bool {
    init_binder_static_values(binder, model, outer).is_some_and(|values| !values.is_empty())
}

/// Select only filter `Ok(true)` values; filter errors are not selected.
/// Source errors, state reads and dependencies on supplied outer names return `None`.
fn init_binder_static_values(
    binder: &Binder,
    model: &KernelModel,
    outer: Option<&BTreeMap<String, Option<Vec<Value>>>>,
) -> Option<Vec<Value>> {
    if outer.is_some_and(|names| init_binder_depends_on_outer(binder, names)) {
        return None;
    }
    let (sources, filter) = init_binder_exprs(binder);
    if sources
        .iter()
        .any(|expr| !state_references(expr, model, []).is_empty())
        || filter
            .is_some_and(|expr| !state_references(expr, model, [binder_name(binder)]).is_empty())
    {
        return None;
    }
    let state = State::new();
    let values = super::binder_values(binder, &state, &Bindings::new(), model, None).ok()?;
    let mut selected = Vec::new();
    for (value, mut bindings) in values {
        bindings.insert(binder_name(binder).to_owned(), value.clone());
        if super::binder_where_holds(binder, &state, &mut bindings, model, None) == Ok(true) {
            selected.push(value);
        }
    }
    Some(selected)
}

/// Binder sources are evaluated outside its scope; only the filter is inside.
fn init_binder_exprs(binder: &Binder) -> (Vec<&Expr>, Option<&Expr>) {
    match binder {
        Binder::Typed { where_expr, .. } => (vec![], where_expr.as_deref()),
        Binder::Range {
            lo, hi, where_expr, ..
        } => (vec![lo, hi], where_expr.as_deref()),
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => (vec![collection], where_expr.as_deref()),
    }
}

fn init_binder_depends_on_outer(
    binder: &Binder,
    outer: &BTreeMap<String, Option<Vec<Value>>>,
) -> bool {
    if outer.is_empty() {
        return false;
    }
    let (sources, filter) = init_binder_exprs(binder);
    let mut local = BTreeSet::new();
    if sources
        .iter()
        .any(|expr| init_expr_depends_on_outer(expr, outer, &local))
    {
        return true;
    }
    local.insert(binder_name(binder).to_owned());
    filter.is_some_and(|expr| init_expr_depends_on_outer(expr, outer, &local))
}

fn init_expr_depends_on_outer(
    expr: &Expr,
    outer: &BTreeMap<String, Option<Vec<Value>>>,
    local: &BTreeSet<String>,
) -> bool {
    fsl_core::recursion::guard(|| {
        if let Expr::Var(name) = expr {
            return outer.contains_key(name) && !local.contains(name);
        }
        let scoped = match expr {
            Expr::Quantified { binder, body, .. } => Some((binder, Some(body.as_ref()))),
            Expr::Aggregate { binder, value, .. } => Some((binder, value.as_deref())),
            _ => None,
        };
        if let Some((binder, body)) = scoped {
            let (sources, filter) = init_binder_exprs(binder);
            if sources
                .iter()
                .any(|expr| init_expr_depends_on_outer(expr, outer, local))
            {
                return true;
            }
            let mut nested = local.clone();
            nested.insert(binder_name(binder).to_owned());
            return filter
                .into_iter()
                .chain(body)
                .any(|expr| init_expr_depends_on_outer(expr, outer, &nested));
        }
        init_expr_children(expr)
            .iter()
            .any(|child| init_expr_depends_on_outer(child, outer, local))
    })
}

fn const_int_bound(
    expr: &Expr,
    model: &KernelModel,
    outer: Option<&BTreeMap<String, Option<Vec<Value>>>>,
) -> Option<i64> {
    match expr {
        Expr::Num(value) => Some(*value),
        // Nonempty rollback is the decisive coverage refusal for an outer-
        // dependent range. Also leave its keys unresolved here; this guard
        // alone does not change admission while that rollback is in place.
        Expr::Var(name) if outer.is_some_and(|names| names.contains_key(name)) => None,
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

/// Resolve `name` as an integer constant only for bounded integer key types.
///
/// Map-key coverage is typed: an integer constant must not stand in for an enum
/// member merely because they share a spelling.
fn resolve_integer_const_key(model: &KernelModel, key_type: &TypeRef, name: &str) -> Option<Value> {
    let is_integer_key = match key_type {
        TypeRef::Range(_, _) => true,
        TypeRef::Named(type_name) => {
            matches!(model.types.get(type_name), Some(TypeDef::Domain { .. }))
        }
        // Source-level `Map<Int, _>` is rejected by `build_model`; fail closed
        // if an internal caller ever bypasses that construction path.
        TypeRef::Int
        | TypeRef::Bool
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
/// (an unresolved dynamic map key, or an unsupported nested field lvalue).
fn assignment_coverage(
    target: &LValue,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
) -> Option<Coverage> {
    match target {
        LValue::Var(_) => Some(Coverage::Full),
        LValue::Index(name, key_expr) => match key_expr {
            Expr::Num(value) => Some(Coverage::Keys(BTreeMap::from([(
                Value::Int(*value),
                Coverage::Full,
            )]))),
            Expr::Var(key) => {
                if let Some(binder_values) = bound_names.get(key) {
                    binder_values.clone().map(|values| {
                        Coverage::Keys(
                            values
                                .into_iter()
                                .map(|key| (key, Coverage::Full))
                                .collect(),
                        )
                    })
                } else if let Some(TypeRef::Map(key_ty, _)) = model.state_type(name) {
                    resolve_enum_key(model, key_ty, key)
                        .or_else(|| resolve_integer_const_key(model, key_ty, key))
                        .map(|value| Coverage::Keys(BTreeMap::from([(value, Coverage::Full)])))
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
            LValue::Index(_, _) => {
                let Coverage::Keys(keys) = assignment_coverage(base, bound_names, model)? else {
                    return None;
                };
                Some(Coverage::Keys(
                    keys.into_keys()
                        .map(|key| {
                            (
                                key,
                                Coverage::Fields(BTreeMap::from([(field.clone(), Coverage::Full)])),
                            )
                        })
                        .collect(),
                ))
            }
            LValue::Field(_, _) => None,
        },
    }
}

/// Enforce init's assign-once ownership rule without requiring a concrete
/// initial state. Legacy-accepted partial init retains symbolic free values;
/// newly split field writes require complete coverage of every assigned root. Reject
/// two writes that can own the same state location and
/// reads of a root written by the same forall outside its already-written entry.
///
/// # Errors
///
/// Returns [`RuntimeError`] when two writes may overlap on one logical root,
/// a root in newly admitted init is not definitely complete, or an init read
/// violates its coverage or forall iteration-order boundary.
pub fn check_init_write_ownership(model: &KernelModel) -> Result<(), RuntimeError> {
    walk_init_forall_reads(&model.init, &mut [], &[], model)?;
    let field_ownership = walk_init_write_ownership(
        &model.init,
        BTreeSet::new(),
        false,
        &BTreeMap::new(),
        model,
        true,
    );
    // Preserve every init accepted by the pre-#1481 root-granularity gate.
    // Newly admitted per-field forms must completely cover every may-assigned
    // root, including non-field roots on refinement's free-root path.
    let legacy_error = walk_init_write_ownership(
        &model.init,
        BTreeSet::new(),
        false,
        &BTreeMap::new(),
        model,
        false,
    )
    .err();
    if let Err(error) = field_ownership {
        // Keep the first collision's legacy location for already rejected init.
        return Err(legacy_error.unwrap_or(error));
    }
    let mut assigned_roots = BTreeSet::new();
    collect_assigned_init_roots(&model.init, &mut assigned_roots);
    if let Some(legacy_error) = legacy_error {
        let mut field_roots = BTreeSet::new();
        collect_field_init_roots(&model.init, &mut field_roots);
        // Summarize coverage without read refusals or concrete-only restrictions.
        // Partial field-written roots keep the legacy error. When those roots
        // are complete, an early field read wins over partial other roots.
        let Ok((assigned, _)) = walk_init(
            &model.init,
            BTreeMap::new(),
            BTreeSet::new(),
            false,
            &BTreeMap::new(),
            model,
            InitWalkMode::ScopedFields(&BTreeSet::new()),
        ) else {
            // Conservative key resolution may itself encounter an unresolved
            // overlap. Keep the original main refusal, not an analysis error.
            return Err(legacy_error);
        };
        if field_roots.iter().all(|root| {
            model
                .state_type(root)
                .is_some_and(|ty| coverage_is_full(assigned.get(root), ty, model))
        }) {
            walk_init(
                &model.init,
                BTreeMap::new(),
                BTreeSet::new(),
                false,
                &BTreeMap::new(),
                model,
                InitWalkMode::ScopedFields(&field_roots),
            )?;
        }
        // Refinement's free-root path executes init from defaults in program
        // order. A later whole-value assignment must not make an earlier read
        // safe. Keep field gate(a)'s diagnostic priority, but use the saved
        // legacy refusal for reads of other may-assigned roots.
        let other_roots = assigned_roots.difference(&field_roots).cloned().collect();
        if walk_init(
            &model.init,
            BTreeMap::new(),
            BTreeSet::new(),
            false,
            &BTreeMap::new(),
            model,
            InitWalkMode::ScopedFields(&other_roots),
        )
        .is_err()
        {
            return Err(legacy_error);
        }
        if assigned_roots.iter().any(|root| {
            !model
                .state_type(root)
                .is_some_and(|ty| coverage_is_full(assigned.get(root), ty, model))
        }) {
            return Err(legacy_error);
        }
    }
    walk_init(
        &model.init,
        BTreeMap::new(),
        BTreeSet::new(),
        false,
        &BTreeMap::new(),
        model,
        InitWalkMode::ReadOrder(&assigned_roots, false),
    )?;
    Ok(())
}

#[derive(Clone)]
struct InitForallReads {
    binder_depth: usize,
    written_roots: BTreeSet<String>,
    // The key is implicitly this scope's binder; an empty path owns the entry.
    assigned: BTreeSet<(String, Vec<String>)>,
}

struct InitFieldReadCoverage<'a> {
    assigned: &'a BTreeMap<String, Coverage>,
    model: &'a KernelModel,
    roots: &'a BTreeSet<String>,
}

fn collect_field_init_roots(statements: &[Statement], roots: &mut BTreeSet<String>) {
    for statement in statements {
        match statement {
            Statement::Assign { target, .. } => {
                if matches!(target, LValue::Field(_, _))
                    && let Some(root) = logical_var(target)
                {
                    roots.insert(root.to_owned());
                }
            }
            Statement::ForAll { statements, .. } => collect_field_init_roots(statements, roots),
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_field_init_roots(then_statements, roots);
                collect_field_init_roots(else_statements, roots);
            }
        }
    }
}

/// Return a root, optional map key, and field path, without treating a field
/// read as a read of the entire enclosing entry.
fn init_read_place(expr: &Expr) -> Option<(&str, Option<&Expr>, Vec<String>)> {
    match expr {
        Expr::Var(root) => Some((root, None, vec![])),
        Expr::Index(base, key) => match base.as_ref() {
            Expr::Var(root) => Some((root, Some(key), vec![])),
            _ => None,
        },
        Expr::Field(base, field) => {
            let (root, key, mut fields) = init_read_place(base)?;
            fields.push(field.clone());
            Some((root, key, fields))
        }
        _ => None,
    }
}

fn init_target_expr(target: &LValue) -> Expr {
    match target {
        LValue::Var(root) => Expr::Var(root.clone()),
        LValue::Index(root, key) => {
            Expr::Index(Box::new(Expr::Var(root.clone())), Box::new(key.clone()))
        }
        LValue::Field(base, field) => Expr::Field(Box::new(init_target_expr(base)), field.clone()),
    }
}

fn is_current_init_key(key: Option<&Expr>, depth: usize, locals: &[String]) -> bool {
    matches!(key, Some(Expr::Var(name))
        if locals.iter().rposition(|local| local == name) == Some(depth))
}

/// Shared by the check/symbolic ownership gate and the Monitor/explicit gate.
/// Locations are enclosing statement spans, as for duplicate-write diagnostics.
fn walk_init_forall_reads(
    statements: &[Statement],
    scopes: &mut [InitForallReads],
    locals: &[String],
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    for statement in statements {
        match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => {
                let target_expr = init_target_expr(target);
                let (root, key, fields) = init_read_place(&target_expr)
                    .ok_or_else(|| runtime_error("invalid init assignment target"))?;
                if let Some(key) = key {
                    check_init_forall_read_expr(key, scopes, locals, *span)?;
                }
                check_init_forall_read_expr(value, scopes, locals, *span)?;
                for scope in &mut *scopes {
                    if is_current_init_key(key, scope.binder_depth, locals) {
                        scope.assigned.insert((root.to_owned(), fields.clone()));
                    }
                }
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => {
                check_init_forall_read_expr(condition, scopes, locals, *span)?;
                let mut then_scopes = scopes.to_vec();
                walk_init_forall_reads(then_statements, &mut then_scopes, locals, model)?;
                let mut else_scopes = scopes.to_vec();
                walk_init_forall_reads(else_statements, &mut else_scopes, locals, model)?;
                for ((scope, then_scope), else_scope) in
                    scopes.iter_mut().zip(then_scopes).zip(else_scopes)
                {
                    // Whole-entry ownership also covers a branch's field write.
                    scope.assigned = then_scope
                        .assigned
                        .union(&else_scope.assigned)
                        .filter(|place| {
                            init_place_was_assigned(place, &then_scope.assigned)
                                && init_place_was_assigned(place, &else_scope.assigned)
                        })
                        .cloned()
                        .collect();
                }
            }
            Statement::ForAll {
                binder,
                statements,
                span,
            } => {
                let (sources, filter) = init_binder_exprs(binder);
                for source in sources {
                    check_init_forall_read_expr(source, scopes, locals, *span)?;
                }
                let mut nested_scopes = scopes.to_vec();
                let mut written_roots = BTreeSet::new();
                collect_assigned_init_roots(statements, &mut written_roots);
                written_roots.retain(|root| model.state_type(root).is_some());
                nested_scopes.push(InitForallReads {
                    binder_depth: locals.len(),
                    written_roots,
                    assigned: BTreeSet::new(),
                });
                let mut nested_locals = locals.to_vec();
                nested_locals.push(binder_name(binder).to_owned());
                if let Some(filter) = filter {
                    check_init_forall_read_expr(filter, &nested_scopes, &nested_locals, *span)?;
                }
                walk_init_forall_reads(statements, &mut nested_scopes, &nested_locals, model)?;
                // A nested forall may execute zero times: do not promote its
                // writes to the enclosing iteration's definite write set.
            }
        }
    }
    Ok(())
}

fn init_place_was_assigned(
    (root, fields): &(String, Vec<String>),
    assigned: &BTreeSet<(String, Vec<String>)>,
) -> bool {
    assigned.iter().any(|(written_root, written_fields)| {
        written_root == root && fields.starts_with(written_fields)
    })
}

fn check_init_forall_read_expr(
    expr: &Expr,
    scopes: &[InitForallReads],
    locals: &[String],
    span: fsl_core::Span,
) -> Result<(), RuntimeError> {
    check_init_read_expr(expr, scopes, locals, span, None)
}

fn check_init_read_expr(
    expr: &Expr,
    scopes: &[InitForallReads],
    locals: &[String],
    span: fsl_core::Span,
    field_coverage: Option<&InitFieldReadCoverage<'_>>,
) -> Result<(), RuntimeError> {
    fsl_core::recursion::guard(|| {
        check_init_forall_read_expr_inner(expr, scopes, locals, span, field_coverage)
    })
}

#[allow(clippy::too_many_lines)]
fn check_init_forall_read_expr_inner(
    expr: &Expr,
    scopes: &[InitForallReads],
    locals: &[String],
    span: fsl_core::Span,
    field_coverage: Option<&InitFieldReadCoverage<'_>>,
) -> Result<(), RuntimeError> {
    if let Some((root, key, fields)) = init_read_place(expr) {
        if !locals.iter().any(|local| local == root) {
            if let Some(coverage) = field_coverage
                && coverage.roots.contains(root)
                && let Some(ty) = coverage.model.state_type(root)
                && !coverage_is_full(coverage.assigned.get(root), ty, coverage.model)
            {
                return Err(runtime_error_at(
                    format!(
                        "init cannot read '{root}.{}' before state variable '{root}' is fully assigned",
                        if fields.is_empty() {
                            if key.is_some() {
                                "<whole entry>".to_owned()
                            } else {
                                "<whole root>".to_owned()
                            }
                        } else {
                            fields.join(".")
                        }
                    ),
                    span,
                ));
            }
            for scope in scopes {
                if scope.written_roots.contains(root)
                    && !(is_current_init_key(key, scope.binder_depth, locals)
                        && init_place_was_assigned(
                            &(root.to_owned(), fields.clone()),
                            &scope.assigned,
                        ))
                {
                    return Err(runtime_error_at(
                        format!(
                            "init forall cannot read state variable '{root}' except at an already assigned current binder key"
                        ),
                        span,
                    ));
                }
            }
        }
        if let Some(key) = key {
            check_init_read_expr(key, scopes, locals, span, field_coverage)?;
        }
        return Ok(());
    }
    match expr {
        Expr::Quantified { binder, body, .. } => {
            return check_init_forall_read_binder(
                binder,
                Some(body),
                scopes,
                locals,
                span,
                field_coverage,
            );
        }
        Expr::Aggregate { binder, value, .. } => {
            return check_init_forall_read_binder(
                binder,
                value.as_deref(),
                scopes,
                locals,
                span,
                field_coverage,
            );
        }
        _ => {}
    }
    for child in init_expr_children(expr) {
        check_init_read_expr(child, scopes, locals, span, field_coverage)?;
    }
    Ok(())
}

/// Ordinary children shared by the read and outer-dependency visitors. Each
/// caller handles quantified/aggregate binder scopes before using this list.
fn init_expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Some(expr)
        | Expr::Neg(expr)
        | Expr::Not(expr)
        | Expr::Stage { entity: expr, .. }
        | Expr::UnaryNamed { expr, .. }
        | Expr::Is { expr, .. }
        | Expr::Field(expr, _) => vec![expr],
        Expr::Set(items) | Expr::Seq(items) => items.iter().collect(),
        Expr::Struct { fields, .. } => fields.iter().map(|(_, expr)| expr).collect(),
        Expr::Call { args, .. } => args.iter().collect(),
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => vec![left, right],
        Expr::Method { receiver, args, .. } => std::iter::once(receiver.as_ref())
            .chain(args.iter())
            .collect(),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => vec![condition, then_expr, else_expr],
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => vec![first, second, third],
        Expr::Num(_)
        | Expr::Bool(_)
        | Expr::None
        | Expr::Var(_)
        | Expr::EnumMember { .. }
        | Expr::Quantified { .. }
        | Expr::Aggregate { .. } => {
            vec![]
        }
    }
}

fn check_init_forall_read_binder(
    binder: &Binder,
    body: Option<&Expr>,
    scopes: &[InitForallReads],
    locals: &[String],
    span: fsl_core::Span,
    field_coverage: Option<&InitFieldReadCoverage<'_>>,
) -> Result<(), RuntimeError> {
    let (sources, filter) = init_binder_exprs(binder);
    for source in sources {
        check_init_read_expr(source, scopes, locals, span, field_coverage)?;
    }
    let mut nested_locals = locals.to_vec();
    nested_locals.push(binder_name(binder).to_owned());
    for child in filter.into_iter().chain(body) {
        check_init_read_expr(child, scopes, &nested_locals, span, field_coverage)?;
    }
    Ok(())
}

fn walk_init_write_ownership(
    statements: &[Statement],
    mut possibly_assigned: BTreeSet<InitWriteKey>,
    in_forall: bool,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
    field_ownership: bool,
) -> Result<BTreeSet<InitWriteKey>, RuntimeError> {
    for statement in statements {
        match statement {
            Statement::Assign { target, span, .. } => {
                let logical = logical_var(target)
                    .ok_or_else(|| runtime_error("invalid init assignment target"))?;
                let contribution = assignment_coverage(target, bound_names, model);
                let keys = if !field_ownership && matches!(target, LValue::Field(_, _)) {
                    BTreeSet::from([InitWriteKey::Root(logical.to_owned())])
                } else {
                    init_write_keys(target, contribution.as_ref())
                };
                if init_writes_overlap(&possibly_assigned, &keys) {
                    let scope = if in_forall { "init forall" } else { "init" };
                    return Err(runtime_error_at(
                        format!("state variable '{logical}' assigned more than once in {scope}"),
                        *span,
                    ));
                }
                possibly_assigned.extend(keys);
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                // Preserve main's ownership resolution, including #1515. Only
                // the failed-legacy admission analysis uses the scoped policy.
                let binder_values = compute_binder_values(binder, model, None)?;
                let mut nested_bound = bound_names.clone();
                nested_bound.insert(binder_name(binder).to_owned(), binder_values);
                possibly_assigned = walk_init_write_ownership(
                    statements,
                    possibly_assigned,
                    true,
                    &nested_bound,
                    model,
                    field_ownership,
                )?;
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                let then_possible = walk_init_write_ownership(
                    then_statements,
                    possibly_assigned.clone(),
                    in_forall,
                    bound_names,
                    model,
                    field_ownership,
                )?;
                let else_possible = walk_init_write_ownership(
                    else_statements,
                    possibly_assigned,
                    in_forall,
                    bound_names,
                    model,
                    field_ownership,
                )?;
                possibly_assigned = then_possible.union(&else_possible).cloned().collect();
            }
        }
    }
    Ok(possibly_assigned)
}

#[allow(clippy::too_many_lines)]
fn walk_init(
    statements: &[Statement],
    mut definitely_assigned: BTreeMap<String, Coverage>,
    mut possibly_assigned: BTreeSet<InitWriteKey>,
    in_forall: bool,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
    mode: InitWalkMode<'_>,
) -> Result<(BTreeMap<String, Coverage>, BTreeSet<InitWriteKey>), RuntimeError> {
    let field_read_roots = match mode {
        InitWalkMode::ScopedFields(roots) | InitWalkMode::ReadOrder(roots, _) => Some(roots),
        InitWalkMode::Concrete | InitWalkMode::LegacyConcrete => None,
    };
    let read_order = matches!(mode, InitWalkMode::ReadOrder(..));
    let field_reads_only = field_read_roots.is_some();
    let legacy = matches!(mode, InitWalkMode::LegacyConcrete);
    let proof_scope = matches!(mode, InitWalkMode::ScopedFields(_)).then_some(bound_names);
    let nonempty_scope = match mode {
        InitWalkMode::ReadOrder(_, false) | InitWalkMode::LegacyConcrete => proof_scope,
        _ => Some(bound_names),
    };
    for statement in statements {
        match statement {
            Statement::Assign {
                target,
                value,
                span,
                ..
            } => {
                let logical = logical_var(target)
                    .ok_or_else(|| runtime_error("invalid init assignment target"))?;
                let contribution = if legacy
                    && matches!(target, LValue::Field(base, _) if matches!(base.as_ref(), LValue::Index(_, _)))
                {
                    None
                } else {
                    assignment_coverage(target, bound_names, model)
                };
                let keys = if legacy && matches!(target, LValue::Field(_, _)) {
                    BTreeSet::from([InitWriteKey::Root(logical.to_owned())])
                } else {
                    init_write_keys(target, contribution.as_ref())
                };
                if init_writes_overlap(&possibly_assigned, &keys) {
                    let scope = if in_forall { "init forall" } else { "init" };
                    return Err(runtime_error_at(
                        format!("state variable '{logical}' assigned more than once in {scope}"),
                        *span,
                    ));
                }
                if let Some(roots) = field_read_roots {
                    let target_expr = init_target_expr(target);
                    if let Some((_, Some(key), _)) = init_read_place(&target_expr) {
                        check_init_field_expr(
                            key,
                            &definitely_assigned,
                            bound_names,
                            model,
                            *span,
                            roots,
                            read_order,
                        )?;
                    }
                    check_init_field_expr(
                        value,
                        &definitely_assigned,
                        bound_names,
                        model,
                        *span,
                        roots,
                        read_order,
                    )?;
                } else {
                    // Concrete mode checks Index keys, not Field(Index) keys.
                    // An unresolved Field(Index) cannot establish full-root
                    // coverage, so the final deterministic-init gate refuses it.
                    if let LValue::Index(_, key_expr) = target {
                        check_init_expr(key_expr, &definitely_assigned, bound_names, model)?;
                    }
                    check_init_expr(value, &definitely_assigned, bound_names, model)?;
                }
                if let Some(contribution) = contribution {
                    let previous = definitely_assigned.remove(logical);
                    definitely_assigned
                        .insert(logical.to_owned(), join_coverage(previous, contribution));
                }
                possibly_assigned.extend(keys);
            }
            Statement::ForAll {
                binder,
                statements,
                span,
            } => {
                if in_forall && !field_reads_only {
                    return Err(runtime_error("nested forall in init is not supported"));
                }
                if let Some(roots) = field_read_roots.filter(|_| !read_order) {
                    let (sources, _) = init_binder_exprs(binder);
                    for source in sources {
                        check_init_field_expr(
                            source,
                            &definitely_assigned,
                            bound_names,
                            model,
                            *span,
                            roots,
                            read_order,
                        )?;
                    }
                }
                match binder {
                    Binder::Range { lo, hi, .. } if !field_reads_only => {
                        let mut references =
                            state_references(lo, model, bound_names.keys().map(String::as_str));
                        references.extend(state_references(
                            hi,
                            model,
                            bound_names.keys().map(String::as_str),
                        ));
                        if let Some(name) = references.first() {
                            return Err(runtime_error(format!(
                                "init forall range bounds must be compile-time constants; state variable '{name}' is not allowed"
                            )));
                        }
                    }
                    Binder::Collection { .. } if !field_reads_only => {
                        fsl_core::check_init_collection_binder(
                            model,
                            binder,
                            bound_names.keys().map(String::as_str),
                        )
                        .map_err(|error| runtime_error(error.message))?;
                    }
                    Binder::Typed { .. } | Binder::Range { .. } | Binder::Collection { .. } => {}
                }
                let where_expr = match binder {
                    Binder::Typed { where_expr, .. }
                    | Binder::Range { where_expr, .. }
                    | Binder::Collection { where_expr, .. } => where_expr,
                };
                let binder_values = compute_binder_values(binder, model, proof_scope)?;
                let binder_values = binder_values.or_else(|| match mode {
                    InitWalkMode::Concrete | InitWalkMode::ReadOrder(_, true) => {
                        init_binder_static_values(binder, model, Some(bound_names))
                    }
                    _ => None,
                });
                let mut nested_bound = bound_names.clone();
                nested_bound.insert(binder_name(binder).to_owned(), binder_values);
                // Only unresolved binders retain #1520's current-key exemption.
                // Check leaves filtered binders unresolved; concrete/refinement resolve static values.
                let mut nested_roots = BTreeSet::new();
                let nested_mode = if let InitWalkMode::ReadOrder(roots, resolve_static) = mode
                    && nested_bound[binder_name(binder)].is_none()
                {
                    collect_assigned_init_roots(statements, &mut nested_roots);
                    nested_roots = roots.difference(&nested_roots).cloned().collect();
                    InitWalkMode::ReadOrder(&nested_roots, resolve_static)
                } else {
                    mode
                };
                if let Some(where_expr) = where_expr {
                    if let Some(roots) = field_read_roots {
                        check_init_field_expr(
                            where_expr,
                            &definitely_assigned,
                            &nested_bound,
                            model,
                            *span,
                            if read_order { &nested_roots } else { roots },
                            read_order,
                        )?;
                    } else {
                        check_init_expr(where_expr, &definitely_assigned, &nested_bound, model)?;
                    }
                }
                let before_forall = definitely_assigned.clone();
                (definitely_assigned, possibly_assigned) = walk_init(
                    statements,
                    definitely_assigned,
                    possibly_assigned,
                    true,
                    &nested_bound,
                    model,
                    nested_mode,
                )?;
                if !init_binder_is_nonempty(binder, model, nonempty_scope) {
                    definitely_assigned = before_forall;
                }
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => {
                if let Some(roots) = field_read_roots {
                    check_init_field_expr(
                        condition,
                        &definitely_assigned,
                        bound_names,
                        model,
                        *span,
                        roots,
                        read_order,
                    )?;
                } else {
                    check_init_expr(condition, &definitely_assigned, bound_names, model)?;
                }
                let (then_definite, then_possible) = walk_init(
                    then_statements,
                    definitely_assigned.clone(),
                    possibly_assigned.clone(),
                    in_forall,
                    bound_names,
                    model,
                    mode,
                )?;
                let (else_definite, else_possible) = walk_init(
                    else_statements,
                    definitely_assigned,
                    possibly_assigned,
                    in_forall,
                    bound_names,
                    model,
                    mode,
                )?;
                definitely_assigned = meet_branches(then_definite, &else_definite);
                possibly_assigned = then_possible.union(&else_possible).cloned().collect();
            }
        }
    }
    Ok((definitely_assigned, possibly_assigned))
}

fn check_init_field_expr(
    expr: &Expr,
    assigned: &BTreeMap<String, Coverage>,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
    span: fsl_core::Span,
    roots: &BTreeSet<String>,
    read_order: bool,
) -> Result<(), RuntimeError> {
    if read_order {
        // Exempt free/current-forall roots only in this expression's read check.
        let mut assigned = assigned.clone();
        assigned.extend(
            model
                .state
                .iter()
                .filter(|(name, _)| !roots.contains(name))
                .map(|(name, _)| (name.clone(), Coverage::Full)),
        );
        return check_init_expr(expr, &assigned, bound_names, model)
            .map_err(|error| runtime_error_at(error.message, span));
    }
    let locals = bound_names.keys().cloned().collect::<Vec<_>>();
    check_init_read_expr(
        expr,
        &[],
        &locals,
        span,
        Some(&InitFieldReadCoverage {
            assigned,
            model,
            roots,
        }),
    )
}

fn check_init_expr(
    expr: &Expr,
    definitely_assigned: &BTreeMap<String, Coverage>,
    bound_names: &BTreeMap<String, Option<Vec<Value>>>,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    let references = state_references(expr, model, bound_names.keys().map(String::as_str));
    if let Some(name) = references.iter().find(|name| {
        let ty = model.state_type(name.as_str());
        !ty.is_some_and(|ty| coverage_is_full(definitely_assigned.get(name.as_str()), ty, model))
    }) {
        let mut assigned_roots = BTreeSet::new();
        collect_assigned_init_roots(&model.init, &mut assigned_roots);
        if !assigned_roots.contains(name.as_str()) {
            return Err(runtime_error(format!(
                "the explicit engine does not support a free initial value of state variable '{name}' (nondeterministic init); use --engine bmc"
            )));
        }
        return Err(runtime_error(format!(
            "init references state variable '{name}' before it is assigned"
        )));
    }
    Ok(())
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

/// Concrete per-key/per-field identities a single init assignment touches, for
/// duplicate-write detection at the granularity `docs/manual/LANGUAGE.md` §12
/// requires: "assign exactly once" is per concrete key and field, not per variable.
///
/// This reuses `assignment_coverage`'s resolution rather than computing a
/// second, independent classification (issue #821): a `forall i { m[i] =
/// ... }` whose index is the binder itself resolves to `Coverage::Keys` of
/// every binder value, so it collides key-for-key with a later concrete
/// write to any of those same keys, and an enum-member key resolves to the
/// same `Value` a numeric literal denoting it would.
///
/// Root struct fields and concrete map-entry fields retain their field identity.
/// Falls back to `InitWriteKey::Root` (whole-variable) for unresolved coverage
/// (`None`, e.g. a dynamic key or an unsupported nested field lvalue).
/// `init_write_keys_overlap` treats that root as touching every concrete key
/// on the same logical variable, so unresolved indexed writes fail closed
/// against both concrete writes and other unresolved writes (issue #826).
fn init_write_keys(target: &LValue, coverage: Option<&Coverage>) -> BTreeSet<InitWriteKey> {
    if let (LValue::Index(name, _), Some(Coverage::Keys(keys))) = (target, coverage) {
        return keys
            .keys()
            .cloned()
            .map(|key| InitWriteKey::ConcreteIndex(name.clone(), key))
            .collect();
    }
    if let LValue::Field(base, field) = target {
        match (base.as_ref(), coverage) {
            (LValue::Var(name), Some(Coverage::Fields(_))) => {
                return BTreeSet::from([InitWriteKey::RootField(name.clone(), field.clone())]);
            }
            (LValue::Index(name, _), Some(Coverage::Keys(keys))) => {
                return keys
                    .keys()
                    .cloned()
                    .map(|key| InitWriteKey::ConcreteIndexField(name.clone(), key, field.clone()))
                    .collect();
            }
            _ => {}
        }
    }
    BTreeSet::from([InitWriteKey::Root(
        logical_var(target)
            .expect("kernel lvalue has a logical root")
            .to_owned(),
    )])
}

/// Whether one of `keys` overlaps one of `previous`
/// ([`init_write_keys_overlap`]). Keys on different roots never overlap, so
/// each key is compared only with the `previous` keys on its own root: a
/// `forall` writing every key of one map no longer compares each key with
/// every key an earlier `forall` wrote to another map.
fn init_writes_overlap(previous: &BTreeSet<InitWriteKey>, keys: &BTreeSet<InitWriteKey>) -> bool {
    let roots = keys
        .iter()
        .map(init_write_key_root)
        .collect::<BTreeSet<_>>();
    let candidates = previous
        .iter()
        .filter(|previous| roots.contains(init_write_key_root(previous)))
        .collect::<Vec<_>>();
    keys.iter().any(|key| {
        candidates
            .iter()
            .any(|previous| init_write_keys_overlap(previous, key))
    })
}

fn init_write_key_root(key: &InitWriteKey) -> &str {
    match key {
        InitWriteKey::Root(root)
        | InitWriteKey::ConcreteIndex(root, _)
        | InitWriteKey::ConcreteIndexField(root, _, _)
        | InitWriteKey::RootField(root, _) => root,
    }
}

fn init_write_keys_overlap(left: &InitWriteKey, right: &InitWriteKey) -> bool {
    #[cfg(test)]
    crate::evaluation_counts::INIT_WRITE_COMPARISONS.with(|count| count.set(count.get() + 1));
    match (left, right) {
        (
            InitWriteKey::Root(left),
            InitWriteKey::Root(right)
            | InitWriteKey::ConcreteIndex(right, _)
            | InitWriteKey::ConcreteIndexField(right, _, _)
            | InitWriteKey::RootField(right, _),
        )
        | (
            InitWriteKey::ConcreteIndex(left, _)
            | InitWriteKey::ConcreteIndexField(left, _, _)
            | InitWriteKey::RootField(left, _),
            InitWriteKey::Root(right),
        )
        | (
            InitWriteKey::RootField(left, _),
            InitWriteKey::ConcreteIndex(right, _) | InitWriteKey::ConcreteIndexField(right, _, _),
        )
        | (
            InitWriteKey::ConcreteIndex(left, _) | InitWriteKey::ConcreteIndexField(left, _, _),
            InitWriteKey::RootField(right, _),
        ) => left == right,
        (
            InitWriteKey::ConcreteIndex(left_root, left_key)
            | InitWriteKey::ConcreteIndexField(left_root, left_key, _),
            InitWriteKey::ConcreteIndex(right_root, right_key),
        )
        | (
            InitWriteKey::ConcreteIndex(left_root, left_key),
            InitWriteKey::ConcreteIndexField(right_root, right_key, _),
        ) => left_root == right_root && left_key == right_key,
        (
            InitWriteKey::ConcreteIndexField(left_root, left_key, left_field),
            InitWriteKey::ConcreteIndexField(right_root, right_key, right_field),
        ) => left_root == right_root && left_key == right_key && left_field == right_field,
        (
            InitWriteKey::RootField(left_root, left_field),
            InitWriteKey::RootField(right_root, right_field),
        ) => left_root == right_root && left_field == right_field,
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}
