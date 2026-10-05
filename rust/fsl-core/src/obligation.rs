// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The obligation catalog (issues #1201, #1202).
//!
//! [`catalog`] lists every question a verdict on one [`KernelModel`] rests on:
//! one row per authored site and aspect, generated from the model's
//! declarations rather than from what an engine asks
//! (`docs/design/DESIGN-obligation-catalog.md`). It is a pure function of the
//! model -- no solver, no property selection -- so the rows a run owes exist
//! before any engine decides which of them it discharges.

use std::collections::BTreeMap;

use fsl_syntax::{Binder, Expr, LValue, Pattern, Span, Statement};

use crate::partial_operation::{
    for_each_partial_operation_candidate, statement_has_partial_operation_candidate,
};
use crate::typecheck::{TypeEnv, base_env, binder_type, resolve, struct_field_type};
use crate::{
    ActionDef, ActionGuard, KernelAggregateKind, KernelModel, ParamDef, PartialOperation,
    PropertyDef, TypeRef, expression_has_partial_operation_candidate, recursion,
};

/// The aspect of a site an obligation asks about.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObligationKind {
    /// The site's condition holds: a state variable's type bound, an
    /// `invariant`, a `trans`, an `ensures` clause.
    Holds,
    /// Some reachable state satisfies a `reachable`.
    Witnessed,
    /// Every pending `leadsTo` trigger is eventually followed by its goal.
    Responds,
    /// A `leadsTo ... within K` goal follows its trigger within `K` steps.
    Deadline,
    /// Evaluating the site reaches none of the partial operations of
    /// `docs/manual/LANGUAGE.md` §6.
    PartialDefined,
    /// Evaluating the site overflows no `i64` arithmetic (`+ - * / %`, unary
    /// `-`, `abs`, `sum`).
    NoOverflow,
    /// Every `Map` index the site reads or writes is inside the map's finite
    /// key domain.
    KeyInDomain,
    /// A ranked `leadsTo`'s measure is non-negative while it is pending.
    RankLowerBound,
    /// A ranked `leadsTo` is never pending in a state that enables no action
    /// (#1189).
    RankNoDeadlock,
    /// One action, taken while a ranked `leadsTo` is pending, reaches the goal
    /// or decreases the measure (a non-`helpful` action: does not increase it).
    RankStep,
    /// The actions a `helpful` clause names are declared `fair`.
    RankHelpfulFair,
    /// A pending `helpful` action stays enabled until it fires or the goal
    /// holds.
    RankHelpfulSticky,
    /// The `init` block admits an initial state.
    InitSatisfiable,
    /// Every reachable non-`terminal` state enables an action.
    NoDeadlock,
}

impl ObligationKind {
    /// Every obligation kind, in declaration order.
    pub const ALL: [Self; 14] = [
        Self::Holds,
        Self::Witnessed,
        Self::Responds,
        Self::Deadline,
        Self::PartialDefined,
        Self::NoOverflow,
        Self::KeyInDomain,
        Self::RankLowerBound,
        Self::RankNoDeadlock,
        Self::RankStep,
        Self::RankHelpfulFair,
        Self::RankHelpfulSticky,
        Self::InitSatisfiable,
        Self::NoDeadlock,
    ];

    /// This kind's index in [`Self::ALL`]. The match is exhaustive and each
    /// arm looks its kind up at compile time, so a kind missing from `ALL`
    /// does not build; [`catalog`] checks that every kind it generates is at
    /// its index.
    const fn position(self) -> usize {
        match self {
            Self::Holds => const { Self::index_in_all(Self::Holds) },
            Self::Witnessed => const { Self::index_in_all(Self::Witnessed) },
            Self::Responds => const { Self::index_in_all(Self::Responds) },
            Self::Deadline => const { Self::index_in_all(Self::Deadline) },
            Self::PartialDefined => const { Self::index_in_all(Self::PartialDefined) },
            Self::NoOverflow => const { Self::index_in_all(Self::NoOverflow) },
            Self::KeyInDomain => const { Self::index_in_all(Self::KeyInDomain) },
            Self::RankLowerBound => const { Self::index_in_all(Self::RankLowerBound) },
            Self::RankNoDeadlock => const { Self::index_in_all(Self::RankNoDeadlock) },
            Self::RankStep => const { Self::index_in_all(Self::RankStep) },
            Self::RankHelpfulFair => const { Self::index_in_all(Self::RankHelpfulFair) },
            Self::RankHelpfulSticky => const { Self::index_in_all(Self::RankHelpfulSticky) },
            Self::InitSatisfiable => const { Self::index_in_all(Self::InitSatisfiable) },
            Self::NoDeadlock => const { Self::index_in_all(Self::NoDeadlock) },
        }
    }

