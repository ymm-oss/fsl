// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The next-edition `rule REQ "text" { … }` member (fsl 5 plan §3 S6).
//!
//! A rule's statements are a closed set (design draft B.4): `allow P only
//! when X | for C`, `never P when X | for C`, `P only after Q`,
//! `E enters T only from S…`, `always C1 is C2`,
//! `always E [not] in S… has X` and `deadline name <= bound`. Each lowers to
//! a guard conjoined to the events its phrase names ([`RuleGuard`]), to a
//! property ([`RuleProperty`]), or, for `deadline`, to the requirement
//! block's own `deadline`. Guards are conjunctions, so the order the rules
//! are written in does not matter (B.6).
//!
//! A statement that lowers to a property can be named, `Name: always …` or
//! `Name: E enters T only from S…`; the name becomes the Kernel property's,
//! and an unnamed one gets a name the core generates.
//!
//! `P only after Q` adds a ghost field `did_<events of Q>` to the entity,
//! set by Q's events and required by P's, unless the lifecycle already
//! orders the two (`implied_by_lifecycle`).

use std::collections::{BTreeMap, BTreeSet};

use super::super::{Parser, join_span};
use super::{
    Entity, EventSignature, MIXED_PARTICLES, PARTICLE_ROLE_MISMATCH, Roles, UNKNOWN_EVENT_PHRASE,
    UNKNOWN_VERB, Word, actor_not_declared, check_marked_roles, is_symbol, join_names, nearest,
    resolve_event_names,
};
use crate::keywords::{KeywordContext, Kw, Particle, english_of};
use crate::{
    Annotation, ConceptDeclaration, EditionWarning, Expr, NextEditionRequirements, ParseError,
    ProcessField, ProcessItem, QualifiedName, RequirementBlockItem, RequirementsItem, RuleGuard,
    RuleProperty, Span, TokenKind, stage_reference,
};

/// Info: an ordering or entry rule the lifecycle already implies.
pub const IMPLIED_BY_LIFECYCLE: &str = "implied_by_lifecycle";
/// Warning: a requirement with `allow` / `never` rules that no forbidden
/// trace probes (#1300: a rejection rule is a checked guard plus a forbidden
/// trace).
pub const NEVER_RULE_WITHOUT_FORBIDDEN: &str = "never_rule_without_forbidden";
/// A name written before a rule statement that no property takes, or that
/// is not a valid property name.
pub const INVALID_RULE_PROPERTY_NAME: &str = "invalid_rule_property_name";
/// A ghost field an ordering rule adds is already a field of the entity.
/// Same code as the core's check of the names an entity generates.
pub const GENERATED_NAME_COLLISION: &str = "generated_name_collision";

/// `Actor verb Entity` or `Actor Entity verb`, as read; with particles
/// (`Actor が Entity を verb`, any order) `words` is `[verb, entity]`.
struct Phrase {
    actor: Word,
    words: [Word; 2],
    particles: bool,
    /// From the phrase's first word to its last.
    span: Span,
}

/// One conjunct of a concept expression: a concept, or `Stage Entity`.
pub(super) enum Conjunct {
    Concept(Word),
    Stage { stage: Word, entity: Word },
}

enum Condition {
    /// The expression, and its text in canonical names and English keywords
    /// ([`Parser::canonical_text`]).
    When(Expr, String),
    For(Vec<Conjunct>),
}

enum RuleStatement {
    Allow(Phrase, Condition),
    Never(Phrase, Condition),
    After(Phrase, Phrase),
    Enters {
        entity: Word,
        target: Word,
        sources: Vec<Word>,
    },
    Always(Vec<Conjunct>, Vec<Conjunct>),
    AlwaysHas {
        entity: Word,
        negated: bool,
        stages: Vec<Word>,
        has: Expr,
        /// `has`'s text, as [`Condition::When`]'s.
        has_text: String,
    },
    Deadline {
        name: String,
        bound: Expr,
    },
}

struct Located {
    /// The property name written before the statement (`Name: …`).
    name: Option<Word>,
    statement: RuleStatement,
    /// The statement as written, whitespace collapsed, for the parser's
    /// own diagnostics. The guards and properties carry
    /// [`Scope::statement_text`] instead.
    detail: String,
    span: Span,
}

/// `rule REQ "text" { statement+ }`, as read.
pub(in crate::parser) struct RuleDeclaration {
    id: String,
    id_span: Span,
    text: String,
    span: Span,
    statements: Vec<Located>,
}

