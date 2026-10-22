// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The next-edition decision table `decide ID "text" for Stage Entity { rows
//! [otherwise => outcome] } [assuming Q]` (fsl 5 plan §3 S7, design B.3–B.5).
//!
//! A row is `Concept {and Concept} => Actor verb {or verb} {covers ID "text"}`
//! (or `Stage Entity => …`). Every event that leaves the table's stage must be
//! named by a row (`decide_event_not_listed`), and each such event gets, as a
//! guard (one `decide` entry of the event's `rule_guards`, so one `requires`
//! line per table), the disjunction of the conditions of the rows that name it;
//! `otherwise` is the conjunction of the other rows' negations. The concepts
//! are expanded here, so the condition reads the entity's fields as
//! `Entity.field` names (which an event input cannot shadow) and a concept's
//! stages are already decided against the table's stage. Whether the rows
//! cover every value and never overlap is checked over the fields' finite
//! domains by fsl-core, which owns the types ([`DecisionTable`] carries what
//! it needs).

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Entity, EventSignature, MIXED_PARTICLES, PARTICLE_ROLE_MISMATCH, UNKNOWN_EVENT_PHRASE, Word,
    actor_not_declared, join_names,
};
use crate::keywords::{Kw, Particle};
use crate::parser::{Parser, join_span};
use crate::{
    Annotation, ConceptDeclaration, DecisionRow, DecisionTable, Expr, ParseError, ProcessCover,
    ProcessItem, RuleGuard, Span, TokenKind,
};

/// An event that leaves a decision table's stage but no row names.
pub const DECIDE_EVENT_NOT_LISTED: &str = "decide_event_not_listed";

/// `decide …`, as read.
pub(in crate::parser) struct DecideDeclaration {
    id: String,
    text: String,
    stage: Word,
    entity: Word,
    rows: Vec<RowSource>,
    assuming: Option<Expr>,
    span: Span,
}

enum Condition {
    /// `Concept {and Concept}`.
    Concepts(Vec<Word>),
    /// `Stage Entity`.
    Stage(Word, Word),
    /// `otherwise`.
    Otherwise,
}

struct RowSource {
    condition: Condition,
    actor: Word,
    verbs: Vec<Word>,
    /// Whether the outcome marks its actor with が.
    particles: bool,
    covers: Vec<ProcessCover>,
    source: String,
    span: Span,
}