    const fn index_in_all(kind: Self) -> usize {
        Self::index_in(&Self::ALL, kind, 0)
    }

    /// `kind`'s index in `kinds`, counted from `offset`.
    const fn index_in(kinds: &[Self], kind: Self, offset: usize) -> usize {
        match kinds {
            [first, rest @ ..] => {
                if *first as usize == kind as usize {
                    offset
                } else {
                    Self::index_in(rest, kind, offset + 1)
                }
            }
            [] => panic!("an ObligationKind is missing from ObligationKind::ALL"),
        }
    }
}

/// An authored declaration: its name and the byte offsets of its span. A
/// checked model has unique property names (#1192); the offsets keep two
/// same-named declarations of a hand-built model apart.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SiteRef {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

impl SiteRef {
    fn new(name: &str, span: Span) -> Self {
        Self {
            name: name.to_owned(),
            start: span.start.offset,
            end: span.end.offset,
        }
    }
}

/// Where an obligation is owed.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Site {
    /// A state variable's declared type. State declarations carry no span, so
    /// the declaration's position in `KernelModel::state` stands in for it.
    TypeBound {
        var: String,
        position: usize,
    },
    Invariant(SiteRef),
    /// A `trans`, an `unless`, or the `<name>_until_safety` trans an `until`
    /// lowers to; the last shares its `leadsTo`'s span.
    Trans(SiteRef),
    Reachable(SiteRef),
    LeadsTo(SiteRef),
    /// The trigger `P` of a `leadsTo` `P ~> Q`.
    Trigger(SiteRef),
    /// The goal `Q` of a `leadsTo` `P ~> Q`.
    Goal(SiteRef),
    /// The `decreases` measure of a ranked `leadsTo`.
    Measure(SiteRef),
    /// One action's progress step under a ranked `leadsTo`.
    RankStep {
        leads_to: SiteRef,
        action: SiteRef,
    },
    /// The model's `terminal` condition.
    Terminal,
    /// An action's `requires` and `let` clauses, under the action's name and
    /// span.
    Guard(SiteRef),
    /// An action's statements.
    Body(SiteRef),
    /// An action's `index`-th `ensures` clause.
    Ensures {
        action: SiteRef,
        index: usize,
    },
    /// The model's `init` block.
    Init,
    /// The model as a whole.
    Model,
}

/// One row's identity: the same aspect of the same site is the same row.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObligationId {
    pub kind: ObligationKind,
    pub site: Site,
}

/// One row of the catalog.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Obligation {
    pub id: ObligationId,
    /// The site holds no candidate for this obligation: no partial operation,
    /// no overflowing operator, no index the catalog cannot place inside its
    /// key type, or a state type every value conforms to. Only type-bound and
    /// definedness rows can be vacuous. Candidates over-approximate, so a
    /// vacuous row is one no evaluation of the site can fail -- for
    /// `KeyInDomain`, provided the state it reads satisfies the model's
    /// `Holds@TypeBound` rows; a run that leaves one of those unchecked cannot
    /// rely on it.
    pub statically_vacuous: bool,
}

/// Every obligation of one model, one row per [`ObligationId`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Catalog {
    pub obligations: Vec<Obligation>,
}