impl Parser<'_> {
    /// `rule REQ "text" { … }`.
    pub(in crate::parser) fn rule_declaration(&mut self) -> Result<RuleDeclaration, ParseError> {
        let start = self.bump_kw(Kw::Rule);
        let id_start = self.peek().span;
        let id = self.req_id()?;
        let id_span = join_span(id_start, self.last_span());
        let (text, text_span) = self.expect_string_with_span()?;
        let span = join_span(start, text_span);
        self.expect_symbol("{")?;
        let mut statements = Vec::new();
        while !self.eat_symbol("}") {
            let name = if matches!(self.peek().kind, TokenKind::Ident(_))
                && is_symbol(self.peek_n(1), ":")
            {
                let name = self.word()?;
                self.bump();
                Some(name)
            } else {
                None
            };
            let begin = self.peek().span;
            let statement = self.rule_statement()?;
            if let Some(name) = &name
                && !matches!(
                    statement,
                    RuleStatement::Always(..)
                        | RuleStatement::AlwaysHas { .. }
                        | RuleStatement::Enters { .. }
                )
            {
                return Err(ParseError::coded(
                    INVALID_RULE_PROPERTY_NAME,
                    format!(
                        "'{}:' names a property, but this statement lowers to a guard or a \
                         deadline; only `always` and `enters` statements take a name",
                        name.text
                    ),
                    name.span,
                ));
            }
            let span = join_span(begin, self.last_span());
            self.spellings.statements.push(span);
            let detail = self.source[span.start.offset..span.end.offset]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            statements.push(Located {
                name,
                statement,
                detail,
                span,
            });
        }
        if statements.is_empty() {
            return Err(ParseError::new(format!("rule {id} has no statement"), span));
        }
        Ok(RuleDeclaration {
            id,
            id_span,
            text,
            span,
            statements,
        })
    }

    fn rule_statement(&mut self) -> Result<RuleStatement, ParseError> {
        if self.eat_kw(Kw::Allow) {
            let phrase = self.phrase(&[Kw::AllowOnlyWhen, Kw::AllowOnlyFor])?;
            let condition = self.rule_condition(Kw::AllowOnlyWhen, Kw::AllowOnlyFor)?;
            return Ok(RuleStatement::Allow(phrase, condition));
        }
        if self.eat_kw(Kw::Never) {
            let phrase = self.phrase(&[Kw::NeverWhen, Kw::NeverFor])?;
            let condition = self.rule_condition(Kw::NeverWhen, Kw::NeverFor)?;
            return Ok(RuleStatement::Never(phrase, condition));
        }
        if self.eat_kw(Kw::Always) {
            if self.peek_kw_at(1, Kw::AlwaysNotIn)
                || self.peek_kw_at(1, Kw::AlwaysIn)
                || matches!(&self.peek_n(1).kind, TokenKind::Ident(word) if word == "not")
            {
                let entity = self.word()?;
                let negated = self.eat_kw(Kw::AlwaysNotIn);
                if !negated {
                    self.expect_kw(Kw::AlwaysIn)?;
                }
                let stages = self.word_list()?;
                self.expect_kw(Kw::AlwaysHas)?;
                let from = self.cursor;
                let has = self.expression(0)?;
                return Ok(RuleStatement::AlwaysHas {
                    entity,
                    negated,
                    stages,
                    has,
                    has_text: self.canonical_text(from),
                });
            }
            let left_start = self.peek().span;
            let left = self.conjuncts()?;
            let is = self.peek().span;
            if !self.peek_kw(Kw::AlwaysIs) {
                return Err(self.error(&format!("expected {}", self.kw_spelling(Kw::AlwaysIs))));
            }
            self.bump();
            let right = self.conjuncts()?;
            self.spellings.checks.push(crate::CheckSpelling {
                is,
                span: join_span(left_start, self.last_span()),
            });
            return Ok(RuleStatement::Always(left, right));
        }
        if self.eat_kw(Kw::Deadline) {
            let name = self.expect_ident()?;
            self.expect_symbol("<=")?;
            return Ok(RuleStatement::Deadline {
                name,
                bound: self.expression(0)?,
            });
        }
        if self.peek_kw_at(1, Kw::Enters) {
            let entity = self.word()?;
            self.bump_kw(Kw::Enters);
            let target = self.word()?;
            self.expect_kw(Kw::OnlyFrom)?;
            return Ok(RuleStatement::Enters {
                entity,
                target,
                sources: self.word_list()?,
            });
        }
        if !matches!(self.peek().kind, TokenKind::Ident(_)) {
            return Err(self.error(
                "expected a rule statement: allow, never, always, deadline, \
                 `<phrase> only after <phrase>` or `<Entity> enters <Stage> only from …`",
            ));
        }
        let later = self.phrase(&[Kw::OnlyAfter])?;
        self.expect_kw(Kw::OnlyAfter)?;
        Ok(RuleStatement::After(later, self.phrase(&[])?))
    }

    /// `Actor verb Entity` / `Actor Entity verb`, or the same with particles
    /// on one line up to one of `stops`.
    fn phrase(&mut self, stops: &[Kw]) -> Result<Phrase, ParseError> {
        if !self.phrase_has_particles(stops) {
            let actor = self.word()?;
            let words = [self.word()?, self.word()?];
            return Ok(Phrase {
                span: join_span(actor.span, words[1].span),
                actor,
                words,
                particles: false,
            });
        }
        let start = self.peek().span;
        let mut roles = Roles::default();
        while matches!(self.peek().kind, TokenKind::Ident(_))
            && self.peek().span.start.line == start.start.line
            && !stops.iter().any(|stop| self.peek_kw(*stop))
        {
            let word = self.word()?;
            match self.particle_here() {
                Some(particle) => {
                    self.bump();
                    if particle == Particle::De {
                        return Err(ParseError::coded(
                            PARTICLE_ROLE_MISMATCH,
                            format!(
                                "'{}' takes で (input), but a rule's phrase names events by \
                                 actor, verb and entity; state the input in the condition \
                                 (`input {}`)",
                                word.text, word.text
                            ),
                            word.span,
                        ));
                    }
                    roles.mark(word, particle, "a rule phrase")?;
                }
                None => roles.unmarked(word)?,
            }
        }
        let actor = roles.actor(start)?;
        match (roles.verb, roles.object) {
            (Some(verb), Some(object)) => Ok(Phrase {
                actor,
                words: [verb, object],
                particles: true,
                span: join_span(start, self.last_span()),
            }),
            (Some(word), None) => Err(ParseError::coded(
                MIXED_PARTICLES,
                format!(
                    "a rule phrase names its entity with を and its verb without a particle; \
                     '{}' is the only word without one",
                    word.text
                ),
                word.span,
            )),
            (None, _) => Err(ParseError::new(
                "this rule phrase names no verb: the word without a particle",
                start,
            )),
        }
    }

    fn word_list(&mut self) -> Result<Vec<Word>, ParseError> {
        let mut words = vec![self.word()?];
        while self.eat_symbol(",") {
            words.push(self.word()?);
        }
        Ok(words)
    }

    /// `<when> <expression>` (inputs spelled `input name`) or
    /// `<for> <concept expression>`: allow's `only when` / `only for`, never's
    /// `when` / `for`.
    fn rule_condition(&mut self, when: Kw, r#for: Kw) -> Result<Condition, ParseError> {
        if self.eat_kw(when) {
            let from = self.cursor;
            let expression = self.event_expression()?;
            Ok(Condition::When(expression, self.canonical_text(from)))
        } else if self.eat_kw(r#for) {
            Ok(Condition::For(self.conjuncts()?))
        } else {
            Err(self.error(&format!(
                "expected {} <condition> or {} <concept>",
                self.kw_spelling(when),
                self.kw_spelling(r#for)
            )))
        }
    }

    /// `C {and C}` where each `C` is a concept or `Stage Entity`. The entity
    /// of `Stage Entity` is on the stage's line: a statement starts on a new
    /// line, so a word on the next line is not part of the expression.
    fn conjuncts(&mut self) -> Result<Vec<Conjunct>, ParseError> {
        let mut conjuncts = Vec::new();
        loop {
            let first = self.word()?;
            let next = self.peek();
            let same_line = next.span.start.line == first.span.start.line;
            let conjunct = if matches!(next.kind, TokenKind::Ident(_))
                && same_line
                && !self.peek_kw(Kw::And)
                && !self.peek_kw(Kw::AlwaysIs)
                && !self.peek_kw_at(1, Kw::Enters)
            {
                Conjunct::Stage {
                    stage: first,
                    entity: self.word()?,
                }
            } else {
                Conjunct::Concept(first)
            };
            conjuncts.push(conjunct);
            if !self.eat_kw(Kw::And) {
                return Ok(conjuncts);
            }
        }
    }
}

