// SPDX-License-Identifier: Apache-2.0

//! Solver-dependent vacuity lanes (`docs/DESIGN-vacuity.md` §2 lanes 3–5).
//!
//! The two reachability lanes (`vacuous_implication`, `vacuous_leadsto`) are
//! solver-independent and live in `fsl-runtime`. The three lanes here need Z3,
//! so they must stay on this side of the `fsl-runtime`/`fsl-solver` boundary
//! (`AGENTS.md`: `fsl-runtime` must remain independent of `fsl-solver`).
//!
//! Every lane is a **bounded number of one-shot queries over freshly named
//! states**, decided before the bounded-model-checking unrolling accumulates
//! path constraints. That placement is what makes them depth-independent:
//! each judgment quantifies over the declared type space, which is a superset
//! of the reachable states, so "unsat" here means "unsat in every reachable
//! state at every depth". A judgment that would disappear at a larger `--depth`
//! is never produced (issue #465, accepted resolution (b)).
//!
//! Inconclusive backends never produce a finding: `SatResult::Unknown` is
//! treated as "not proven", the same fail-quiet direction the lanes take for
//! every other unproven obligation.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    ActionDef, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, PropertyDef, Span,
};
use fsl_solver::{SatResult, SmtSolver};

use crate::VerifyError;
use crate::eval::eval;
use crate::transition::{ActionInstance, action_guards, init_constraints, transition_constraint};
use crate::value::{
    Bindings, SymbolicState, bool_term, bounds, i64_index, logical_equal,
    symbolic_state_with_suffix,
};

/// One proven solver-dependent vacuity fact, carried backend-neutrally to the
/// frontend that renders warning JSON.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VacuityFinding {
    /// A user invariant that holds for every dynamics because the state it
    /// depends on is pinned by init and never assigned by any action.
    TautologyOverFrozen {
        invariant: String,
        span: Span,
        frozen_vars: Vec<String>,
    },
    /// The generated `tick` action of a requirements `deadline` is dead
    /// because the generated urgent condition is an inductive invariant.
    UrgencyFreeze {
        span: Span,
        /// The generated deadline invariants rendered vacuous by the freeze.
        /// The first one carries the requirement metadata for the warning.
        deadlines: Vec<String>,
    },
    /// A `requires` clause that cannot be false in any type-valid state once
    /// its preceding clauses hold, for every instance of the action.
    AlwaysTrueRequires {
        action: String,
        span: Span,
        clause_index: usize,
    },
}

impl VacuityFinding {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::TautologyOverFrozen { .. } => "tautology_over_frozen",
            Self::UrgencyFreeze { .. } => "urgency_freeze",
            Self::AlwaysTrueRequires { .. } => "always_true_requires",
        }
    }

    /// The declaration the warning is named after.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::TautologyOverFrozen { invariant, .. } => invariant,
            Self::UrgencyFreeze { .. } => "tick",
            Self::AlwaysTrueRequires { action, .. } => action,
        }
    }

    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::TautologyOverFrozen { span, .. }
            | Self::UrgencyFreeze { span, .. }
            | Self::AlwaysTrueRequires { span, .. } => *span,
        }
    }
}

/// Prove the solver-dependent vacuity lanes for a model whose exploration was
/// carried out by a solver-free engine.
///
/// The lanes are properties of the model rather than of the exploration, so the
/// explicit-state engine reports exactly what bounded model checking reports;
/// `action_coverage` is the only exploration input, and it is used solely to
/// suppress `always_true_requires` for actions that were never enabled.
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported symbolic expressions or backend
/// failures.
pub async fn model_vacuity_findings<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    action_coverage: &BTreeMap<String, bool>,
) -> Result<Vec<VacuityFinding>, VerifyError> {
    let instances = crate::transition::action_instances(solver, model)?;
    let mut findings = static_findings(model, solver, &instances).await?;
    retain_covered(&mut findings, action_coverage);
    Ok(findings)
}

/// Prove every solver-dependent vacuity lane that does not depend on the
/// exploration bound.
///
/// Runs against freshly named states inside `push`/`pop`, so it neither reads
/// nor leaves any constraint on the caller's unrolling session.
///
/// `always_true_requires` findings are returned unfiltered; the caller must
/// drop the ones whose action never became enabled (`docs/DESIGN-vacuity.md` §2
/// excludes coverage-false actions, which are already reported by their own
/// warning).
///
/// # Errors
///
/// Returns [`VerifyError`] for unsupported symbolic expressions or backend
/// failures. An `unknown` backend verdict is not an error; it yields no
/// finding.
pub(crate) async fn static_findings<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    instances: &[ActionInstance<S::Term>],
) -> Result<Vec<VacuityFinding>, VerifyError> {
    let mut findings = frozen_tautologies(model, solver).await?;
    if let Some(finding) = urgency_freeze(model, solver, instances).await? {
        findings.push(finding);
    }
    findings.extend(always_true_requires(model, solver, instances).await?);
    Ok(findings)
}