/// The obligations `model` owes, generated from its declarations alone.
#[must_use]
pub fn catalog(model: &KernelModel) -> Catalog {
    let mut rows = Rows::default();
    push_init(&mut rows);
    push_state(&mut rows, model);
    push_invariants(&mut rows, model);
    push_transitions(&mut rows, model);
    push_reachables(&mut rows, model);
    push_leadstos(&mut rows, model);
    push_ranks(&mut rows, model);
    push_terminal(&mut rows, model);
    push_actions(&mut rows, model);
    push_model(&mut rows);
    debug_assert!(
        rows.0
            .iter()
            .all(|row| ObligationKind::ALL.get(row.id.kind.position()) == Some(&row.id.kind)),
        "a generated kind is missing from ObligationKind::ALL"
    );
    debug_assert_eq!(
        rows.0
            .iter()
            .map(|row| &row.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        rows.0.len(),
        "two obligations share an id"
    );
    Catalog {
        obligations: rows.0,
    }
}

#[derive(Default)]
struct Rows(Vec<Obligation>);

impl Rows {
    fn push(&mut self, kind: ObligationKind, site: Site, statically_vacuous: bool) {
        self.0.push(Obligation {
            id: ObligationId { kind, site },
            statically_vacuous,
        });
    }

    fn holds(&mut self, kind: ObligationKind, site: &Site) {
        self.push(kind, site.clone(), false);
    }

    /// The three definedness rows of `site`: partial operations, overflow,
    /// and out-of-domain `Map` keys.
    fn definedness(&mut self, site: &Site, has_partial_operation: bool, found: &Found) {
        self.push(
            ObligationKind::PartialDefined,
            site.clone(),
            !has_partial_operation,
        );
        self.push(ObligationKind::NoOverflow, site.clone(), !found.overflow);
        self.push(ObligationKind::KeyInDomain, site.clone(), !found.key);
    }

    /// The definedness rows of a property-context expression.
    fn property_definedness(&mut self, site: &Site, found: &Found, expr: &Expr) {
        self.definedness(site, property_has_partial_operation(expr), found);
    }
}

fn push_init(rows: &mut Rows) {
    rows.holds(ObligationKind::InitSatisfiable, &Site::Init);
}

fn push_state(rows: &mut Rows, model: &KernelModel) {
    for (position, (var, ty)) in model.state.iter().enumerate() {
        let site = Site::TypeBound {
            var: var.clone(),
            position,
        };
        rows.push(ObligationKind::Holds, site, !has_bounds(ty));
    }
}

fn push_invariants(rows: &mut Rows, model: &KernelModel) {
    push_properties(rows, model, &model.invariants, Site::Invariant);
}

fn push_transitions(rows: &mut Rows, model: &KernelModel) {
    push_properties(rows, model, &model.transitions, Site::Trans);
}

fn push_properties(
    rows: &mut Rows,
    model: &KernelModel,
    properties: &[PropertyDef],
    site: fn(SiteRef) -> Site,
) {
    for property in properties {
        let site = site(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Holds, &site);
        let found = expression_found(model, &[], &property.expr);
        rows.property_definedness(&site, &found, &property.expr);
    }
}

fn push_reachables(rows: &mut Rows, model: &KernelModel) {
    for property in &model.reachables {
        let site = Site::Reachable(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Witnessed, &site);
        let found = expression_found(model, &[], &property.expr);
        rows.property_definedness(&site, &found, &property.expr);
    }
}

/// A `leadsTo`'s trigger, goal and measure are each evaluated from its binder
/// bindings alone. Its binders take no `where` filter or collection and have
/// static range bounds (`static_leadsto_bindings`), so they read no state.
fn push_leadstos(rows: &mut Rows, model: &KernelModel) {
    for property in &model.leadstos {
        let at = SiteRef::new(&property.name, property.span);
        let leads_to = Site::LeadsTo(at.clone());
        rows.holds(ObligationKind::Responds, &leads_to);
        if property.within.is_some() {
            rows.holds(ObligationKind::Deadline, &leads_to);
        }
        let trigger = expression_found(model, &property.binders, &property.before);
        rows.property_definedness(&Site::Trigger(at.clone()), &trigger, &property.before);
        let goal = expression_found(model, &property.binders, &property.after);
        rows.property_definedness(&Site::Goal(at), &goal, &property.after);
    }
}

/// The ranking obligations of every `leadsTo ... decreases`
/// (`docs/design/DESIGN-induction.md` §2.3). Whether a `helpful` action
/// matches, and so whether stickiness has two instances to compare, depends on
/// each binding; the rows exist whenever `helpful` is declared.
fn push_ranks(rows: &mut Rows, model: &KernelModel) {
    for property in &model.leadstos {
        let Some(measure) = &property.decreases else {
            continue;
        };
        let at = SiteRef::new(&property.name, property.span);
        rows.definedness(
            &Site::Measure(at.clone()),
            expression_has_partial_operation_candidate(measure),
            &expression_found(model, &property.binders, measure),
        );
        let leads_to = Site::LeadsTo(at.clone());
        rows.holds(ObligationKind::RankLowerBound, &leads_to);
        rows.holds(ObligationKind::RankNoDeadlock, &leads_to);
        for action in &model.actions {
            let site = Site::RankStep {
                leads_to: at.clone(),
                action: SiteRef::new(&action.name, action.span),
            };
            rows.holds(ObligationKind::RankStep, &site);
        }
        if !property.helpful.is_empty() {
            rows.holds(ObligationKind::RankHelpfulFair, &leads_to);
            rows.holds(ObligationKind::RankHelpfulSticky, &leads_to);
        }
    }
}

fn push_terminal(rows: &mut Rows, model: &KernelModel) {
    if let Some(terminal) = &model.terminal {
        let found = expression_found(model, &[], terminal);
        rows.property_definedness(&Site::Terminal, &found, terminal);
    }
}

fn push_actions(rows: &mut Rows, model: &KernelModel) {
    for action in &model.actions {
        push_action(rows, model, action);
    }
}

fn push_action(rows: &mut Rows, model: &KernelModel, action: &ActionDef) {
    let at = SiteRef::new(&action.name, action.span);
    let mut found = settle(model, |outer| action_found(outer, action)).into_iter();
    let guard_partial = action.guards.iter().any(|clause| match clause {
        ActionGuard::Let(_, expr) | ActionGuard::Requires(expr) => {
            expression_has_partial_operation_candidate(expr)
        }
    });
    let guard = found.next().unwrap_or_default();
    rows.definedness(&Site::Guard(at.clone()), guard_partial, &guard);
    let body_partial = action
        .statements
        .iter()
        .any(statement_has_partial_operation_candidate);
    let body = found.next().unwrap_or_default();
    rows.definedness(&Site::Body(at.clone()), body_partial, &body);
    for (index, (ensures, found)) in action.ensures.iter().zip(found).enumerate() {
        let site = Site::Ensures {
            action: at.clone(),
            index,
        };
        rows.holds(ObligationKind::Holds, &site);
        rows.definedness(
            &site,
            expression_has_partial_operation_candidate(ensures),
            &found,
        );
    }
}

/// The candidates of an action's guard, body and each `ensures`, in that
/// order. They are one evaluation context: parameters, then each `let` in
/// clause order, and every pattern binding reaches the clauses, statements
/// and `ensures` evaluated after it.
fn action_found(outer: &Scope<'_>, action: &ActionDef) -> Vec<Found> {
    let mut scope = outer.clone();
    for param in &action.params {
        let ty = match param {
            ParamDef::Typed { ty, .. } => ty.clone(),
            ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
        };
        scope.env.insert(param.name().to_owned(), ty);
    }
    let mut guard = Found::default();
    for clause in &action.guards {
        match clause {
            ActionGuard::Requires(expr) => scope.expr(expr, &mut guard),
            ActionGuard::Let(name, expr) => {
                scope.expr(expr, &mut guard);
                scope.bind(name, scope.value_type(expr));
            }
        }
    }
    let mut body = Found::default();
    for statement in &action.statements {
        scope.statement(statement, &mut body);
    }
    let mut found = vec![guard, body];
    for ensures in &action.ensures {
        let mut item = Found::default();
        scope.expr(ensures, &mut item);
        found.push(item);
    }
    found
}

/// The candidates of one property-context expression under `binders`.
fn expression_found(model: &KernelModel, binders: &[Binder], expr: &Expr) -> Found {
    settle(model, |outer| {
        let scope = binders
            .iter()
            .fold(outer.clone(), |scope, binder| scope.enter(binder));
        let mut found = Found::default();
        scope.expr(expr, &mut found);
        vec![found]
    })
    .pop()
    .unwrap_or_default()
}

/// Walk one evaluation context from the outermost scope its `is some(v)`
/// patterns allow. A pattern never rebinds a parameter, `let` or binder
/// already in scope (`or_insert` in both evaluators), but it binds `v` for
/// everything evaluated after it in the context once it matches, whether or
/// not the path to that point required the match. So wherever `v` is not a
/// parameter, `let` or binder it may name any of its patterns' payloads or
/// the state variable, constant or enum member `v` (the base scope types all
/// three; `build_model` rejects a duplicate member): its outermost type is the
/// join of all of them, and none when they do not join or a payload's type
/// is not bounded. The second walk types the payloads with every pattern name
/// untyped, so no payload type rests on another pattern.
fn settle<'m>(model: &'m KernelModel, walk: impl Fn(&Scope<'m>) -> Vec<Found>) -> Vec<Found> {
    let base = Scope::new(model);
    let mut outer = base.clone();
    for name in patterns(&walk(&base)).keys() {
        outer.env.remove(name);
    }
    for (name, payload) in patterns(&walk(&outer)) {
        let ty = match base.env.get(&name) {
            Some(ty) => join(resolve(model, ty).ok(), payload),
            None => payload,
        };
        outer.bind(&name, ty);
    }
    walk(&outer)
}

/// Every pattern name of `found`, with the join of its payload types.
fn patterns(found: &[Found]) -> BTreeMap<String, Option<TypeRef>> {
    let mut joined = BTreeMap::new();
    for item in found {
        for (name, payload) in &item.patterns {
            joined
                .entry(name.clone())
                .and_modify(|ty: &mut Option<TypeRef>| *ty = join(ty.take(), payload.clone()))
                .or_insert_with(|| payload.clone());
        }
    }
    joined
}

/// A type holding every value of two resolved types: the hull of two ranges,
/// or the type itself when both are the same. `None` is a value of no known
/// type.
fn join(left: Option<TypeRef>, right: Option<TypeRef>) -> Option<TypeRef> {
    match (left?, right?) {
        (TypeRef::Range(left_lo, left_hi), TypeRef::Range(right_lo, right_hi)) => {
            Some(TypeRef::Range(left_lo.min(right_lo), left_hi.max(right_hi)))
        }
        (left, right) => (left == right).then_some(left),
    }
}

fn push_model(rows: &mut Rows) {
    rows.holds(ObligationKind::NoDeadlock, &Site::Model);
}

/// [`expression_has_partial_operation_candidate`] under property evaluation,
/// where `/` and `%` are total (`docs/manual/LANGUAGE.md` §6).
fn property_has_partial_operation(expr: &Expr) -> bool {
    let mut found = false;
    for_each_partial_operation_candidate(expr, &mut |operation| {
        found |= !matches!(
            operation,
            PartialOperation::Divide | PartialOperation::Remainder
        );
    });
    found
}

/// Whether some value a state variable of type `ty` can hold fails the
/// type-bound check (`value_conforms` in `fsl-runtime`). Only `Int`, `Bool`
/// and options of them conform whatever they hold: a `Map` can be assigned a
/// map over a narrower key range and a relation can gain an out-of-type pair
/// (`check` accepts both), and every engine reports the bound.
fn has_bounds(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int | TypeRef::Bool => false,
        TypeRef::Option(inner) => has_bounds(inner),
        TypeRef::Range(_, _)
        | TypeRef::Set(_)
        | TypeRef::Seq(_, _)
        | TypeRef::Map(_, _)
        | TypeRef::Relation(_, _)
        | TypeRef::Named(_) => true,
    }
}