impl Parser<'_> {
    /// The tokens read since `from` in canonical names and English keywords:
    /// aliases are already resolved on the tokens, a Japanese keyword of an
    /// expression (`入力`, `かつ`, `または`) is spelled in English through the
    /// keyword table, and the space between two tokens is one space. An
    /// English source reads as written, whitespace collapsed.
    fn canonical_text(&self, from: usize) -> String {
        let mut text = String::new();
        let mut previous: Option<Span> = None;
        for token in &self.tokens[from..self.cursor] {
            let written = &self.source[token.span.start.offset..token.span.end.offset];
            let spelled = match &token.kind {
                TokenKind::Ident(word) => english_of(KeywordContext::RuleExpression, word)
                    .or_else(|| english_of(KeywordContext::Expression, word))
                    .unwrap_or(word),
                _ => written,
            };
            if previous.is_some_and(|previous| previous.end.offset < token.span.start.offset) {
                text.push(' ');
            }
            text.push_str(spelled);
            previous = Some(token.span);
        }
        text
    }
}

/// `C {and C}` in canonical names and English keywords.
fn conjuncts_text(conjuncts: &[Conjunct]) -> String {
    conjuncts
        .iter()
        .map(|conjunct| match conjunct {
            Conjunct::Concept(name) => name.text.clone(),
            Conjunct::Stage { stage, entity } => format!("{} {}", stage.text, entity.text),
        })
        .collect::<Vec<_>>()
        .join(&format!(" {} ", Kw::And.english()))
}