/// State variables no action ever assigns.
///
/// `transition_constraint` frames every unassigned state variable forward
/// unchanged, so such a variable equals its init value in every reachable
/// state. The walk must stay complete: a missed assignment form would call a
/// live variable frozen and manufacture a false positive.
fn frozen_state_vars(model: &KernelModel) -> BTreeSet<String> {
    let mut assigned = BTreeSet::new();
    for action in &model.actions {
        collect_assigned_roots(&action.statements, &mut assigned);
    }
    model
        .state
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| !assigned.contains(name))
        .collect()
}

fn collect_assigned_roots(statements: &[Statement], out: &mut BTreeSet<String>) {
    for statement in statements {
        match statement {
            Statement::Assign { target, .. } => {
                out.insert(lvalue_root(target).to_owned());
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_assigned_roots(then_statements, out);
                collect_assigned_roots(else_statements, out);
            }
            Statement::ForAll { statements, .. } => collect_assigned_roots(statements, out),
        }
    }
}

fn lvalue_root(target: &LValue) -> &str {
    match target {
        LValue::Var(name) | LValue::Index(name, _) => name,
        LValue::Field(inner, _) => lvalue_root(inner),
    }
}

/// State variables an expression may read.
///
/// Only used to decide whether a lane is worth a query; both an over- and an
/// under-approximation stay sound, because the query itself quantifies over
/// the whole type space either way.
fn referenced_state_vars(model: &KernelModel, expr: &Expr) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_vars(expr, &mut names);
    names
        .into_iter()
        .filter(|name| model.state.iter().any(|(state, _)| state == name))
        .collect()
}

fn collect_vars(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::EnumMember { .. } => {}
        Expr::Var(name) => {
            out.insert(name.clone());
        }
        Expr::Some(inner)
        | Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::Field(inner, _)
        | Expr::UnaryNamed { expr: inner, .. }
        | Expr::Stage { entity: inner, .. } => collect_vars(inner, out),
        Expr::Set(items) | Expr::Seq(items) => {
            for item in items {
                collect_vars(item, out);
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                collect_vars(value, out);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_vars(arg, out);
            }
        }
        Expr::Method { receiver, args, .. } => {
            collect_vars(receiver, out);
            for arg in args {
                collect_vars(arg, out);
            }
        }
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            collect_vars(left, out);
            collect_vars(right, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            collect_vars(condition, out);
            collect_vars(then_expr, out);
            collect_vars(else_expr, out);
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            collect_vars(first, out);
            collect_vars(second, out);
            collect_vars(third, out);
        }
        Expr::Is { expr, .. } => collect_vars(expr, out),
        Expr::Quantified { binder, body, .. } => {
            collect_binder_vars(binder, out);
            collect_vars(body, out);
        }
        Expr::Aggregate { binder, value, .. } => {
            collect_binder_vars(binder, out);
            if let Some(value) = value {
                collect_vars(value, out);
            }
        }
    }
}

fn collect_binder_vars(binder: &Binder, out: &mut BTreeSet<String>) {
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(where_expr) = where_expr {
                collect_vars(where_expr, out);
            }
        }
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            collect_vars(lo, out);
            collect_vars(hi, out);
            if let Some(where_expr) = where_expr {
                collect_vars(where_expr, out);
            }
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            collect_vars(collection, out);
            if let Some(where_expr) = where_expr {
                collect_vars(where_expr, out);
            }
        }
    }
}

fn is_generated_invariant(model: &KernelModel, name: &str) -> bool {
    model
        .property_origin("invariant", name)
        .is_some_and(|origin| origin.generated)
}

fn is_generated_action(model: &KernelModel, name: &str) -> bool {
    model
        .action_origin(name)
        .is_some_and(|origin| origin.generated)
}