/// The overflow and `Map`-key candidates found in a site, and the payload type
/// of each `is some(v)` pattern it evaluates (`None` when not bounded).
#[derive(Clone, Default)]
struct Found {
    overflow: bool,
    key: bool,
    patterns: BTreeMap<String, Option<TypeRef>>,
}

/// The names visible at one point of a site, each with a type that holds
/// every value it can have there. A name without one is left out, so an index
/// through it counts as a candidate.
#[derive(Clone)]
struct Scope<'m> {
    model: &'m KernelModel,
    env: TypeEnv,
}

impl<'m> Scope<'m> {
    fn new(model: &'m KernelModel) -> Self {
        Self {
            model,
            env: base_env(model),
        }
    }

    fn bind(&mut self, name: &str, ty: Option<TypeRef>) {
        match ty {
            Some(ty) => self.env.insert(name.to_owned(), ty),
            None => self.env.remove(name),
        };
    }

    /// The scope of a binder's `where` filter and body. A typed binder ranges
    /// over its type's domain and a range binder with literal bounds over
    /// that range; a collection binder over the members of its collection.
    fn enter(&self, binder: &Binder) -> Self {
        let ty = match binder {
            Binder::Collection { collection, .. } => match self.value_type(collection) {
                Some(TypeRef::Set(item) | TypeRef::Seq(item, _)) => resolve(self.model, &item).ok(),
                _ => None,
            },
            Binder::Typed { .. } | Binder::Range { .. } => {
                binder_type(binder, &self.env, self.model).ok()
            }
        };
        let mut scope = self.clone();
        scope.bind(binder_name(binder), ty);
        scope
    }

