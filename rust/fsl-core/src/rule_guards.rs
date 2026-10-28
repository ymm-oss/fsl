// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Whether the guards next-edition declarations add to an event can hold
//! together (design draft B.6), over the types' domains (decisions/048 J3)
//! with the one enumeration of [`crate::finite_domain`].
//!
//! - `rule_conflict_unsatisfiable_guard` (error): an event's guards (its
//!   decision tables' and rules', at the stage it leaves) hold for no value
//!   of the entity's fields and the event's inputs, and a rule takes part in
//!   a minimal conflicting subset. A decision table's guard that is empty on
//!   its own is the table's `decide_row_emptied`, not this.
//! - `decide_row_emptied` (warning, B.6): a decision-table row holds for some
//!   value, but for none that the rules guarding each event it names admit
//!   (a `never` that forbids the row's whole range).
//!
//! An event the enumeration defers becomes a [`GuardConflict`] for the
//! solver: the property "no value satisfies these guards", over the whole
//! state and the event's inputs.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fsl_syntax::{
    BusinessItem, DecisionTable, Expr, Param, ProcessField, ProcessItem, QualifiedName,
    RequirementsItem, Span, SurfaceRequirements, stage_reference,
};
use serde_json::{Value, json};

use crate::decision_table::DECIDE_ROW_EMPTIED;
use crate::finite_domain::{Assignment, Domains, domains, enumerate, evaluate, names};
use crate::{CoreError, CoreErrorKind, FslValue, KernelModel};

/// The guards of an event cannot all hold, over the types' domains, when the
/// event leaves its stage.
pub const RULE_CONFLICT_UNSATISFIABLE_GUARD: &str = "rule_conflict_unsatisfiable_guard";

/// One guard of an event, with every field spelled `Entity.field`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EventGuard {
    /// `decide`, `allow`, `never` or `after`.
    kind: &'static str,
    requirement: String,
    detail: String,
    condition: Expr,
}

impl EventGuard {
    fn label(&self) -> String {
        if self.kind == "decide" {
            format!("{} `decide {}`", self.requirement, self.detail)
        } else {
            format!("{} `{}`", self.requirement, self.detail)
        }
    }
}

/// The guards of one event and the names they range over.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EventGuards {
    event: String,
    entity: String,
    /// The stage the event leaves from.
    stage: String,
    span: Span,
    fields: Vec<ProcessField>,
    /// The entity's relation roles: fields whose state is an `Option`,
    /// which the enumeration leaves to the solver.
    roles: Vec<String>,
    /// The document's resources, read as themselves.
    resources: Vec<(String, QualifiedName)>,
    inputs: Vec<(String, QualifiedName)>,
    guards: Vec<EventGuard>,
}

/// The events of one requirements document with next-edition guards, and
/// its decision tables.
pub(crate) struct RuleGuards {
    events: Vec<EventGuards>,
    tables: Vec<DecisionTable>,
}

/// The answer of enumerating an event's guards.
enum Outcome {
    Satisfiable,
    /// The indexes of a minimal unsatisfiable subset.
    Unsatisfiable(Vec<usize>),
    Deferred(String),
}

/// `expression` with each bare field name of the entity spelled
/// `Entity.field`; inputs, which shadow fields of the same name, stay bare.
fn spell_fields(
    expression: Expr,
    fields: &BTreeSet<&str>,
    inputs: &BTreeSet<&str>,
    entity: &str,
) -> Expr {
    let recurse = |expression: Expr| spell_fields(expression, fields, inputs, entity);
    let boxed = |expression: Box<Expr>| Box::new(recurse(*expression));
    match expression {
        Expr::Var(name) if fields.contains(name.as_str()) && !inputs.contains(name.as_str()) => {
            Expr::Var(format!("{entity}.{name}"))
        }
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: boxed(left),
            right: boxed(right),
        },
        Expr::Not(value) => Expr::Not(boxed(value)),
        Expr::Neg(value) => Expr::Neg(boxed(value)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            condition: boxed(condition),
            then_expr: boxed(then_expr),
            else_expr: boxed(else_expr),
            spans,
        },
        other => other,
    }
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    }
}

impl EventGuards {
    fn stage_marker(&self) -> String {
        stage_reference(&self.entity)
    }