/// Whether a declaration points at real source text.
///
/// `OriginChain::generated` alone does not separate authored declarations from
/// generated ones: some dialect lowerings synthesize a whole catalog kernel
/// (the governance catalog's `_governance_catalog_ok`, whose `_governance_ok`
/// is a generated Bool that is true at init and assigned by nothing) and give
/// the synthetic declarations a zero span instead of a `generated_only`
/// origin. Source lines are 1-based, so line 0 means "no source text behind
/// this". `docs/DESIGN-vacuity.md` §2 scopes these lanes to **user**
/// declarations; a warning that cannot point at anything the author wrote is
/// noise, and it is the frozen scaffolding of every governance spec.
fn is_source_backed(span: Span) -> bool {
    span.start.line > 0
}

fn all_bounds<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<Vec<S::Term>, VerifyError> {
    model
        .state
        .iter()
        .map(|(name, _)| {
            bounds(
                solver,
                model,
                state
                    .get(name)
                    .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
            )
        })
        .collect()
}

/// Lane 4 — `tautology_over_frozen`.
///
/// Sound because every reachable state agrees with some init state on the
/// frozen variables (nothing assigns them) and satisfies the declared type
/// bounds. If no such state can falsify the invariant, the invariant holds
/// everywhere for reasons independent of the dynamics, which is exactly the
/// hollow-invariant claim. Unlike the frozen Python reference this leaves init
/// symbolic instead of picking one completed init model, so a non-deterministic
/// init cannot make the judgment depend on an arbitrary witness.
async fn frozen_tautologies<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
) -> Result<Vec<VacuityFinding>, VerifyError> {
    let frozen = frozen_state_vars(model);
    if frozen.is_empty() {
        return Ok(Vec::new());
    }
    // One pair of probe states for the whole lane: every constraint below is
    // asserted inside `proven_unsat`'s push/pop, so nothing carries between
    // invariants. Allocating per invariant instead would multiply the Z3 term
    // count by the invariant count for no gain, which matters most in the
    // browser backend's bounded heap.
    let init = symbolic_state_with_suffix(solver, model, "vac_frozen_init")?;
    let state = symbolic_state_with_suffix(solver, model, "vac_frozen")?;
    let bound_terms = all_bounds(solver, model, &state)?;
    let init_terms = init_constraints(solver, model, &init)?;
    let mut findings = Vec::new();
    for property in &model.invariants {
        if is_generated_invariant(model, &property.name) || !is_source_backed(property.span) {
            continue;
        }
        let frozen_vars = referenced_state_vars(model, &property.expr)
            .intersection(&frozen)
            .cloned()
            .collect::<Vec<_>>();
        if frozen_vars.is_empty() {
            continue;
        }
        let mut terms = init_terms.clone();
        for name in &frozen_vars {
            let missing = || VerifyError::new(format!("missing state '{name}'"));
            terms.push(logical_equal(
                solver,
                model,
                state.get(name).ok_or_else(missing)?,
                init.get(name).ok_or_else(missing)?,
            )?);
        }
        terms.extend(bound_terms.iter().cloned());
        let mut bindings = Bindings::new();
        let value = eval(solver, model, &property.expr, &state, &mut bindings, None)?;
        terms.push(solver.not(bool_term(&value)?)?);
        solver.set_query_context("vacuity", &property.name);
        if proven_unsat(solver, &terms).await? {
            findings.push(VacuityFinding::TautologyOverFrozen {
                invariant: property.name.clone(),
                span: property.span,
                frozen_vars,
            });
        }
    }
    Ok(findings)
}

/// The generated deadline invariants a requirements `time`/`deadline` block
/// lowers to.
fn deadline_invariants(model: &KernelModel) -> Vec<&PropertyDef> {
    model
        .invariants
        .iter()
        .filter(|property| {
            property.name.starts_with("_deadline_") && is_generated_invariant(model, &property.name)
        })
        .collect()
}

/// The unique generated `tick` action, if the spec has one.
fn generated_tick(model: &KernelModel) -> Option<&ActionDef> {
    let mut ticks = model.actions.iter().filter(|action| action.name == "tick");
    let tick = ticks.next()?;
    if ticks.next().is_some() || !is_generated_action(model, "tick") {
        return None;
    }
    Some(tick)
}

/// The structural `requires not(<urgent>)` guard the `tick` generator emits.
fn tick_urgent_expr(tick: &ActionDef) -> Option<&Expr> {
    match tick.requires.as_slice() {
        [Expr::Not(inner)] => Some(inner),
        _ => None,
    }
}