    /// A resolved type that holds every value `expr` can evaluate to, for the
    /// forms whose values are bounded by a declaration: a literal, a name in
    /// scope, an enum member, a field, a `Map` or `Seq` element (`m[i]`,
    /// `head`, `at`) and `old` of one, and a conditional whose branches join.
    /// `check` types other forms -- `s.add(e)`, `q.push(e)`, a struct
    /// literal, a conditional -- from one operand alone, so they get none.
    fn value_type(&self, expr: &Expr) -> Option<TypeRef> {
        let ty = match expr {
            Expr::Num(value) => TypeRef::Range(*value, *value),
            Expr::Bool(_) => TypeRef::Bool,
            Expr::Var(name) => self.env.get(name)?.clone(),
            Expr::EnumMember { type_name, .. } => TypeRef::Named(type_name.clone()),
            Expr::Field(base, field) => match self.value_type(base)? {
                TypeRef::Named(name) => struct_field_type(self.model, &name, field).ok()?,
                _ => return None,
            },
            Expr::Index(base, _) => match self.value_type(base)? {
                TypeRef::Map(_, item) | TypeRef::Seq(item, _) => *item,
                _ => return None,
            },
            Expr::Method { receiver, name, .. } if name == "head" || name == "at" => {
                match self.value_type(receiver)? {
                    TypeRef::Seq(item, _) => *item,
                    _ => return None,
                }
            }
            Expr::UnaryNamed { name, expr, .. } if name == "old" => return self.value_type(expr),
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => return join(self.value_type(then_expr), self.value_type(else_expr)),
            _ => return None,
        };
        resolve(self.model, &ty).ok()
    }