    /// The domains of the fields and inputs `conditions` read.
    fn domains(&self, model: &KernelModel, conditions: &[&Expr]) -> Result<Domains, String> {
        let prefix = format!("{}.", self.entity);
        let marker = self.stage_marker();
        let inputs = self
            .inputs
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<BTreeSet<_>>();
        let is_variable = |name: &str| {
            name.starts_with(&prefix)
                || inputs.contains(name)
                || name == marker
                || self.resources.iter().any(|(resource, _)| resource == name)
        };
        let mut read = BTreeSet::new();
        let mut constants = BTreeMap::new();
        for condition in conditions {
            names(condition, &is_variable, model, &mut read, &mut constants)?;
        }
        if let Some(role) = self
            .roles
            .iter()
            .find(|role| read.contains(&format!("{prefix}{role}")))
        {
            return Err(format!("it reads the relation '{role}'"));
        }
        let mut wanted = Vec::new();
        for field in &self.fields {
            let name = format!("{prefix}{}", field.name);
            if read.contains(&name) {
                wanted.push((name, &field.type_name, format!("field '{}'", field.name)));
            }
        }
        for (name, type_name) in &self.inputs {
            if read.contains(name) {
                wanted.push((name.clone(), type_name, format!("input '{name}'")));
            }
        }
        for (name, type_name) in &self.resources {
            if read.contains(name) {
                wanted.push((name.clone(), type_name, format!("resource '{name}'")));
            }
        }
        domains(model, &wanted, "fields and inputs")
    }

    /// Whether the guards at `subset` and `extra` hold together for some
    /// value.
    fn satisfiable(
        &self,
        model: &KernelModel,
        subset: &[usize],
        extra: &[&Expr],
    ) -> Result<bool, String> {
        let conditions = subset
            .iter()
            .map(|index| &self.guards[*index].condition)
            .chain(extra.iter().copied())
            .collect::<Vec<_>>();
        let domains = self.domains(model, &conditions)?;
        let stage = model
            .enum_members
            .get(&self.stage)
            .cloned()
            .ok_or_else(|| format!("stage '{}' has no member", self.stage))?;
        let fixed = Assignment::from([(self.stage_marker(), stage)]);
        let mut found = false;
        enumerate(&domains, &fixed, |assignment| {
            for condition in &conditions {
                match evaluate(condition, assignment, model)? {
                    FslValue::Bool(true) => {}
                    FslValue::Bool(false) => return Ok(true),
                    _ => return Err("a guard is not Boolean".to_owned()),
                }
            }
            found = true;
            Ok(false)
        })?;
        Ok(found)
    }

    /// Whether the guards can hold together, with a minimal conflicting
    /// subset when they cannot.
    fn outcome(&self, model: &KernelModel) -> Outcome {
        let mut kept = (0..self.guards.len()).collect::<Vec<_>>();
        match self.satisfiable(model, &kept, &[]) {
            Ok(true) => return Outcome::Satisfiable,
            Ok(false) => {}
            Err(reason) => return Outcome::Deferred(reason),
        }
        let mut index = 0;
        while index < kept.len() {
            let without = kept
                .iter()
                .enumerate()
                .filter(|(position, _)| *position != index)
                .map(|(_, guard)| *guard)
                .collect::<Vec<_>>();
            match self.satisfiable(model, &without, &[]) {
                Ok(true) => index += 1,
                Ok(false) => kept = without,
                Err(reason) => return Outcome::Deferred(reason),
            }
        }
        Outcome::Unsatisfiable(kept)
    }

    /// Whether a minimal conflicting subset is reported: it names a rule (a
    /// decision table's own empty guard is the table's warning).
    fn reported(&self, core: &[usize]) -> bool {
        core.iter()
            .any(|index| self.guards[*index].kind != "decide")
    }

    fn message(&self, core: &[usize]) -> String {
        let rules = core
            .iter()
            .map(|index| self.guards[*index].label())
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "event '{}' can never occur: when {} is {}, no value of its fields and inputs \
             satisfies the rules {rules}",
            self.event, self.entity, self.stage
        )
    }

    /// The indexes of the guards that come from rules.
    fn rule_guards(&self) -> Vec<usize> {
        self.guards
            .iter()
            .enumerate()
            .filter(|(_, guard)| guard.kind != "decide")
            .map(|(index, _)| index)
            .collect()
    }
}