fn words_text(words: &[Word]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What the rules leave for the rest of the desugaring.
#[derive(Default)]
pub(in crate::parser) struct RulesOutcome {
    /// Ghost fields per entity, appended to its declared fields.
    pub ghosts: BTreeMap<String, Vec<ProcessField>>,
    /// `requirement` blocks carrying the rules' deadlines, with the item
    /// position each rule was declared at.
    pub generated: Vec<(usize, RequirementsItem)>,
    /// Per requirement with `allow` / `never`: its span and the events those
    /// statements guard, for [`warn_unprobed`].
    probes: Vec<(String, Span, BTreeSet<String>)>,
}

/// The declarations rule statements resolve against.
pub(in crate::parser) struct Scope<'a> {
    pub actors: &'a BTreeSet<&'a str>,
    pub entities: &'a BTreeMap<String, Entity>,
    pub concepts: &'a [ConceptDeclaration],
    pub events: &'a [EventSignature],
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn stage_is(entity: &str, stage: &str) -> Expr {
    binary(
        "==",
        Expr::Var(stage_reference(entity)),
        Expr::Var(stage.to_owned()),
    )
}

fn any_of(terms: impl IntoIterator<Item = Expr>, op: &str) -> Option<Expr> {
    terms
        .into_iter()
        .reduce(|left, right| binary(op, left, right))
}

/// `expression` with every bare or `Entity.field` name of a field of
/// `entity` spelled `Entity.field`, so it reads the field even in an event
/// with an input of the same name.
fn field_names(expression: Expr, entity: &Entity, entity_name: &str) -> Expr {
    let recurse = |expression: Expr| field_names(expression, entity, entity_name);
    let boxed = |expression: Box<Expr>| Box::new(recurse(*expression));
    let dotted = |field: &str| Expr::Var(format!("{entity_name}.{field}"));
    match expression {
        Expr::Var(name) if entity.fields.contains_key(&name) => dotted(&name),
        Expr::EnumMember {
            type_name, member, ..
        } if type_name == entity_name && entity.fields.contains_key(&member) => dotted(&member),
        Expr::Field(receiver, field)
            if matches!(receiver.as_ref(), Expr::Var(name) if name == entity_name)
                && entity.fields.contains_key(&field) =>
        {
            dotted(&field)
        }
        Expr::Field(receiver, field) => Expr::Field(boxed(receiver), field),
        Expr::Some(value) => Expr::Some(boxed(value)),
        Expr::Set(values) => Expr::Set(values.into_iter().map(recurse).collect()),
        Expr::Seq(values) => Expr::Seq(values.into_iter().map(recurse).collect()),
        Expr::Index(map, key) => Expr::Index(boxed(map), boxed(key)),
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: boxed(left),
            right: boxed(right),
        },
        Expr::Neg(value) => Expr::Neg(boxed(value)),
        Expr::Not(value) => Expr::Not(boxed(value)),
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
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args.into_iter().map(recurse).collect(),
            span,
        },
        other => other,
    }
}