impl Parser<'_> {
    /// `decide ID "text" for Stage Entity { row … [otherwise => outcome] }
    /// [assuming Q]`.
    pub(super) fn decide_declaration(&mut self) -> Result<DecideDeclaration, ParseError> {
        let start = self.bump().span;
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        self.expect_kw(Kw::DecideFor)?;
        let stage = self.word()?;
        let entity = self.word()?;
        self.expect_symbol("{")?;
        let mut rows = Vec::new();
        while !self.eat_symbol("}") {
            if rows
                .last()
                .is_some_and(|row: &RowSource| matches!(row.condition, Condition::Otherwise))
            {
                return Err(self.error("`otherwise` is the last row of a decision table"));
            }
            rows.push(self.decide_row()?);
        }
        if rows.is_empty() {
            return Err(ParseError::new(
                format!("decision table '{id}' has no rows"),
                start,
            ));
        }
        let assuming = if self.eat_kw(Kw::Assuming) {
            Some(self.expression(0)?)
        } else {
            None
        };
        Ok(DecideDeclaration {
            id,
            text,
            stage,
            entity,
            rows,
            assuming,
            span: join_span(start, text_span),
        })
    }

    /// `cexpr => Actor verb {or verb} {covers ID "text"}`, or the same after
    /// `otherwise`.
    fn decide_row(&mut self) -> Result<RowSource, ParseError> {
        let start = self.peek().span;
        let condition = if self.eat_kw(Kw::Otherwise) {
            Condition::Otherwise
        } else {
            let first = self.word()?;
            if self.peek_kw(Kw::And) {
                let mut concepts = vec![first];
                while self.eat_kw(Kw::And) {
                    concepts.push(self.word()?);
                }
                Condition::Concepts(concepts)
            } else if matches!(self.peek().kind, TokenKind::Ident(_)) {
                Condition::Stage(first, self.word()?)
            } else {
                Condition::Concepts(vec![first])
            }
        };
        self.expect_symbol("=>")?;
        let particles = self.phrase_has_particles(&[Kw::Covers]);
        let (actor, verbs) = if particles {
            self.marked_row_outcome()?
        } else {
            let actor = self.word()?;
            let mut verbs = vec![self.word()?];
            while self.eat_kw(Kw::RowOr) {
                verbs.push(self.word()?);
            }
            (actor, verbs)
        };
        let end = self.last_span();
        let source = self.source[start.start.offset..end.end.offset]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut covers = Vec::new();
        while self.peek_kw(Kw::Covers) {
            let cover_start = self.bump().span;
            let id = self.req_id()?;
            let (text, text_span) = self.expect_string_with_span()?;
            covers.push(ProcessCover {
                id,
                text,
                span: join_span(cover_start, text_span),
            });
        }
        Ok(RowSource {
            condition,
            actor,
            verbs,
            particles,
            covers,
            source,
            span: join_span(start, end),
        })
    }

    /// A row's outcome written with particles, on the row's line: the actor
    /// marked が and the verbs, without particles, joined by `or`. The table
    /// names the entity, so no word takes を.
    fn marked_row_outcome(&mut self) -> Result<(Word, Vec<Word>), ParseError> {
        let start = self.peek().span;
        let mut actor: Option<Word> = None;
        let mut verbs = Vec::new();
        while matches!(self.peek().kind, TokenKind::Ident(_))
            && self.peek().span.start.line == start.start.line
            && !self.peek_kw(Kw::Covers)
        {
            if !verbs.is_empty() && self.eat_kw(Kw::RowOr) {
                continue;
            }
            let word = self.word()?;
            match self.particle_here() {
                Some(Particle::Ga) if actor.is_none() => {
                    self.bump();
                    actor = Some(word);
                }
                Some(particle) => {
                    return Err(ParseError::coded(
                        PARTICLE_ROLE_MISMATCH,
                        format!(
                            "'{}' takes {} ({}), but a decision table row names only its actor \
                             (が) and verbs; the table names the entity",
                            word.text,
                            particle.spelling(),
                            particle.role()
                        ),
                        word.span,
                    ));
                }
                None => verbs.push(word),
            }
        }
        let Some(actor) = actor else {
            return Err(ParseError::coded(
                MIXED_PARTICLES,
                "this row's outcome marks words with particles but not its actor; write \
                 `<actor> が <verb>`",
                start,
            ));
        };
        if verbs.is_empty() {
            return Err(ParseError::new("this row's outcome names no verb", start));
        }
        Ok((actor, verbs))
    }
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// The conjunction of `terms`, with `true` dropped and `false` absorbing.
fn conjunction(terms: impl IntoIterator<Item = Expr>) -> Expr {
    let mut kept = Vec::new();
    for term in terms {
        match term {
            Expr::Bool(true) => {}
            Expr::Bool(false) => return Expr::Bool(false),
            term => kept.push(term),
        }
    }
    kept.into_iter()
        .reduce(|left, right| binary("and", left, right))
        .unwrap_or(Expr::Bool(true))
}

/// The disjunction of `terms`, with `false` dropped and `true` absorbing.
fn disjunction(terms: impl IntoIterator<Item = Expr>) -> Expr {
    let mut kept = Vec::new();
    for term in terms {
        match term {
            Expr::Bool(false) => {}
            Expr::Bool(true) => return Expr::Bool(true),
            term => kept.push(term),
        }
    }
    kept.into_iter()
        .reduce(|left, right| binary("or", left, right))
        .unwrap_or(Expr::Bool(false))
}