/// The age state variables the generated `forall* (age <= bound)` deadline
/// invariants constrain. `None` when any deadline has an unexpected shape,
/// which suppresses the lane rather than guessing.
fn deadline_age_refs(deadlines: &[&PropertyDef]) -> Option<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    for property in deadlines {
        let mut expr = &property.expr;
        while let Expr::Quantified {
            quantifier, body, ..
        } = expr
        {
            if quantifier != "forall" {
                break;
            }
            expr = body;
        }
        let Expr::Binary { op, left, .. } = expr else {
            return None;
        };
        if op != "<=" {
            return None;
        }
        refs.insert(state_root_expr(left)?);
    }
    Some(refs)
}

fn state_root_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(name) => Some(name.clone()),
        Expr::Index(base, _) => match &**base {
            Expr::Var(name) => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn non_tick_assigns_any(model: &KernelModel, roots: &BTreeSet<String>) -> bool {
    model.actions.iter().any(|action| {
        if action.name == "tick" {
            return false;
        }
        let mut assigned = BTreeSet::new();
        collect_assigned_roots(&action.statements, &mut assigned);
        !assigned.is_disjoint(roots)
    })
}

/// Lane 5 — `urgency_freeze`.
///
/// Sound because `urgent && type bounds` is proven to be a genuine inductive
/// invariant: it holds in every init state and every action preserves it. The
/// generated `tick` guard is `not urgent`, so `tick` is enabled in no reachable
/// state, the age variables (which nothing but `tick` assigns) never advance,
/// and the deadline invariants hold for want of time passing. Both obligations
/// are single-step queries over fresh states, so the verdict does not move with
/// `--depth`. Intentionally incomplete: an unproven obligation emits nothing.
async fn urgency_freeze<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    instances: &[ActionInstance<S::Term>],
) -> Result<Option<VacuityFinding>, VerifyError> {
    let deadlines = deadline_invariants(model);
    if deadlines.is_empty() || instances.is_empty() {
        return Ok(None);
    }
    let Some(tick) = generated_tick(model) else {
        return Ok(None);
    };
    let Some(urgent) = tick_urgent_expr(tick) else {
        return Ok(None);
    };
    let Some(age_refs) = deadline_age_refs(&deadlines) else {
        return Ok(None);
    };
    if age_refs.is_empty() || non_tick_assigns_any(model, &age_refs) {
        return Ok(None);
    }

    solver.set_query_context("vacuity", "tick");
    let init = symbolic_state_with_suffix(solver, model, "vac_urgency_init")?;
    let mut base = init_constraints(solver, model, &init)?;
    base.push(solver.not(&strengthened_urgent(solver, model, urgent, &init)?)?);
    if !proven_unsat(solver, &base).await? {
        return Ok(None);
    }

    let current = symbolic_state_with_suffix(solver, model, "vac_urgency_cur")?;
    let next = symbolic_state_with_suffix(solver, model, "vac_urgency_next")?;
    let choice = solver.constant("__vac_urgency_choice", &fsl_solver::Sort::Int)?;
    let mut step = vec![
        solver.ge(&choice, &solver.int_value(0))?,
        solver.lt(&choice, &solver.int_value(i64_index(instances.len())?))?,
        strengthened_urgent(solver, model, urgent, &current)?,
        transition_constraint(solver, model, instances, &current, &next, &choice)?,
    ];
    step.push(solver.not(&strengthened_urgent(solver, model, urgent, &next)?)?);
    if !proven_unsat(solver, &step).await? {
        return Ok(None);
    }

    Ok(Some(VacuityFinding::UrgencyFreeze {
        span: tick.require_spans.first().copied().unwrap_or(tick.span),
        deadlines: deadlines
            .iter()
            .map(|property| property.name.clone())
            .collect(),
    }))
}

/// `urgent && <every declared type bound>`, the predicate proven inductive by
/// the `urgency_freeze` lane. Carrying the bounds inside the induction is what
/// keeps the proof self-contained: nothing is assumed from the bounded
/// exploration that produced the `verified` verdict.
fn strengthened_urgent<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    urgent: &Expr,
    state: &SymbolicState<S::Term>,
) -> Result<S::Term, VerifyError> {
    let mut bindings = Bindings::new();
    let value = eval(solver, model, urgent, state, &mut bindings, None)?;
    let mut terms = vec![bool_term(&value)?.clone()];
    terms.extend(all_bounds(solver, model, state)?);
    Ok(solver.and(&terms)?)
}