fn event_guards(
    entity: &str,
    fields: &[ProcessField],
    next: &fsl_syntax::NextEditionRequirements,
    transition: &fsl_syntax::ProcessTransition,
    event: &fsl_syntax::EventForm,
) -> EventGuards {
    let roles = next
        .relations
        .iter()
        .filter(|relation| relation.entity == entity)
        .map(|relation| relation.role.clone())
        .collect::<Vec<_>>();
    let inputs = transition
        .inputs
        .iter()
        .filter_map(|input| match input {
            Param::Typed(name, type_name) => Some((name.clone(), type_name.clone())),
            Param::Range(..) => None,
        })
        .collect::<Vec<_>>();
    let field_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .chain(roles.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    let input_names = inputs
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    let guards = event
        .rule_guards
        .iter()
        .map(|guard| EventGuard {
            kind: guard.kind,
            requirement: guard.requirement.clone(),
            detail: guard.written.clone(),
            condition: spell_fields(guard.condition.clone(), &field_names, &input_names, entity),
        })
        .collect();
    EventGuards {
        event: transition.name.clone(),
        entity: entity.to_owned(),
        stage: transition.source.clone(),
        span: transition.span,
        fields: fields.to_vec(),
        roles,
        resources: next
            .resources
            .iter()
            .map(|resource| (resource.name.clone(), resource.type_name.clone()))
            .collect(),
        inputs,
        guards,
    }
}

impl RuleGuards {
    pub(crate) fn of(requirements: &SurfaceRequirements) -> Self {
        let mut events = Vec::new();
        let tables = requirements
            .next
            .as_ref()
            .map(|next| next.decisions.clone())
            .unwrap_or_default();
        let Some(next) = &requirements.next else {
            return Self { events, tables };
        };
        for item in &requirements.items {
            let RequirementsItem::Process(BusinessItem::Process {
                name,
                fields,
                items,
                ..
            }) = item
            else {
                continue;
            };
            let fields = fields
                .as_ref()
                .map(|fields| fields.fields.as_slice())
                .unwrap_or_default();
            for process_item in items {
                if let ProcessItem::Transition(transition) = process_item
                    && let Some(event) = &transition.event
                    && !event.rule_guards.is_empty()
                {
                    events.push(event_guards(name.name(), fields, next, transition, event));
                }
            }
        }
        Self { events, tables }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The enumerated checks, returning the `decide_row_emptied` warnings
    /// the rules cause.
    ///
    /// # Errors
    ///
    /// Returns [`RULE_CONFLICT_UNSATISFIABLE_GUARD`] for the first event
    /// whose guards cannot hold together.
    pub(crate) fn check(&self, model: &KernelModel) -> Result<Vec<Value>, CoreError> {
        for event in &self.events {
            if let Outcome::Unsatisfiable(core) = event.outcome(model)
                && event.reported(&core)
            {
                return Err(CoreError {
                    message: event.message(&core),
                    line: event.span.start.line,
                    column: event.span.start.column,
                    origin: None,
                    kind: CoreErrorKind::Semantics,
                    diagnostic_code: Some(RULE_CONFLICT_UNSATISFIABLE_GUARD),
                });
            }
        }
        Ok(self.emptied_rows(model))
    }

    /// The rows that hold for some value but for none the rules guarding
    /// every event they name admit.
    fn emptied_rows(&self, model: &KernelModel) -> Vec<Value> {
        let mut warnings = Vec::new();
        for table in &self.tables {
            for (index, row) in table.rows.iter().enumerate() {
                let guarded = row
                    .events
                    .iter()
                    .filter_map(|name| {
                        self.events
                            .iter()
                            .find(|event| event.entity == table.entity && event.event == *name)
                    })
                    .collect::<Vec<_>>();
                let Some(first) = guarded.first() else {
                    continue;
                };
                // A row empty on its own is the table's warning; a row one of
                // whose events no rule guards stays open for that event.
                if guarded.len() < row.events.len()
                    || guarded.iter().any(|event| event.rule_guards().is_empty())
                    || first.satisfiable(model, &[], &[&row.condition]) != Ok(true)
                {
                    continue;
                }
                let emptied = guarded.iter().all(|event| {
                    event.satisfiable(model, &event.rule_guards(), &[&row.condition]) == Ok(false)
                });
                if !emptied {
                    continue;
                }
                let rules = guarded
                    .iter()
                    .flat_map(|event| {
                        event
                            .rule_guards()
                            .into_iter()
                            .map(|guard| event.guards[guard].label())
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join("; ");
                warnings.push(json!({
                    "kind": DECIDE_ROW_EMPTIED,
                    "code": DECIDE_ROW_EMPTIED,
                    "severity": "warning",
                    "message": format!(
                        "decision table '{}' for {} {}: row {} `{}` holds for no value the rules \
                         {rules} admit, so its events never take this row",
                        table.id,
                        table.stage,
                        table.entity,
                        index + 1,
                        row.source
                    ),
                    "loc": row.span.python_loc(),
                }));
            }
        }
        warnings
    }

    /// The events with a rule's guard whose guards the enumeration
    /// deferred, for the solver.
    pub(crate) fn obligations(&self, model: &KernelModel) -> Vec<GuardConflict> {
        self.events
            .iter()
            .filter(|event| !event.rule_guards().is_empty())
            .filter_map(|event| match event.outcome(model) {
                Outcome::Deferred(reason) => Some(GuardConflict {
                    span: event.span,
                    guards: event.clone(),
                    reason,
                }),
                _ => None,
            })
            .collect()
    }
}

/// An event's guards the enumeration deferred: the solver decides for which
/// subsets [`Self::property`] (no value satisfies them) holds in every
/// state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardConflict {
    guards: EventGuards,
    /// Why the enumeration deferred the event.
    pub reason: String,
    pub span: Span,
}

impl GuardConflict {
    /// The number of guards.
    #[must_use]
    pub fn len(&self) -> usize {
        self.guards.guards.len()
    }

    /// Whether the event has no guard.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.guards.guards.is_empty()
    }

    /// Whether a minimal conflicting `subset` is reported: it names a rule.
    #[must_use]
    pub fn reported(&self, subset: &[usize]) -> bool {
        self.guards.reported(subset)
    }

    /// The diagnostic for a conflicting `subset`, as the enumeration words
    /// it.
    #[must_use]
    pub fn message(&self, subset: &[usize]) -> String {
        self.guards.message(subset)
    }

    /// The event's inputs as state variables, each with its type: the
    /// solver's state has to carry them, because [`Self::property`] reads an
    /// input as a variable rather than quantifying over it (the verifier
    /// expands a typed quantifier into one copy per value of the type, which
    /// is what the bound keeps the enumeration from doing).
    #[must_use]
    pub fn input_state(&self) -> Vec<(String, crate::TypeRef)> {
        self.guards
            .inputs
            .iter()
            .map(|(name, type_name)| (self.input_variable(name), type_ref(type_name)))
            .collect()
    }

    fn input_variable(&self, input: &str) -> String {
        format!("{} input {input}", self.guards.event)
    }

    /// `forall b: Entity { not (stage(b) == S and guards) }` over the
    /// kernel's state extended with [`Self::input_state`], for the guards at
    /// `subset`: it holds in every state exactly when no value of the fields
    /// and inputs satisfies them.
    #[must_use]
    pub fn property(&self, subset: &[usize]) -> Expr {
        let event = &self.guards;
        let binder = crate::naming::entity_binder(&event.entity);
        let read = |state: String| {
            Expr::Index(
                Box::new(Expr::Var(state)),
                Box::new(Expr::Var(binder.clone())),
            )
        };
        let mut replacements = event
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .chain(event.roles.iter().map(String::as_str))
            .map(|field| {
                (
                    format!("{}.{field}", event.entity),
                    read(crate::naming::process_field_state(&event.entity, field)),
                )
            })
            .collect::<HashMap<_, _>>();
        let stage = read(crate::naming::process_stage_state(&event.entity));
        replacements.insert(event.stage_marker(), stage.clone());
        for (name, _) in &event.inputs {
            replacements.insert(name.clone(), Expr::Var(self.input_variable(name)));
        }
        let conjunction = subset.iter().fold(
            binary("==", stage, Expr::Var(event.stage.clone())),
            |left, guard| {
                binary(
                    "and",
                    left,
                    crate::substitute_expr(event.guards[*guard].condition.clone(), &replacements),
                )
            },
        );
        let body = Expr::Not(Box::new(conjunction));
        quantified(
            &binder,
            QualifiedName {
                namespace: None,
                name: event.entity.clone(),
            },
            body,
        )
    }
}

/// The kernel type `type_name` names.
fn type_ref(type_name: &QualifiedName) -> crate::TypeRef {
    match (type_name.namespace.as_deref(), type_name.name.as_str()) {
        (None, "Bool") => crate::TypeRef::Bool,
        (None, "Int") => crate::TypeRef::Int,
        (None, name) => crate::TypeRef::Named(name.to_owned()),
        (Some(namespace), name) => crate::TypeRef::Named(format!("{namespace}.{name}")),
    }
}

fn quantified(name: &str, type_name: QualifiedName, body: Expr) -> Expr {
    Expr::Quantified {
        quantifier: "forall".to_owned(),
        binder: fsl_syntax::Binder::Typed {
            name: name.to_owned(),
            type_name,
            where_expr: None,
        },
        body: Box::new(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_syntax::{ConditionalSpans, SourcePos};

    fn var(name: &str) -> Expr {
        Expr::Var(name.to_owned())
    }

    fn spell(expression: Expr) -> Expr {
        spell_fields(
            expression,
            &BTreeSet::from(["amount", "limit", "ready"]),
            &BTreeSet::new(),
            "Claim",
        )
    }

    #[test]
    fn spell_fields_qualifies_only_bare_entity_fields() {
        assert_eq!(spell(var("amount")), var("Claim.amount"));
        for name in ["LIMIT", "input", "Claim.amount", "Other.amount"] {
            assert_eq!(spell(var(name)), var(name), "preserve {name}");
        }
    }

    #[test]
    fn spell_fields_recurses_into_both_binary_operands() {
        let expression = binary("+", var("amount"), binary("*", var("limit"), Expr::Num(2)));
        let expected = binary(
            "+",
            var("Claim.amount"),
            binary("*", var("Claim.limit"), Expr::Num(2)),
        );
        assert_eq!(spell(expression), expected);
    }

    #[test]
    fn spell_fields_recurses_into_not() {
        assert_eq!(
            spell(Expr::Not(Box::new(var("ready")))),
            Expr::Not(Box::new(var("Claim.ready"))),
        );
    }

    #[test]
    fn spell_fields_recurses_into_neg() {
        assert_eq!(
            spell(Expr::Neg(Box::new(var("amount")))),
            Expr::Neg(Box::new(var("Claim.amount"))),
        );
    }

    #[test]
    fn spell_fields_recurses_into_every_conditional_branch_and_preserves_spans() {
        let span = |offset| {
            let position = SourcePos {
                offset,
                line: 1,
                column: u32::try_from(offset + 1).unwrap(),
            };
            Span {
                start: position,
                end: position,
            }
        };
        let spans = Box::new(ConditionalSpans {
            condition: span(1),
            then_expr: span(5),
            else_expr: span(9),
        });
        let expression = Expr::Conditional {
            condition: Box::new(var("ready")),
            then_expr: Box::new(var("amount")),
            else_expr: Box::new(var("limit")),
            spans: spans.clone(),
        };
        let expected = Expr::Conditional {
            condition: Box::new(var("Claim.ready")),
            then_expr: Box::new(var("Claim.amount")),
            else_expr: Box::new(var("Claim.limit")),
            spans,
        };
        assert_eq!(spell(expression), expected);
    }

    #[test]
    fn spell_fields_inputs_shadow_fields_without_hiding_other_fields() {
        let expression = binary("+", var("amount"), var("limit"));
        assert_eq!(
            spell_fields(
                expression,
                &BTreeSet::from(["amount", "limit"]),
                &BTreeSet::from(["amount"]),
                "Claim",
            ),
            binary("+", var("amount"), var("Claim.limit")),
        );
    }
}
