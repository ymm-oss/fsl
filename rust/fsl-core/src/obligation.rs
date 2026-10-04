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

use fsl_syntax::{Expr, Span};

use crate::partial_operation::{
    for_each_partial_operation_candidate, statement_has_partial_operation_candidate,
};
use crate::{
    ActionDef, ActionGuard, KernelModel, LeadsToDef, PartialOperation, PropertyDef, TypeDef,
    TypeRef, expression_has_partial_operation_candidate,
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
    /// Evaluating the site reaches none of the partial operations of
    /// `docs/manual/LANGUAGE.md` §6.
    PartialDefined,
    /// The `init` block admits an initial state.
    InitSatisfiable,
    /// Every reachable non-`terminal` state enables an action.
    NoDeadlock,
}

impl ObligationKind {
    /// Every obligation kind, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::Holds,
        Self::Witnessed,
        Self::Responds,
        Self::PartialDefined,
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
    push_invariants(&mut rows, &model.invariants);
    push_transitions(&mut rows, &model.transitions);
    push_reachables(&mut rows, &model.reachables);
    push_leadstos(&mut rows, &model.leadstos);
    push_terminal(&mut rows, model.terminal.as_ref());
    push_actions(&mut rows, &model.actions);
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

    fn definedness(&mut self, site: Site, has_partial_operation: bool) {
        self.push(ObligationKind::PartialDefined, site, !has_partial_operation);
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

fn push_invariants(rows: &mut Rows, invariants: &[PropertyDef]) {
    for property in invariants {
        let site = Site::Invariant(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Holds, &site);
        rows.definedness(site, property_has_partial_operation(&property.expr));
    }
}

fn push_transitions(rows: &mut Rows, transitions: &[PropertyDef]) {
    for property in transitions {
        let site = Site::Trans(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Holds, &site);
        rows.definedness(site, property_has_partial_operation(&property.expr));
    }
}

fn push_reachables(rows: &mut Rows, reachables: &[PropertyDef]) {
    for property in reachables {
        let site = Site::Reachable(SiteRef::new(&property.name, property.span));
        rows.holds(ObligationKind::Witnessed, &site);
        rows.definedness(site, property_has_partial_operation(&property.expr));
    }
}

fn push_leadstos(rows: &mut Rows, leadstos: &[LeadsToDef]) {
    for property in leadstos {
        let at = SiteRef::new(&property.name, property.span);
        rows.holds(ObligationKind::Responds, &Site::LeadsTo(at.clone()));
        rows.definedness(
            Site::Trigger(at.clone()),
            property_has_partial_operation(&property.before),
        );
        rows.definedness(
            Site::Goal(at),
            property_has_partial_operation(&property.after),
        );
    }
}

fn push_terminal(rows: &mut Rows, terminal: Option<&Expr>) {
    if let Some(terminal) = terminal {
        rows.definedness(Site::Terminal, property_has_partial_operation(terminal));
    }
}

fn push_actions(rows: &mut Rows, actions: &[ActionDef]) {
    for action in actions {
        let at = SiteRef::new(&action.name, action.span);
        let guard_partial = action.guards.iter().any(|guard| match guard {
            ActionGuard::Let(_, expr) | ActionGuard::Requires(expr) => {
                expression_has_partial_operation_candidate(expr)
            }
        });
        rows.definedness(Site::Guard(at.clone()), guard_partial);
        let body_partial = action
            .statements
            .iter()
            .any(statement_has_partial_operation_candidate);
        rows.definedness(Site::Body(at.clone()), body_partial);
        for (index, ensures) in action.ensures.iter().enumerate() {
            let site = Site::Ensures {
                action: at.clone(),
                index,
            };
            rows.holds(ObligationKind::Holds, &site);
            rows.definedness(site, expression_has_partial_operation_candidate(ensures));
        }
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