impl Scope<'_> {
    fn entity(&self, word: &Word) -> Result<&Entity, ParseError> {
        self.entities.get(&word.text).ok_or_else(|| {
            ParseError::new(
                format!(
                    "'{}' is not an entity with a lifecycle; declared: {}",
                    word.text,
                    join_names(self.entities.keys().map(String::as_str))
                ),
                word.span,
            )
        })
    }

    /// That `stage` is a stage of the lifecycle of `entity`.
    pub(super) fn stage_of(&self, entity: &Word, stage: &Word) -> Result<(), ParseError> {
        self.stage(entity, stage)
    }

    /// `entity` is in one of `stages`: `in S1, S2`.
    pub(super) fn stages_condition(
        &self,
        entity: &Word,
        stages: &[Word],
    ) -> Result<Expr, ParseError> {
        for stage in stages {
            self.stage(entity, stage)?;
        }
        Ok(any_of(
            stages
                .iter()
                .map(|stage| stage_is(&entity.text, &stage.text)),
            "or",
        )
        .expect("a stage"))
    }

    fn stage(&self, entity: &Word, stage: &Word) -> Result<(), ParseError> {
        if self.entity(entity)?.has_stage(&stage.text) {
            Ok(())
        } else {
            Err(ParseError::new(
                format!(
                    "'{}' is not a stage of the lifecycle of '{}'",
                    stage.text, entity.text
                ),
                stage.span,
            ))
        }
    }

    /// The entity a phrase is about and the events it names: every event
    /// with its actor, verb and entity (B.6: a phrase can name several).
    /// The phrase's spelling, by role, goes to `phrases`.
    fn phrase(
        &self,
        phrase: &Phrase,
        phrases: &mut Vec<crate::PhraseSpelling>,
    ) -> Result<(String, Vec<&EventSignature>), ParseError> {
        if phrase.particles {
            let [verb, entity] = &phrase.words;
            check_marked_roles(
                &phrase.actor,
                Some(entity),
                Some(verb),
                self.actors,
                |word| self.entities.contains_key(word),
            )?;
        }
        if !self.actors.contains(phrase.actor.text.as_str()) {
            return Err(actor_not_declared(&phrase.actor, self.actors));
        }
        let [first, second] = &phrase.words;
        let (verb, entity) = if self.entities.contains_key(&second.text) {
            (first, second)
        } else if self.entities.contains_key(&first.text) {
            (second, first)
        } else {
            return Err(ParseError::new(
                format!(
                    "rule phrase '{} {} {}' names no entity with a lifecycle; declared: {}",
                    phrase.actor.text,
                    first.text,
                    second.text,
                    join_names(self.entities.keys().map(String::as_str))
                ),
                second.span,
            ));
        };
        phrases.push(crate::PhraseSpelling {
            kind: crate::PhraseKind::Rule,
            span: phrase.span,
            particles: phrase.particles,
            actor: phrase.actor.span,
            verbs: vec![verb.span],
            object: Some(entity.span),
            inputs: Vec::new(),
        });
        let matching = self
            .events
            .iter()
            .filter(|event| {
                event.actor == phrase.actor.text
                    && event.verb == verb.text
                    && event.entity == entity.text
            })
            .collect::<Vec<_>>();
        if !matching.is_empty() {
            return Ok((entity.text.clone(), matching));
        }
        let verbs = self
            .events
            .iter()
            .map(|event| event.verb.as_str())
            .collect::<BTreeSet<_>>();
        if !verbs.contains(verb.text.as_str()) {
            let suggestion = nearest(&verb.text, verbs.iter().copied())
                .map_or_else(String::new, |verb| format!("; did you mean '{verb}'?"));
            return Err(ParseError::coded(
                UNKNOWN_VERB,
                format!("no event has the verb '{}'{suggestion}", verb.text),
                verb.span,
            ));
        }
        let pairs = self
            .events
            .iter()
            .filter(|event| event.actor == phrase.actor.text && event.entity == entity.text)
            .map(|event| event.verb.as_str())
            .collect::<BTreeSet<_>>();
        Err(ParseError::coded(
            UNKNOWN_EVENT_PHRASE,
            format!(
                "no event matches '{} {} {}'; verbs of '{}' on {}: {}",
                phrase.actor.text,
                verb.text,
                entity.text,
                phrase.actor.text,
                entity.text,
                if pairs.is_empty() {
                    "none".to_owned()
                } else {
                    join_names(pairs)
                }
            ),
            verb.span,
        ))
    }

    /// The condition a conjunction of concepts and `Stage Entity` terms
    /// states about one instance of `entity`.
    pub(super) fn concept_condition(
        &self,
        conjuncts: &[Conjunct],
        entity: &str,
    ) -> Result<Expr, ParseError> {
        let mut terms = Vec::new();
        for conjunct in conjuncts {
            let (about, term) = match conjunct {
                Conjunct::Concept(name) => {
                    let Some(concept) = self
                        .concepts
                        .iter()
                        .find(|concept| concept.name == name.text)
                    else {
                        let suggestion = nearest(
                            &name.text,
                            self.concepts.iter().map(|concept| concept.name.as_str()),
                        )
                        .map_or_else(String::new, |found| format!("; did you mean '{found}'?"));
                        return Err(ParseError::new(
                            format!("'{}' is not a concept{suggestion}", name.text),
                            name.span,
                        ));
                    };
                    let declared = &self.entities[&concept.entity];
                    let stages = any_of(
                        concept
                            .stages
                            .iter()
                            .map(|stage| stage_is(&concept.entity, stage)),
                        "or",
                    );
                    let condition = concept
                        .condition
                        .clone()
                        .map(|condition| field_names(condition, declared, &concept.entity));
                    let term = match (stages, condition) {
                        (Some(stages), Some(condition)) => binary("and", stages, condition),
                        (Some(term), None) | (None, Some(term)) => term,
                        (None, None) => Expr::Bool(true),
                    };
                    ((concept.entity.as_str(), name), term)
                }
                Conjunct::Stage { stage, entity } => {
                    self.stage(entity, stage)?;
                    (
                        (entity.text.as_str(), entity),
                        stage_is(&entity.text, &stage.text),
                    )
                }
            };
            if about.0 != entity {
                return Err(ParseError::new(
                    format!(
                        "'{}' is about '{}', but the rule is about '{entity}'",
                        about.1.text, about.0
                    ),
                    about.1.span,
                ));
            }
            terms.push(term);
        }
        Ok(any_of(terms, "and").expect("a concept expression has a conjunct"))
    }

    fn conjunct_entity<'w>(&'w self, conjunct: &'w Conjunct) -> Result<&'w str, ParseError> {
        match conjunct {
            Conjunct::Stage { entity, .. } => Ok(entity.text.as_str()),
            Conjunct::Concept(name) => self
                .concepts
                .iter()
                .find(|concept| concept.name == name.text)
                .map(|concept| concept.entity.as_str())
                .ok_or_else(|| {
                    ParseError::new(format!("'{}' is not a concept", name.text), name.span)
                }),
        }
    }

    /// `phrase` as `Actor verb Entity`, in whichever order and with whatever
    /// particles it was written; the verb and the entity are told apart as
    /// [`Self::phrase`] tells them.
    fn phrase_text(&self, phrase: &Phrase) -> String {
        let [first, second] = &phrase.words;
        let (verb, entity) = if phrase.particles
            || self.entities.contains_key(&second.text)
            || !self.entities.contains_key(&first.text)
        {
            (first, second)
        } else {
            (second, first)
        };
        format!("{} {} {}", phrase.actor.text, verb.text, entity.text)
    }

    /// `statement` in canonical names and English keywords, the detail of the
    /// lowering step of each guard or property it lowers to: one text however
    /// the source spelled it (aliases, Japanese keywords, particles, word
    /// order). `None` for `deadline`, which lowers to no guard or property.
    fn statement_text(&self, statement: &RuleStatement) -> Option<String> {
        let condition = |condition: &Condition, when: Kw, r#for: Kw| match condition {
            Condition::When(_, text) => format!("{} {text}", when.english()),
            Condition::For(conjuncts) => {
                format!("{} {}", r#for.english(), conjuncts_text(conjuncts))
            }
        };
        Some(match statement {
            RuleStatement::Allow(phrase, guard) => format!(
                "{} {} {}",
                Kw::Allow.english(),
                self.phrase_text(phrase),
                condition(guard, Kw::AllowOnlyWhen, Kw::AllowOnlyFor)
            ),
            RuleStatement::Never(phrase, guard) => format!(
                "{} {} {}",
                Kw::Never.english(),
                self.phrase_text(phrase),
                condition(guard, Kw::NeverWhen, Kw::NeverFor)
            ),
            RuleStatement::After(later, earlier) => format!(
                "{} {} {}",
                self.phrase_text(later),
                Kw::OnlyAfter.english(),
                self.phrase_text(earlier)
            ),
            RuleStatement::Enters {
                entity,
                target,
                sources,
            } => format!(
                "{} {} {} {} {}",
                entity.text,
                Kw::Enters.english(),
                target.text,
                Kw::OnlyFrom.english(),
                words_text(sources)
            ),
            RuleStatement::Always(left, right) => format!(
                "{} {} {} {}",
                Kw::Always.english(),
                conjuncts_text(left),
                Kw::AlwaysIs.english(),
                conjuncts_text(right)
            ),
            RuleStatement::AlwaysHas {
                entity,
                negated,
                stages,
                has_text,
                ..
            } => format!(
                "{} {} {} {} {} {has_text}",
                Kw::Always.english(),
                entity.text,
                if *negated {
                    Kw::AlwaysNotIn.english()
                } else {
                    Kw::AlwaysIn.english()
                },
                words_text(stages),
                Kw::AlwaysHas.english()
            ),
            RuleStatement::Deadline { .. } => return None,
        })
    }
}