    /// Whether indexing a collection of type `collection` with `index` can
    /// miss a `Map`'s key domain. A `Seq` read is a partial operation, not a
    /// key-domain miss.
    fn key_may_miss(&self, collection: Option<TypeRef>, index: &Expr) -> bool {
        match collection {
            Some(TypeRef::Map(key, _)) => !self.within(index, &key),
            Some(TypeRef::Seq(_, _)) => false,
            _ => true,
        }
    }

    /// Whether every value of `index` lies inside `key`. A finite key type is
    /// a range, an enum, or `Bool` (`Map<Int, _>` is rejected by `check`).
    fn within(&self, index: &Expr, key: &TypeRef) -> bool {
        match (self.value_type(index), resolve(self.model, key).ok()) {
            (Some(TypeRef::Range(lo, hi)), Some(TypeRef::Range(key_lo, key_hi))) => {
                key_lo <= lo && hi <= key_hi
            }
            (Some(index), Some(key)) => index == key,
            _ => false,
        }
    }

    fn expr(&self, expr: &Expr, found: &mut Found) {
        recursion::guard(|| self.expr_inner(expr, found));
    }

    fn expr_inner(&self, expr: &Expr, found: &mut Found) {
        match expr {
            Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {}
            Expr::Neg(item) => {
                found.overflow = true;
                self.expr(item, found);
            }
            Expr::UnaryNamed {
                name, expr: item, ..
            } => {
                found.overflow |= name == "abs";
                self.expr(item, found);
            }
            Expr::Is {
                expr: item,
                pattern,
            } => {
                if let Pattern::Some(name) = pattern {
                    let payload = match self.value_type(item) {
                        Some(TypeRef::Option(inner)) => resolve(self.model, &inner).ok(),
                        _ => None,
                    };
                    found
                        .patterns
                        .entry(name.clone())
                        .and_modify(|ty| *ty = join(ty.take(), payload.clone()))
                        .or_insert(payload);
                }
                self.expr(item, found);
            }
            Expr::Some(item)
            | Expr::Not(item)
            | Expr::Field(item, _)
            | Expr::Stage { entity: item, .. } => self.expr(item, found),
            Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
                for item in items {
                    self.expr(item, found);
                }
            }
            Expr::Struct { fields, .. } => {
                for (_, item) in fields {
                    self.expr(item, found);
                }
            }
            Expr::Index(collection, index) => {
                found.key |= self.key_may_miss(self.value_type(collection), index);
                self.expr(collection, found);
                self.expr(index, found);
            }
            Expr::BinaryNamed { left, right, .. } => {
                self.expr(left, found);
                self.expr(right, found);
            }
            Expr::Binary { op, left, right } => {
                found.overflow |= matches!(op.as_str(), "+" | "-" | "*" | "/" | "%");
                self.expr(left, found);
                self.expr(right, found);
            }
            Expr::Method { receiver, args, .. } => {
                self.expr(receiver, found);
                for arg in args {
                    self.expr(arg, found);
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(condition, found);
                self.expr(then_expr, found);
                self.expr(else_expr, found);
            }
            Expr::Quantified { binder, body, .. } => {
                self.binder(binder, found);
                self.enter(binder).expr(body, found);
            }
            Expr::Aggregate {
                kind,
                binder,
                value,
            } => {
                found.overflow |= *kind == KernelAggregateKind::Sum;
                self.binder(binder, found);
                if let Some(value) = value {
                    self.enter(binder).expr(value, found);
                }
            }
            Expr::TernaryNamed {
                first,
                second,
                third,
                ..
            } => {
                self.expr(first, found);
                self.expr(second, found);
                self.expr(third, found);
            }
        }
    }