fn negation(term: Expr) -> Expr {
    match term {
        Expr::Bool(value) => Expr::Bool(!value),
        term => Expr::Not(Box::new(term)),
    }
}

/// What the names of a table's conditions resolve against.
struct Scope<'a> {
    table: &'a str,
    entity_name: &'a str,
    entity: &'a Entity,
    stage: &'a str,
    concepts: &'a [ConceptDeclaration],
}

impl Scope<'_> {
    /// `concept`'s predicate at the table's stage.
    fn concept(&self, word: &Word) -> Result<Expr, ParseError> {
        let Some(concept) = self
            .concepts
            .iter()
            .find(|concept| concept.name == word.text)
        else {
            return Err(ParseError::new(
                format!(
                    "decision table '{}': '{}' is not a concept; declared concepts: {}",
                    self.table,
                    word.text,
                    if self.concepts.is_empty() {
                        "none".to_owned()
                    } else {
                        join_names(self.concepts.iter().map(|concept| concept.name.as_str()))
                    }
                ),
                word.span,
            ));
        };
        if concept.entity != self.entity_name {
            return Err(ParseError::new(
                format!(
                    "decision table '{}' is for '{}', but concept '{}' is of '{}'",
                    self.table, self.entity_name, concept.name, concept.entity
                ),
                word.span,
            ));
        }
        if !concept.stages.is_empty() && !concept.stages.iter().any(|stage| stage == self.stage) {
            return Ok(Expr::Bool(false));
        }
        concept
            .condition
            .clone()
            .map_or(Ok(Expr::Bool(true)), |condition| {
                self.fields(condition, word.span, false)
            })
    }

    /// `expression` with the entity's fields spelled `Entity.field`, and,
    /// when `concepts` is set, a concept's name replaced by its predicate.
    fn fields(&self, expression: Expr, span: Span, concepts: bool) -> Result<Expr, ParseError> {
        let recurse = |expression: Expr| self.fields(expression, span, concepts);
        let boxed = |expression: Box<Expr>| recurse(*expression).map(Box::new);
        let dotted = |field: &str| Expr::Var(format!("{}.{field}", self.entity_name));
        Ok(match expression {
            Expr::Var(name) if self.entity.fields.contains_key(&name) => dotted(&name),
            Expr::Var(name)
                if concepts && self.concepts.iter().any(|concept| concept.name == name) =>
            {
                self.concept(&Word { text: name, span })?
            }
            Expr::EnumMember { type_name, member }
                if type_name == self.entity_name && self.entity.fields.contains_key(&member) =>
            {
                dotted(&member)
            }
            Expr::Field(receiver, field)
                if matches!(receiver.as_ref(), Expr::Var(name) if name == self.entity_name)
                    && self.entity.fields.contains_key(&field) =>
            {
                dotted(&field)
            }
            Expr::Field(receiver, field) => Expr::Field(boxed(receiver)?, field),
            Expr::Some(value) => Expr::Some(boxed(value)?),
            Expr::Set(values) => {
                Expr::Set(values.into_iter().map(recurse).collect::<Result<_, _>>()?)
            }
            Expr::Seq(values) => {
                Expr::Seq(values.into_iter().map(recurse).collect::<Result<_, _>>()?)
            }
            Expr::Index(map, key) => Expr::Index(boxed(map)?, boxed(key)?),
            Expr::Binary { op, left, right } => Expr::Binary {
                op,
                left: boxed(left)?,
                right: boxed(right)?,
            },
            Expr::Neg(value) => Expr::Neg(boxed(value)?),
            Expr::Not(value) => Expr::Not(boxed(value)?),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                spans,
            } => Expr::Conditional {
                condition: boxed(condition)?,
                then_expr: boxed(then_expr)?,
                else_expr: boxed(else_expr)?,
                spans,
            },
            Expr::Call { name, args, span } => Expr::Call {
                name,
                args: args.into_iter().map(recurse).collect::<Result<_, _>>()?,
                span,
            },
            Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
                name,
                expr: boxed(expr)?,
                span,
            },
            Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
                name,
                left: boxed(left)?,
                right: boxed(right)?,
            },
            other => other,
        })
    }

    /// A row's condition at the table's stage.
    fn condition(&self, condition: &Condition) -> Result<Expr, ParseError> {
        match condition {
            Condition::Concepts(concepts) => Ok(conjunction(
                concepts
                    .iter()
                    .map(|concept| self.concept(concept))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Condition::Stage(stage, entity) => {
                if entity.text != self.entity_name {
                    return Err(ParseError::new(
                        format!(
                            "decision table '{}' is for '{}', but the row names '{}'",
                            self.table, self.entity_name, entity.text
                        ),
                        entity.span,
                    ));
                }
                if !self.entity.has_stage(&stage.text) {
                    return Err(ParseError::new(
                        format!("'{}' is not a stage of '{}'", stage.text, self.entity_name),
                        stage.span,
                    ));
                }
                Ok(Expr::Bool(stage.text == self.stage))
            }
            Condition::Otherwise => unreachable!("otherwise is resolved from the other rows"),
        }
    }
}

/// Fold each decision table into the guards of the events it names, and
/// return the tables for fsl-core's checks.
///
/// # Errors
///
/// Returns [`DECIDE_EVENT_NOT_LISTED`], `unknown_event_phrase`,
/// `actor_not_declared`, and plain errors for undeclared names.
#[allow(clippy::too_many_lines)]
pub(super) fn apply(
    decisions: Vec<DecideDeclaration>,
    transitions: &mut BTreeMap<String, Vec<ProcessItem>>,
    events: &[EventSignature],
    entities: &BTreeMap<String, Entity>,
    concepts: &[ConceptDeclaration],
    actors: &BTreeSet<&str>,
) -> Result<Vec<DecisionTable>, ParseError> {
    let mut tables = Vec::new();
    for decision in decisions {
        let Some(entity) = entities.get(&decision.entity.text) else {
            return Err(ParseError::new(
                format!(
                    "decision table '{}' names '{}', which is not an entity with a lifecycle",
                    decision.id, decision.entity.text
                ),
                decision.entity.span,
            ));
        };
        if !entity.has_stage(&decision.stage.text) {
            return Err(ParseError::new(
                format!(
                    "decision table '{}': '{}' is not a stage of '{}'",
                    decision.id, decision.stage.text, decision.entity.text
                ),
                decision.stage.span,
            ));
        }
        let scope = Scope {
            table: &decision.id,
            entity_name: &decision.entity.text,
            entity,
            stage: &decision.stage.text,
            concepts,
        };
        let leaving = events
            .iter()
            .filter(|event| {
                event.entity == decision.entity.text
                    && event.from == decision.stage.text
                    && event.to != decision.stage.text
            })
            .collect::<Vec<_>>();
        let mut rows: Vec<DecisionRow> = Vec::new();
        let mut row_covers = Vec::new();
        for row in decision.rows {
            if !actors.contains(row.actor.text.as_str()) {
                if row.particles
                    && let Some(verb) = row
                        .verbs
                        .iter()
                        .find(|verb| actors.contains(verb.text.as_str()))
                {
                    return Err(ParseError::coded(
                        PARTICLE_ROLE_MISMATCH,
                        format!(
                            "'{}' takes が (actor), but '{}' is the actor; write `{} が`",
                            row.actor.text, verb.text, verb.text
                        ),
                        row.actor.span,
                    ));
                }
                return Err(actor_not_declared(&row.actor, actors));
            }
            let mut named = Vec::new();
            for verb in &row.verbs {
                let matching = leaving
                    .iter()
                    .filter(|event| event.actor == row.actor.text && event.verb == verb.text)
                    .map(|event| event.name.clone())
                    .collect::<Vec<_>>();
                if matching.is_empty() {
                    let phrases = leaving
                        .iter()
                        .map(|event| format!("{} {}", event.actor, event.verb))
                        .collect::<BTreeSet<_>>();
                    return Err(ParseError::coded(
                        UNKNOWN_EVENT_PHRASE,
                        format!(
                            "decision table '{}': no event of '{}' leaving '{}' matches '{} {}'; \
                             the events leaving it are {}",
                            decision.id,
                            decision.entity.text,
                            decision.stage.text,
                            row.actor.text,
                            verb.text,
                            if phrases.is_empty() {
                                "none".to_owned()
                            } else {
                                join_names(phrases.iter().map(String::as_str))
                            }
                        ),
                        verb.span,
                    ));
                }
                for name in matching {
                    if !named.contains(&name) {
                        named.push(name);
                    }
                }
            }
            let condition = if matches!(row.condition, Condition::Otherwise) {
                conjunction(rows.iter().map(|other| negation(other.condition.clone())))
            } else {
                scope.condition(&row.condition)?
            };
            row_covers.push(row.covers);
            rows.push(DecisionRow {
                source: row.source,
                condition,
                events: named,
                otherwise: matches!(row.condition, Condition::Otherwise),
                span: row.span,
            });
        }
        let listed = rows
            .iter()
            .flat_map(|row| row.events.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let unlisted = leaving
            .iter()
            .filter(|event| !listed.contains(event.name.as_str()))
            .map(|event| format!("{} ({} {})", event.name, event.actor, event.verb))
            .collect::<Vec<_>>();
        if !unlisted.is_empty() {
            return Err(ParseError::coded(
                DECIDE_EVENT_NOT_LISTED,
                format!(
                    "decision table '{}' for {} {} lists no row for {}; every event leaving \
                     '{}' needs a row (or `otherwise`)",
                    decision.id,
                    decision.stage.text,
                    decision.entity.text,
                    join_names(unlisted.iter().map(String::as_str)),
                    decision.stage.text
                ),
                decision.span,
            ));
        }
        let assuming = decision
            .assuming
            .map(|assuming| scope.fields(assuming, decision.span, true))
            .transpose()?;

        let entity_transitions = transitions
            .get_mut(&decision.entity.text)
            .expect("an event leaving the stage has its transitions");
        for event in &leaving {
            let guard = disjunction(
                rows.iter()
                    .filter(|row| row.events.contains(&event.name))
                    .map(|row| row.condition.clone()),
            );
            let Some(ProcessItem::Transition(transition)) =
                entity_transitions.iter_mut().find(|item| {
                    matches!(item, ProcessItem::Transition(transition) if transition.name == event.name)
                })
            else {
                unreachable!("every event signature has its transition");
            };
            if guard != Expr::Bool(true) {
                let detail = rows
                    .iter()
                    .filter(|row| row.events.contains(&event.name))
                    .map(|row| row.source.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ");
                transition
                    .event
                    .as_mut()
                    .expect("a decision table guards an event")
                    .rule_guards
                    .push(RuleGuard {
                        kind: "decide",
                        requirement: decision.id.clone(),
                        condition: guard,
                        detail: format!(
                            "for {} {}: {detail}",
                            decision.stage.text, decision.entity.text
                        ),
                        span: decision.span,
                    });
            }
            transition.annotations.push(Annotation::Requirement {
                id: decision.id.clone(),
                text: Some(decision.text.clone()),
                span: decision.span,
            });
            for (row, covers) in rows.iter().zip(&row_covers) {
                if row.events.contains(&event.name) {
                    for cover in covers {
                        transition.annotations.push(Annotation::Requirement {
                            id: cover.id.clone(),
                            text: Some(cover.text.clone()),
                            span: cover.span,
                        });
                    }
                }
            }
        }
        tables.push(DecisionTable {
            id: decision.id,
            text: decision.text,
            entity: decision.entity.text,
            stage: decision.stage.text,
            rows,
            assuming,
            span: decision.span,
        });
    }
    Ok(tables)
}