fn transition_mut<'t>(
    transitions: &'t mut BTreeMap<String, Vec<ProcessItem>>,
    entity: &str,
    event: &str,
) -> &'t mut crate::ProcessTransition {
    transitions
        .get_mut(entity)
        .into_iter()
        .flatten()
        .find_map(|item| match item {
            ProcessItem::Transition(transition) if transition.name == event => {
                Some(transition.as_mut())
            }
            _ => None,
        })
        .expect("a resolved event has a transition")
}

/// Whether the lifecycle already orders `later` after `earlier`: no stage a
/// `later` event leaves from is reachable from the initial stage along
/// events other than `earlier`'s.
fn ordered_by_lifecycle(
    entity: &Entity,
    events: &[EventSignature],
    entity_name: &str,
    earlier: &BTreeSet<&str>,
    later: &[&EventSignature],
) -> bool {
    let mut reached = BTreeSet::from([entity.stages[0].as_str()]);
    let mut frontier = vec![entity.stages[0].as_str()];
    while let Some(stage) = frontier.pop() {
        for event in events {
            if event.entity == entity_name
                && event.from == stage
                && !earlier.contains(event.name.as_str())
                && reached.insert(event.to.as_str())
            {
                frontier.push(event.to.as_str());
            }
        }
    }
    later
        .iter()
        .all(|event| !reached.contains(event.from.as_str()))
}

