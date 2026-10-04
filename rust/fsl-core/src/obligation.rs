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

use fsl_syntax::{Binder, Expr, LValue, Span, Statement};

use crate::partial_operation::{
    for_each_partial_operation_candidate, statement_has_partial_operation_candidate,
};
use crate::typecheck::{
    TypeEnv, base_env, binder_type, extend_pattern_binding, infer_type, resolve,
};
use crate::{
    ActionDef, ActionGuard, KernelAggregateKind, KernelModel, LeadsToDef, ParamDef,
    PartialOperation, PropertyDef, TypeDef, TypeRef, expression_has_partial_operation_candidate,
    recursion,
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
    /// No evaluation of the site can fail this obligation, by a syntactic
    /// over-approximation: an engine that asks nothing here skips no
    /// question. Only type-bound and definedness rows can be vacuous.
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
    fn definedness(&mut self, site: &Site, has_partial_operation: bool, found: Found) {
        self.push(
            ObligationKind::PartialDefined,
            site.clone(),
            !has_partial_operation,
        );
        self.push(ObligationKind::NoOverflow, site.clone(), !found.overflow);
        self.push(ObligationKind::KeyInDomain, site.clone(), !found.key);
    }

    /// The definedness rows of a property-context expression.
    fn property_definedness(&mut self, site: &Site, scope: &Scope<'_>, expr: &Expr) {
        let mut found = Found::default();
        scope.expr(expr, &mut found);
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
        rows.push(ObligationKind::Holds, site, !has_bounds(model, ty));
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
    let scope = Scope::new(model);
    for property in properties {
        let site = site(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Holds, &site);
        rows.property_definedness(&site, &scope, &property.expr);
    }
}

fn push_reachables(rows: &mut Rows, model: &KernelModel) {
    let scope = Scope::new(model);
    for property in &model.reachables {
        let site = Site::Reachable(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Witnessed, &site);
        rows.property_definedness(&site, &scope, &property.expr);
    }
}

fn push_leadstos(rows: &mut Rows, model: &KernelModel) {
    for property in &model.leadstos {
        let at = SiteRef::new(&property.name, property.span);
        let scope = Scope::leadsto(model, property);
        let leads_to = Site::LeadsTo(at.clone());
        rows.holds(ObligationKind::Responds, &leads_to);
        if property.within.is_some() {
            rows.holds(ObligationKind::Deadline, &leads_to);
        }
        rows.property_definedness(&Site::Trigger(at.clone()), &scope, &property.before);
        rows.property_definedness(&Site::Goal(at), &scope, &property.after);
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
        let scope = Scope::leadsto(model, property);
        let mut found = Found::default();
        scope.expr(measure, &mut found);
        rows.definedness(
            &Site::Measure(at.clone()),
            expression_has_partial_operation_candidate(measure),
            found,
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
        rows.property_definedness(&Site::Terminal, &Scope::new(model), terminal);
    }
}

fn push_actions(rows: &mut Rows, model: &KernelModel) {
    for action in &model.actions {
        push_action(rows, model, action);
    }
}

/// An action's sites share one scope: parameters, then each `let` and each
/// `requires` pattern binding in clause order, as the evaluators bind them.
fn push_action(rows: &mut Rows, model: &KernelModel, action: &ActionDef) {
    let at = SiteRef::new(&action.name, action.span);
    let mut scope = Scope::new(model);
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
            ActionGuard::Requires(expr) => {
                scope.expr(expr, &mut guard);
                scope.bind_pattern(expr);
            }
            ActionGuard::Let(name, expr) => {
                scope.expr(expr, &mut guard);
                scope.bind(name, infer_type(expr, &scope.env, model, None).ok());
            }
        }
    }
    let guard_partial = action.guards.iter().any(|clause| match clause {
        ActionGuard::Let(_, expr) | ActionGuard::Requires(expr) => {
            expression_has_partial_operation_candidate(expr)
        }
    });
    rows.definedness(&Site::Guard(at.clone()), guard_partial, guard);
    let mut body = Found::default();
    for statement in &action.statements {
        scope.statement(statement, &mut body);
    }
    let body_partial = action
        .statements
        .iter()
        .any(statement_has_partial_operation_candidate);
    rows.definedness(&Site::Body(at.clone()), body_partial, body);
    for (index, ensures) in action.ensures.iter().enumerate() {
        let site = Site::Ensures {
            action: at.clone(),
            index,
        };
        rows.holds(ObligationKind::Holds, &site);
        let mut found = Found::default();
        scope.expr(ensures, &mut found);
        rows.definedness(
            &site,
            expression_has_partial_operation_candidate(ensures),
            found,
        );
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

/// Whether a value of `ty` can fall outside its declared type. The verifier's
/// `fsl-verifier/src/induction.rs` asks a type-bound obligation for exactly
/// these state variables.
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

/// The overflow and `Map`-key candidates found in a site.
#[derive(Clone, Copy, Default)]
struct Found {
    overflow: bool,
    key: bool,
}

/// The static types visible at one point of a site. A name whose type cannot
/// be inferred is left out, so an index through it counts as a candidate.
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

    /// The scope of a `leadsTo`'s trigger, goal and measure: its binders.
    fn leadsto(model: &'m KernelModel, property: &LeadsToDef) -> Self {
        let mut scope = Self::new(model);
        for binder in &property.binders {
            scope = scope.enter(binder);
        }
        scope
    }

    fn bind(&mut self, name: &str, ty: Option<TypeRef>) {
        match ty {
            Some(ty) => self.env.insert(name.to_owned(), ty),
            None => self.env.remove(name),
        };
    }

    fn enter(&self, binder: &Binder) -> Self {
        let mut scope = self.clone();
        scope.bind(
            binder_name(binder),
            binder_type(binder, &self.env, self.model).ok(),
        );
        scope
    }

    /// Add `expr`'s `is some(v)` bindings. When they cannot be typed nothing
    /// in scope keeps a type, so no index is mistaken for an in-domain one.
    fn bind_pattern(&mut self, expr: &Expr) {
        if extend_pattern_binding(expr, &mut self.env, self.model).is_err() {
            self.env.clear();
        }
    }

    /// Whether indexing a collection of type `collection` with `index` can
    /// miss a `Map`'s key domain. A `Seq` read is a partial operation, not a
    /// key-domain miss.
    fn key_may_miss(&self, collection: Option<TypeRef>, index: &Expr) -> bool {
        match collection.and_then(|ty| resolve(self.model, &ty).ok()) {
            Some(TypeRef::Map(key, _)) => !self.within(index, &key),
            Some(TypeRef::Seq(_, _)) => false,
            _ => true,
        }
    }

    /// Whether `index`'s static type, or its value for a literal, lies inside
    /// `key`. A finite key type is a range, an enum, or `Bool` (`Map<Int, _>`
    /// is rejected by `check`).
    fn within(&self, index: &Expr, key: &TypeRef) -> bool {
        let index = match index {
            Expr::Num(value) => Some(TypeRef::Range(*value, *value)),
            _ => infer_type(index, &self.env, self.model, None).ok(),
        };
        match (
            index.and_then(|ty| resolve(self.model, &ty).ok()),
            resolve(self.model, key).ok(),
        ) {
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
            Expr::Some(item)
            | Expr::Not(item)
            | Expr::Field(item, _)
            | Expr::Stage { entity: item, .. }
            | Expr::Is { expr: item, .. } => self.expr(item, found),
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
                let ty = infer_type(collection, &self.env, self.model, None).ok();
                found.key |= self.key_may_miss(ty, index);
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
                let mut right_scope = self.clone();
                right_scope.bind_pattern(left);
                right_scope.expr(right, found);
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
                    .map(|(_, ty)| ty.clone());
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