    /// A binder's range bounds and collection are evaluated in the outer
    /// scope; its `where` filter sees the bound variable.
    fn binder(&self, binder: &Binder, found: &mut Found) {
        let where_expr = match binder {
            Binder::Typed { where_expr, .. } => where_expr,
            Binder::Range {
                lo, hi, where_expr, ..
            } => {
                self.expr(lo, found);
                self.expr(hi, found);
                where_expr
            }
            Binder::Collection {
                collection,
                where_expr,
                ..
            } => {
                self.expr(collection, found);
                where_expr
            }
        };
        if let Some(where_expr) = where_expr {
            self.enter(binder).expr(where_expr, found);
        }
    }

    fn statement(&self, statement: &Statement, found: &mut Found) {
        match statement {
            Statement::Assign { target, value, .. } => {
                self.lvalue(target, found);
                self.expr(value, found);
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                self.expr(condition, found);
                for item in then_statements.iter().chain(else_statements) {
                    self.statement(item, found);
                }
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                self.binder(binder, found);
                let inner = self.enter(binder);
                for item in statements {
                    inner.statement(item, found);
                }
            }
        }
    }

    /// An indexed assignment target writes the state variable it names.
    fn lvalue(&self, target: &LValue, found: &mut Found) {
        match target {
            LValue::Var(_) => {}
            LValue::Index(name, index) => {
                let ty = self
                    .model
                    .state
                    .iter()
                    .find(|(var, _)| var == name)
                    .and_then(|(_, ty)| resolve(self.model, ty).ok());
                found.key |= self.key_may_miss(ty, index);
                self.expr(index, found);
            }
            LValue::Field(base, _) => self.lvalue(base, found),
        }
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}