/// Apply `rules` to the desugared events: conjoin their guards, add ghost
/// fields, and collect their properties and deadlines into `next`.
///
/// # Errors
///
/// Returns resolution errors for phrases, concepts, entities and stages.
#[allow(clippy::too_many_lines)]
pub(in crate::parser) fn apply(
    rules: Vec<(usize, RuleDeclaration)>,
    scope: &Scope<'_>,
    transitions: &mut BTreeMap<String, Vec<ProcessItem>>,
    next: &mut NextEditionRequirements,
) -> Result<RulesOutcome, ParseError> {
    let mut outcome = RulesOutcome::default();
    for (position, rule) in rules {
        next.rule_headers.push((rule.id.clone(), rule.id_span));
        let mut guarded = BTreeSet::new();
        let mut probed = false;
        let mut deadlines = Vec::new();
        let annotation = Annotation::Requirement {
            id: rule.id.clone(),
            text: Some(rule.text.clone()),
            span: rule.span,
        };
        let guard = |transitions: &mut BTreeMap<String, Vec<ProcessItem>>,
                     entity: &str,
                     event: &str,
                     guard: RuleGuard| {
            let transition = transition_mut(transitions, entity, event);
            let covered = transition
                .covers
                .as_ref()
                .is_some_and(|cover| cover.id == rule.id)
                || transition.annotations.source_order().iter().any(|existing| {
                    matches!(existing, Annotation::Requirement { id, .. } if *id == rule.id)
                });
            if !covered {
                transition.annotations.push(annotation.clone());
            }
            transition
                .event
                .as_mut()
                .expect("a rule guards an event")
                .rule_guards
                .push(guard);
        };
        for Located {
            name: label,
            statement,
            detail,
            span,
        } in rule.statements
        {
            let never = matches!(statement, RuleStatement::Never(..));
            let text = scope.statement_text(&statement).unwrap_or_default();
            match statement {
                RuleStatement::Allow(phrase, condition)
                | RuleStatement::Never(phrase, condition) => {
                    let (entity_name, events) =
                        scope.phrase(&phrase, &mut next.spellings.phrases)?;
                    let entity = &scope.entities[&entity_name];
                    for event in events {
                        let condition = match &condition {
                            Condition::When(expression, _) => resolve_event_names(
                                expression.clone(),
                                &event.inputs,
                                entity,
                                &entity_name,
                                &event.name,
                                span,
                            )?,
                            Condition::For(conjuncts) => {
                                scope.concept_condition(conjuncts, &entity_name)?
                            }
                        };
                        let condition = if never {
                            Expr::Not(Box::new(condition))
                        } else {
                            condition
                        };
                        guarded.insert(event.name.clone());
                        probed = true;
                        guard(
                            transitions,
                            &entity_name,
                            &event.name,
                            RuleGuard {
                                kind: if never { "never" } else { "allow" },
                                requirement: rule.id.clone(),
                                condition,
                                detail: text.clone(),
                                written: detail.clone(),
                                span,
                            },
                        );
                    }
                }
                RuleStatement::After(later, earlier) => {
                    let (entity_name, later_events) =
                        scope.phrase(&later, &mut next.spellings.phrases)?;
                    let (earlier_entity, earlier_events) =
                        scope.phrase(&earlier, &mut next.spellings.phrases)?;
                    if earlier_entity != entity_name {
                        return Err(ParseError::new(
                            format!(
                                "`only after` orders events of one instance, but '{}' is about \
                                 '{entity_name}' and '{}' about '{earlier_entity}'",
                                later.actor.text, earlier.actor.text
                            ),
                            earlier.actor.span,
                        ));
                    }
                    let entity = &scope.entities[&entity_name];
                    let earlier_names = earlier_events
                        .iter()
                        .map(|event| event.name.as_str())
                        .collect::<BTreeSet<_>>();
                    if ordered_by_lifecycle(
                        entity,
                        scope.events,
                        &entity_name,
                        &earlier_names,
                        &later_events,
                    ) {
                        next.notes.push(EditionWarning {
                            code: IMPLIED_BY_LIFECYCLE,
                            message: format!(
                                "rule {}: `{detail}` already follows from the lifecycle of \
                                 '{entity_name}'; it adds no guard",
                                rule.id
                            ),
                            span,
                        });
                        continue;
                    }
                    let ghost = format!(
                        "did_{}",
                        earlier_names
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join("_or_")
                    );
                    if entity.fields.contains_key(&ghost) {
                        return Err(ParseError::coded(
                            GENERATED_NAME_COLLISION,
                            format!(
                                "rule {}: `{detail}` generates the field '{ghost}' of \
                                 '{entity_name}', which is already declared; rename the field",
                                rule.id
                            ),
                            span,
                        ));
                    }
                    let ghosts = outcome.ghosts.entry(entity_name.clone()).or_default();
                    if !ghosts.iter().any(|field| field.name == ghost) {
                        ghosts.push(ProcessField {
                            name: ghost.clone(),
                            type_name: QualifiedName {
                                namespace: None,
                                name: "Bool".to_owned(),
                            },
                            initial: Some(Expr::Bool(false)),
                            span,
                            type_span: span,
                        });
                        for event in &earlier_events {
                            transition_mut(transitions, &entity_name, &event.name)
                                .assignments
                                .push((ghost.clone(), Expr::Bool(true)));
                        }
                    }
                    for event in later_events {
                        guard(
                            transitions,
                            &entity_name,
                            &event.name,
                            RuleGuard {
                                kind: "after",
                                requirement: rule.id.clone(),
                                condition: Expr::Var(format!("{entity_name}.{ghost}")),
                                detail: text.clone(),
                                written: detail.clone(),
                                span,
                            },
                        );
                    }
                }
                RuleStatement::Enters {
                    entity,
                    target,
                    sources,
                } => {
                    scope.stage(&entity, &target)?;
                    for source in &sources {
                        scope.stage(&entity, source)?;
                    }
                    let declared = scope.entity(&entity)?;
                    let allowed = sources
                        .iter()
                        .map(|source| source.text.as_str())
                        .collect::<BTreeSet<_>>();
                    if declared
                        .edges
                        .iter()
                        .filter(|(from, to, _)| *to == target.text && *from != target.text)
                        .all(|(from, _, _)| allowed.contains(from.as_str()))
                    {
                        next.notes.push(EditionWarning {
                            code: IMPLIED_BY_LIFECYCLE,
                            message: format!(
                                "rule {}: `{detail}` already follows from the lifecycle of \
                                 '{}'",
                                rule.id, entity.text
                            ),
                            span,
                        });
                    }
                    let old_stage = |stage: &str| {
                        binary(
                            "==",
                            Expr::UnaryNamed {
                                name: "old".to_owned(),
                                expr: Box::new(Expr::Var(stage_reference(&entity.text))),
                                span,
                            },
                            Expr::Var(stage.to_owned()),
                        )
                    };
                    let before = any_of(
                        sources
                            .iter()
                            .map(|source| old_stage(&source.text))
                            .chain(std::iter::once(old_stage(&target.text))),
                        "or",
                    )
                    .expect("a stage");
                    next.properties.push(RuleProperty {
                        name: label.as_ref().map(|label| label.text.clone()),
                        kind: "enters",
                        requirement: rule.id.clone(),
                        requirement_text: rule.text.clone(),
                        requirement_span: rule.span,
                        entity: entity.text.clone(),
                        condition: binary("=>", stage_is(&entity.text, &target.text), before),
                        detail: text.clone(),
                        written: detail.clone(),
                        span,
                    });
                }
                RuleStatement::Always(left, right) => {
                    let entity_name = scope.conjunct_entity(&left[0])?.to_owned();
                    let condition = binary(
                        "=>",
                        scope.concept_condition(&left, &entity_name)?,
                        scope.concept_condition(&right, &entity_name)?,
                    );
                    next.properties.push(RuleProperty {
                        name: label.as_ref().map(|label| label.text.clone()),
                        kind: "always",
                        requirement: rule.id.clone(),
                        requirement_text: rule.text.clone(),
                        requirement_span: rule.span,
                        entity: entity_name,
                        condition,
                        detail: text.clone(),
                        written: detail.clone(),
                        span,
                    });
                }
                RuleStatement::AlwaysHas {
                    entity,
                    negated,
                    stages,
                    has,
                    ..
                } => {
                    for stage in &stages {
                        scope.stage(&entity, stage)?;
                    }
                    let declared = scope.entity(&entity)?;
                    // `has <role>`: the relation is set.
                    let has = match has {
                        Expr::Var(role) if declared.roles.contains(&role) => {
                            binary("!=", Expr::Var(role), Expr::None)
                        }
                        other => other,
                    };
                    let in_stages = if negated {
                        any_of(
                            stages.iter().map(|stage| {
                                binary(
                                    "!=",
                                    Expr::Var(stage_reference(&entity.text)),
                                    Expr::Var(stage.text.clone()),
                                )
                            }),
                            "and",
                        )
                    } else {
                        any_of(
                            stages
                                .iter()
                                .map(|stage| stage_is(&entity.text, &stage.text)),
                            "or",
                        )
                    }
                    .expect("a stage");
                    next.properties.push(RuleProperty {
                        name: label.as_ref().map(|label| label.text.clone()),
                        kind: "always",
                        requirement: rule.id.clone(),
                        requirement_text: rule.text.clone(),
                        requirement_span: rule.span,
                        entity: entity.text.clone(),
                        condition: binary(
                            "=>",
                            in_stages,
                            field_names(has, declared, &entity.text),
                        ),
                        detail: text.clone(),
                        written: detail.clone(),
                        span,
                    });
                }
                RuleStatement::Deadline { name, bound } => {
                    deadlines.push(RequirementBlockItem::Deadline { name, bound, span });
                }
            }
        }
        if probed {
            outcome.probes.push((rule.id.clone(), rule.span, guarded));
        }
        if !deadlines.is_empty() {
            outcome.generated.push((
                position,
                RequirementsItem::Requirement {
                    id: rule.id,
                    id_span: rule.id_span,
                    text: rule.text,
                    items: deadlines,
                    span: rule.span,
                    annotations: crate::Annotations::default(),
                },
            ));
        }
    }
    Ok(outcome)
}