/// Lane 3 — `always_true_requires`.
///
/// For clause `j` of an action, ask whether `type bounds && clauses[..j] &&
/// not clauses[j]` is unsatisfiable. The declared type space is a superset of
/// the reachable states, so an unsatisfiable answer means the clause cannot be
/// false in any reachable state at any depth — the property
/// `docs/DESIGN-vacuity.md` §2 lane 3 actually asks for. The frozen Python
/// reference instead discharges the clause from states witnessed within
/// `--depth`, which reports a guard as dead merely because the bound was too
/// small (issue #465: `examples/causal/funnel.fsl`'s `requires visits < 100`
/// with `visits: 0..100`). That depth artifact is deliberately not inherited.
///
/// The clause must be implied for **every** instance of the action, matching
/// the reference's per-action suppression: one instance that can falsify the
/// clause makes it a real guard.
///
/// Synchronized compose actions are excluded because their clauses are
/// inherited copies from several components, where a duplicated guard is the
/// intended "each component defends its own contract" design rather than
/// removable redundancy. Generated actions are excluded as authored-code
/// diagnostics.
async fn always_true_requires<S: SmtSolver>(
    model: &KernelModel,
    solver: &mut S,
    instances: &[ActionInstance<S::Term>],
) -> Result<Vec<VacuityFinding>, VerifyError> {
    // One probe state for the whole lane, for the same reason as
    // `frozen_tautologies`: the guards differ between instances only through
    // their concrete parameter bindings, never through the state.
    let state = symbolic_state_with_suffix(solver, model, "vac_requires")?;
    let bound_terms = all_bounds(solver, model, &state)?;
    let mut findings = Vec::new();
    for (action_index, action) in model.actions.iter().enumerate() {
        if action.sync
            || action.requires.is_empty()
            || is_generated_action(model, &action.name)
            || !action.require_spans.iter().copied().all(is_source_backed)
        {
            continue;
        }
        let mut implied = vec![true; action.requires.len()];
        let mut checked = false;
        for instance in instances {
            if instance.action_index != action_index {
                continue;
            }
            let (guards, _) = action_guards(solver, model, action, &state, &instance.params)?;
            if guards.len() != action.requires.len() {
                // A `let` between clauses cannot change the count, but never
                // index a mismatched pair: skip the whole action instead.
                implied.fill(false);
                break;
            }
            checked = true;
            for (clause_index, guard) in guards.iter().enumerate() {
                if !implied[clause_index] {
                    continue;
                }
                let mut terms = bound_terms.clone();
                terms.extend(guards[..clause_index].iter().cloned());
                terms.push(solver.not(guard)?);
                solver.set_query_context("vacuity", &action.name);
                if !proven_unsat(solver, &terms).await? {
                    implied[clause_index] = false;
                }
            }
            if implied.iter().all(|entry| !entry) {
                break;
            }
        }
        if !checked {
            continue;
        }
        findings.extend(implied.iter().enumerate().filter(|(_, entry)| **entry).map(
            |(clause_index, _)| {
                VacuityFinding::AlwaysTrueRequires {
                    action: action.name.clone(),
                    span: action
                        .require_spans
                        .get(clause_index)
                        .copied()
                        .unwrap_or(action.span),
                    clause_index,
                }
            },
        ));
    }
    Ok(findings)
}

/// Whether the conjunction of `terms` is *proven* unsatisfiable.
///
/// An `unknown` backend verdict answers `false`: a vacuity lane may only fire
/// on a discharged proof obligation, never on the absence of a countermodel.
async fn proven_unsat<S: SmtSolver>(
    solver: &mut S,
    terms: &[S::Term],
) -> Result<bool, VerifyError> {
    solver.push();
    for term in terms {
        if let Err(error) = solver.assert(term) {
            solver.pop(1)?;
            return Err(error.into());
        }
    }
    let checked = solver.check().await;
    let popped = solver.pop(1);
    let result = checked?;
    popped?;
    Ok(matches!(result, SatResult::Unsat))
}

/// Drop `always_true_requires` findings for actions that were never enabled
/// within the explored bound. `docs/DESIGN-vacuity.md` §2 lane 3 puts
/// coverage-false actions out of scope: they already carry their own
/// never-enabled warning, and every clause of a dead action is trivially
/// redundant given the preceding ones.
pub(crate) fn retain_covered(
    findings: &mut Vec<VacuityFinding>,
    coverage: &BTreeMap<String, bool>,
) {
    findings.retain(|finding| match finding {
        VacuityFinding::AlwaysTrueRequires { action, .. } => {
            coverage.get(action).copied().unwrap_or(false)
        }
        _ => true,
    });
}