/// Warn for every requirement whose `allow` / `never` rules no forbidden
/// trace probes: no forbidden ends in an event they guard, or names the
/// requirement with `@requirement`.
pub(in crate::parser) fn warn_unprobed(
    outcome: &RulesOutcome,
    items: &[RequirementsItem],
    next: &mut NextEditionRequirements,
) {
    for (id, span, guarded) in &outcome.probes {
        let probed = items.iter().any(|item| match item {
            RequirementsItem::Forbidden {
                steps, annotations, ..
            } => {
                steps
                    .last()
                    .is_some_and(|step| guarded.contains(&step.name))
                    || annotations.source_order().iter().any(|annotation| {
                        matches!(annotation, Annotation::Requirement { id: named, .. } if *named == *id)
                    })
            }
            _ => false,
        });
        if !probed {
            next.warnings.push(EditionWarning {
                code: NEVER_RULE_WITHOUT_FORBIDDEN,
                message: format!(
                    "rule {id} rejects events with `allow` / `never`, but no forbidden trace \
                     ends in an event it guards ({}); add a `forbidden … then rejected` that \
                     tries one",
                    join_names(guarded.iter().map(String::as_str))
                ),
                span: *span,
            });
        }
    }
}

/// Whether the member starting here is a rule: `rule` followed by a
/// requirement id.
pub(in crate::parser) fn starts_rule(parser: &Parser<'_>) -> bool {
    parser.peek_kw(Kw::Rule)
        && matches!(
            parser.peek_n(1).kind,
            TokenKind::Ident(_) | TokenKind::Int(_)
        )
}
