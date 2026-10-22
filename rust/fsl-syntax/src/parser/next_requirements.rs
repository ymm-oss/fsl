// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The next-edition requirements members `actor`, `entity { … }`, `concept`,
//! `event`, `scenario` and `forbidden … then rejected` (fsl 5 plan §3 S4).
//!
//! They are read only in a document headed `fsl 5` and desugared, once the
//! whole document is read, into the items the published edition already
//! has: each entity with a body becomes a `process` whose transitions are the
//! entity's events, and each scenario or forbidden becomes an `acceptance` or
//! a `forbidden` of event calls. Every consumer of the surface therefore sees
//! the items it already reads, and the kernel's generated names are the
//! process form's. What the desugared items cannot carry (whether an event is
//! fair, whether it moves, the actors, the concepts, the warnings) is kept on
//! [`EventForm`] and [`NextEditionRequirements`].
//!
//! The process form itself is not part of this edition
//! (`mixed_requirements_forms`, decisions/046 Q2).
//!
//! `rule` blocks (fsl 5 plan §3 S6) are read and applied by [`rules`]: their
//! guards land on the desugared events' [`EventForm::rule_guards`], their
//! properties on [`NextEditionRequirements::properties`].
//!
//! Keywords are read through the keyword table (S5, [`crate::keywords`]), in
//! English or in Japanese. A phrase (an event declaration's, a rule's, a
//! decision table row's outcome, a scenario step) may mark its words with the
//! particles が (actor), を (entity or instance) and で (input), written as
//! words of their own; the words then may come in any order, and every word
//! but the verb takes a particle (decisions/048 J9). Without particles a
//! phrase is read by position, as in English.

use std::collections::{BTreeMap, BTreeSet};

mod decide;
mod rules;

use super::{Parser, join_span};
use crate::glossary::Glossary;
use crate::keywords::{Kw, Particle};
use crate::syntax_expr::parse_tokens_expression;
use crate::{
    AcceptanceExpectation, AcceptanceStep, Annotation, Annotations, BusinessItem,
    ConceptDeclaration, EditionWarning, EventForm, Expr, NextEditionRequirements, Param,
    ParseError, ProcessCover, ProcessField, ProcessFields, ProcessItem, ProcessTransition,
    QualifiedName, RequirementsItem, Span, SpecItem, SymbolPath, SyntaxIdent, Token, TokenKind,
    VerifyItem,
};

/// `process` / `transition` in a next-edition requirements document.
pub const MIXED_REQUIREMENTS_FORMS: &str = "mixed_requirements_forms";
/// A bare name in an event's expression that is both one of its inputs and a
/// field of its entity (decisions/046 Q6).
pub const AMBIGUOUS_INPUT_FIELD: &str = "ambiguous_input_field";
/// An event or scenario step whose actor no `actor` declares.
pub const ACTOR_NOT_DECLARED: &str = "actor_not_declared";
/// A scenario step whose verb no event declares.
pub const UNKNOWN_VERB: &str = "unknown_verb";
/// A scenario step whose actor and verb no event pairs.
pub const UNKNOWN_EVENT_PHRASE: &str = "unknown_event_phrase";
/// A scenario step more than one event could take.
pub const AMBIGUOUS_STEP: &str = "ambiguous_step";
/// An event whose move is not an edge, or whose stage is not a stage, of its
/// entity's lifecycle.
pub const EVENT_NOT_ON_LIFECYCLE: &str = "event_not_on_lifecycle";
/// Warning: a lifecycle edge no event moves along.
pub const LIFECYCLE_EDGE_WITHOUT_EVENT: &str = "lifecycle_edge_without_event";
/// A scenario naming more instances of an entity than `verify` bounds.
pub const SCENARIO_INSTANCE_OVERFLOW: &str = "scenario_instance_overflow";
/// Warning: an actor spelled as an enum member.
pub const ACTOR_SHADOWS_MEMBER: &str = "actor_shadows_member";
/// A particle that gives a phrase's word a role of the wrong kind (`従業員 を
/// 申請 が 提出する`: the actor marked as the entity), or a role the phrase
/// has no place for.
pub const PARTICLE_ROLE_MISMATCH: &str = "particle_role_mismatch";
/// A particle written onto the word it marks (`従業員が`): the word is then
/// read as one unknown name.
pub const PARTICLE_NEEDS_SPACE: &str = "particle_needs_space";
/// A phrase that marks some of its words with particles and not others
/// (decisions/048 J9: with particles, every word but the verb takes one).
pub const MIXED_PARTICLES: &str = "mixed_particles";

/// The spelling `input name` is read as inside an event's expression: no
/// identifier contains a space, so it cannot meet a declared name.
const INPUT_MARK: &str = "input ";

#[derive(Clone, Debug)]
struct Word {
    text: String,
    span: Span,
}

enum Move {
    Between(Word, Word),
    Stay(Word),
}

struct EntityDeclaration {
    name: Word,
    fields: Vec<ProcessField>,
    lifecycles: Vec<Vec<Word>>,
    span: Span,
}

struct EventDeclaration {
    fault: bool,
    name: Word,
    actor: Word,
    /// `[verb] entity` or `entity verb`; with particles `[verb] entity`, or
    /// `[verb]` when no word takes を.
    words: Vec<Word>,
    /// With particles: the verb and the entity as marked.
    marked: Option<(Option<Word>, Option<Word>)>,
    inputs: Vec<(Word, Option<QualifiedName>)>,
    movement: Move,
    sets: Vec<(Word, Expr)>,
    covers: Vec<ProcessCover>,
    span: Span,
}

enum Step {
    Phrase {
        actor: Word,
        /// The verb and the instance, in either order; with particles,
        /// `[verb, instance]`.
        words: [Word; 2],
        inputs: Vec<(Word, Expr)>,
        particles: bool,
        span: Span,
    },
    Call(AcceptanceStep),
}

enum Check {
    Stage { instance: Word, stage: Word },
    Expr(Expr),
}

struct TraceDeclaration {
    forbidden: bool,
    id: String,
    text: String,
    steps: Vec<Step>,
    check: Option<(Check, Span)>,
    span: Span,
    annotations: Annotations,
}

/// `concept name = entity [in stages] [where condition]`, as read.
struct ConceptSource {
    name: Word,
    entity: Word,
    stages: Vec<Word>,
    condition: Option<Expr>,
    span: Span,
}

/// Next-edition members read so far, each with the item position it was
/// declared at.
#[derive(Default)]
pub(super) struct Pending {
    actors: Vec<Word>,
    entities: Vec<(usize, EntityDeclaration)>,
    concepts: Vec<ConceptSource>,
    events: Vec<EventDeclaration>,
    traces: Vec<(usize, TraceDeclaration)>,
    decisions: Vec<decide::DecideDeclaration>,
    rules: Vec<(usize, rules::RuleDeclaration)>,
}

fn is_symbol(token: &Token, symbol: &str) -> bool {
    matches!(&token.kind, TokenKind::Symbol(value) if value == symbol)
}

/// The words of a phrase written with particles, by role: the word before
/// が, the word before を, and the one word without a particle (the verb).
#[derive(Default)]
struct Roles {
    actor: Option<Word>,
    object: Option<Word>,
    verb: Option<Word>,
}

impl Roles {
    /// Give `word` the role of `particle` in `phrase` (what the phrase is,
    /// for messages). で and に are the caller's: each phrase decides whether
    /// it has inputs; none has a counterpart.
    fn mark(&mut self, word: Word, particle: Particle, phrase: &str) -> Result<(), ParseError> {
        let slot = match particle {
            Particle::Ga => &mut self.actor,
            Particle::Wo => &mut self.object,
            Particle::Ni | Particle::De | Particle::Wa => {
                return Err(no_such_role(&word, particle, phrase));
            }
        };
        if let Some(first) = slot {
            return Err(ParseError::coded(
                PARTICLE_ROLE_MISMATCH,
                format!(
                    "both '{}' and '{}' take {} ({}) in {phrase}; a phrase has one",
                    first.text,
                    word.text,
                    particle.spelling(),
                    particle.role()
                ),
                word.span,
            ));
        }
        *slot = Some(word);
        Ok(())
    }

    /// `word` has no particle: it is the verb, unless the phrase already has
    /// one.
    fn unmarked(&mut self, word: Word) -> Result<(), ParseError> {
        if let Some(verb) = &self.verb {
            return Err(ParseError::coded(
                MIXED_PARTICLES,
                format!(
                    "'{}' and '{}' have no particle; in a phrase written with particles every \
                     word but the verb takes one (が actor, を entity, で input)",
                    verb.text, word.text
                ),
                word.span,
            ));
        }
        self.verb = Some(word);
        Ok(())
    }

    /// The actor, which a phrase written with particles marks with が.
    fn actor(&self, start: Span) -> Result<Word, ParseError> {
        self.actor.clone().ok_or_else(|| {
            ParseError::coded(
                MIXED_PARTICLES,
                "this phrase marks words with particles but not its actor; write \
                 `<actor> が`",
                start,
            )
        })
    }
}

fn no_such_role(word: &Word, particle: Particle, phrase: &str) -> ParseError {
    ParseError::coded(
        PARTICLE_ROLE_MISMATCH,
        format!(
            "'{}' takes {} ({}), but {phrase} has no {} role; it takes が (actor){}",
            word.text,
            particle.spelling(),
            particle.role(),
            particle.role(),
            match phrase {
                "an event phrase" => ", を (entity) and で (input)",
                "a scenario step" => ", を (instance) and `<input> <value> で`",
                "a rule phrase" => " and を (entity)",
                _ => "",
            }
        ),
        word.span,
    )
}

/// With particles each role is written, so a word of the wrong kind in a
/// role is a misplaced particle rather than an unknown name: an actor marked
/// を, a non-actor marked が while an actor is in the phrase, or an object
/// marked を that is not one while the verb's word is (`is_object`).
fn check_marked_roles(
    actor: &Word,
    object: Option<&Word>,
    verb: Option<&Word>,
    actors: &BTreeSet<&str>,
    is_object: impl Fn(&str) -> bool,
) -> Result<(), ParseError> {
    let mismatch = |word: &Word, message: String| {
        Err(ParseError::coded(
            PARTICLE_ROLE_MISMATCH,
            message,
            word.span,
        ))
    };
    if let Some(object) = object
        && actors.contains(object.text.as_str())
    {
        return mismatch(
            object,
            format!(
                "'{}' takes を, but it is an actor; the actor takes が",
                object.text
            ),
        );
    }
    if !actors.contains(actor.text.as_str())
        && let Some(verb) = verb.filter(|verb| actors.contains(verb.text.as_str()))
    {
        return mismatch(
            actor,
            format!(
                "'{}' takes が (actor), but '{}' is the actor; write `{} が`",
                actor.text, verb.text, verb.text
            ),
        );
    }
    if let Some(verb) = verb.filter(|verb| is_object(&verb.text)) {
        return match object {
            Some(object) if !is_object(&object.text) => mismatch(
                object,
                format!(
                    "'{}' takes を, but '{}' is what the phrase is about; write `{} を`",
                    object.text, verb.text, verb.text
                ),
            ),
            Some(_) => Ok(()),
            None => Err(ParseError::coded(
                MIXED_PARTICLES,
                format!(
                    "'{}' has no particle; in a phrase written with particles it takes を",
                    verb.text
                ),
                verb.span,
            )),
        };
    }
    Ok(())
}

/// Reject a particle written onto the word it marks (`従業員が` for `従業員
/// が`): an identifier of `tokens[from..]` that ends in a particle's character
/// and is a name (an alias or canonical name of `glossary`, or a name the
/// document writes on its own) once that character is taken off. Run before
/// aliases are resolved, on the words as written; an identifier that is
/// itself an alias or a keyword is a word of its own.
pub(super) fn reject_particles_without_space(
    tokens: &[Token],
    from: usize,
    glossary: Option<&Glossary>,
) -> Result<(), ParseError> {
    let mut names = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    if let Some(glossary) = glossary {
        for entry in &glossary.entries {
            aliases.insert(entry.alias.as_str());
            names.insert(entry.alias.as_str());
            names.insert(entry.canonical.name.as_str());
            if let Some(owner) = &entry.canonical.owner {
                names.insert(owner.as_str());
            }
        }
    }
    for token in &tokens[from..] {
        if let TokenKind::Ident(word) = &token.kind
            && !crate::is_keyword_spelling(word)
        {
            names.insert(word.as_str());
        }
    }
    for token in &tokens[from..] {
        let TokenKind::Ident(word) = &token.kind else {
            continue;
        };
        if aliases.contains(word.as_str()) || crate::is_keyword_spelling(word) {
            continue;
        }
        let Some(last) = word.chars().last() else {
            continue;
        };
        let stem = &word[..word.len() - last.len_utf8()];
        let mut buffer = [0; 4];
        if let Some(particle) = crate::keywords::particle(last.encode_utf8(&mut buffer))
            && !stem.is_empty()
            && names.contains(stem)
        {
            return Err(ParseError::coded(
                PARTICLE_NEEDS_SPACE,
                format!(
                    "'{word}' is read as one word; a particle is a word of its own: write \
                     '{stem} {}'",
                    particle.spelling()
                ),
                token.span,
            ));
        }
    }
    Ok(())
}

impl Parser<'_> {
    /// Read one next-edition member into `pending` when one starts here.
    /// `position` is the number of items read so far.
    ///
    /// # Errors
    ///
    /// Returns [`MIXED_REQUIREMENTS_FORMS`] for `process`, and syntax errors.
    pub(super) fn next_requirements_member(
        &mut self,
        pending: &mut Pending,
        position: usize,
    ) -> Result<bool, ParseError> {
        if self.peek_ident("process") {
            return Err(ParseError::coded(
                MIXED_REQUIREMENTS_FORMS,
                "this edition's requirements do not take 'process' and 'transition'; declare an \
                 'entity' with a lifecycle and its 'event's (`fslc migrate --edition 5` rewrites \
                 a process)",
                self.peek().span,
            ));
        }
        if self.peek_kw(Kw::Actor) {
            self.bump();
            loop {
                let actor = self.word()?;
                pending.actors.push(actor);
                if !self.eat_symbol(",") {
                    break;
                }
            }
        } else if self.peek_kw(Kw::Entity) && is_symbol(self.peek_n(2), "{") {
            let entity = self.entity_declaration()?;
            pending.entities.push((position, entity));
        } else if self.peek_kw(Kw::Concept) {
            let span = self.bump().span;
            let name = self.word()?;
            self.expect_symbol("=")?;
            let entity = self.word()?;
            let mut stages = Vec::new();
            if self.eat_kw(Kw::ConceptIn) {
                loop {
                    stages.push(self.word()?);
                    if !self.eat_symbol(",") {
                        break;
                    }
                }
            }
            let condition = if self.eat_kw(Kw::ConceptWhere) {
                Some(self.expression(0)?)
            } else {
                None
            };
            pending.concepts.push(ConceptSource {
                name,
                entity,
                stages,
                condition,
                span,
            });
        } else if self.peek_kw(Kw::Event)
            || (self.peek_kw(Kw::Fault) && self.peek_kw_at(1, Kw::Event))
        {
            let event = self.event_declaration()?;
            pending.events.push(event);
        } else if self.peek_kw(Kw::Decide) {
            let decision = self.decide_declaration()?;
            pending.decisions.push(decision);
        } else if self.peek_kw(Kw::Scenario) || self.peek_kw(Kw::Forbidden) {
            let trace = self.trace_declaration()?;
            pending.traces.push((position, trace));
        } else if rules::starts_rule(self) {
            let rule = self.rule_declaration()?;
            pending.rules.push((position, rule));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    fn word(&mut self) -> Result<Word, ParseError> {
        let span = self.peek().span;
        Ok(Word {
            text: self.expect_ident()?,
            span,
        })
    }

    /// `entity Name { field: Type = value … lifecycle A -> B … }`.
    fn entity_declaration(&mut self) -> Result<EntityDeclaration, ParseError> {
        let span = self.bump().span;
        let name = self.word()?;
        self.expect_symbol("{")?;
        let mut fields = Vec::new();
        let mut lifecycles = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_kw(Kw::Lifecycle) && !is_symbol(self.peek_n(1), ":") {
                self.bump();
                let mut stages = vec![self.word()?];
                while self.eat_symbol("->") {
                    stages.push(self.word()?);
                }
                lifecycles.push(stages);
                continue;
            }
            let field = self.word()?;
            self.expect_symbol(":")?;
            let type_start = self.peek().span;
            let type_name = self.qualified_name()?;
            let type_span = join_span(type_start, self.last_span());
            if !self.eat_symbol("=") {
                return Err(ParseError::new(
                    format!(
                        "field '{}' of entity '{}' needs an initial value (`{}: {} = …`)",
                        field.text, name.text, field.text, type_name.name
                    ),
                    field.span,
                ));
            }
            let initial = self.expression(0)?;
            fields.push(ProcessField {
                name: field.text,
                type_name,
                initial: Some(initial),
                span: join_span(field.span, self.last_span()),
                type_span,
            });
        }
        if lifecycles.is_empty() {
            return Err(ParseError::new(
                format!(
                    "entity '{}' has a body but no lifecycle; declare `lifecycle A -> B`, or \
                     write `entity {}` for an entity without stages",
                    name.text, name.text
                ),
                name.span,
            ));
        }
        Ok(EntityDeclaration {
            name,
            fields,
            lifecycles,
            span,
        })
    }

    /// `[fault] event name = Actor Verb Entity [with i …] S -> T | in S
    /// [set f = e, …] {covers ID "text"}`, or with particles
    /// `Actor が Entity を [Verb] [i で] S -> T …` in any order.
    fn event_declaration(&mut self) -> Result<EventDeclaration, ParseError> {
        let span = self.peek().span;
        let fault = self.eat_kw(Kw::Fault);
        self.expect_kw(Kw::Event)?;
        let name = self.word()?;
        self.expect_symbol("=")?;
        let (actor, words, mut inputs, marked) = if self.phrase_has_particles(&[Kw::EventIn]) {
            let (actor, verb, object, inputs) = self.marked_event_phrase()?;
            let words = verb.iter().chain(object.iter()).cloned().collect();
            (actor, words, inputs, Some((verb, object)))
        } else {
            let actor = self.word()?;
            let mut words = Vec::new();
            while matches!(self.peek().kind, TokenKind::Ident(_))
                && !self.peek_kw(Kw::EventWith)
                && !self.peek_kw(Kw::EventIn)
                && !is_symbol(self.peek_n(1), "->")
            {
                words.push(self.word()?);
            }
            (actor, words, Vec::new(), None)
        };
        if words.is_empty() || words.len() > 2 {
            return Err(ParseError::new(
                format!(
                    "event '{}' expects `<Actor> <verb> <Entity>` (or `<Actor> <Entity>`, whose \
                     verb is the event name)",
                    name.text
                ),
                actor.span,
            ));
        }
        if self.eat_kw(Kw::EventWith) {
            inputs.extend(self.event_inputs()?);
        }
        let movement = if self.eat_kw(Kw::EventIn) {
            Move::Stay(self.word()?)
        } else {
            let from = self.word()?;
            self.expect_symbol("->")?;
            Move::Between(from, self.word()?)
        };
        let mut sets = Vec::new();
        if self.eat_kw(Kw::EventSet) {
            loop {
                let field = self.word()?;
                self.expect_symbol("=")?;
                sets.push((field, self.event_expression()?));
                if !self.eat_symbol(",") {
                    break;
                }
            }
        }
        let mut covers = Vec::new();
        while self.peek_kw(Kw::Covers) {
            let start = self.bump().span;
            let id = self.req_id()?;
            let (text, text_span) = self.expect_string_with_span()?;
            covers.push(ProcessCover {
                id,
                text,
                span: join_span(start, text_span),
            });
        }
        Ok(EventDeclaration {
            fault,
            name,
            actor,
            words,
            marked,
            inputs,
            movement,
            sets,
            covers,
            span,
        })
    }

    /// `i [: Type] {, i [: Type]}`, the inputs after `with` (or before で).
    fn event_inputs(&mut self) -> Result<Vec<(Word, Option<QualifiedName>)>, ParseError> {
        let mut inputs = Vec::new();
        loop {
            let input = self.word()?;
            let type_name = if self.eat_symbol(":") {
                Some(self.qualified_name()?)
            } else {
                None
            };
            inputs.push((input, type_name));
            if !self.eat_symbol(",") {
                return Ok(inputs);
            }
        }
    }

    /// The particle the next token spells.
    fn particle_here(&self) -> Option<Particle> {
        match &self.peek().kind {
            TokenKind::Ident(word) => crate::keywords::particle(word),
            _ => None,
        }
    }

    /// Whether the phrase starting here marks its words with particles: a
    /// particle on the phrase's line before any of `stops`, a `->` or a
    /// brace. Only where keywords are read through the table.
    fn phrase_has_particles(&self, stops: &[Kw]) -> bool {
        if !self.keywords {
            return false;
        }
        let line = self.peek().span.start.line;
        let mut offset = 0;
        loop {
            let token = self.peek_n(offset);
            if token.span.start.line != line
                || matches!(token.kind, TokenKind::Eof)
                || ["{", "}", "->", "=>"]
                    .iter()
                    .any(|symbol| is_symbol(token, symbol))
                || stops.iter().any(|stop| self.peek_kw_at(offset, *stop))
            {
                return false;
            }
            if let TokenKind::Ident(word) = &token.kind
                && crate::keywords::particle(word).is_some()
            {
                return true;
            }
            offset += 1;
        }
    }

    /// An event phrase written with particles, up to its move: the actor,
    /// the verb and the entity (either may be missing; the desugaring
    /// checks), and the inputs marked で.
    #[allow(clippy::type_complexity)]
    fn marked_event_phrase(
        &mut self,
    ) -> Result<
        (
            Word,
            Option<Word>,
            Option<Word>,
            Vec<(Word, Option<QualifiedName>)>,
        ),
        ParseError,
    > {
        const PHRASE: &str = "an event phrase";
        let start = self.peek().span;
        let mut roles = Roles::default();
        let mut inputs = Vec::new();
        while matches!(self.peek().kind, TokenKind::Ident(_))
            && !self.peek_kw(Kw::EventIn)
            && !self.peek_kw(Kw::EventWith)
            && !is_symbol(self.peek_n(1), "->")
        {
            let mut group = self.event_inputs()?;
            match self.particle_here() {
                Some(Particle::De) => {
                    self.bump();
                    inputs.append(&mut group);
                }
                particle => {
                    if group.len() > 1 || group[0].1.is_some() {
                        return Err(ParseError::new(
                            "only the inputs of an event (marked で) are a list or carry a type",
                            group[0].0.span,
                        ));
                    }
                    let (word, _) = group.remove(0);
                    match particle {
                        Some(particle) => {
                            self.bump();
                            roles.mark(word, particle, PHRASE)?;
                        }
                        None => roles.unmarked(word)?,
                    }
                }
            }
        }
        let actor = roles.actor(start)?;
        Ok((actor, roles.verb, roles.object, inputs))
    }

    /// An expression in which `input name` names the event's input `name`.
    /// The pair is read as one identifier spelled with [`INPUT_MARK`], over
    /// the tokens up to the end of the enclosing block.
    fn event_expression(&mut self) -> Result<Expr, ParseError> {
        let mut tokens = Vec::new();
        let mut origins = Vec::new();
        let mut depth = 0usize;
        let mut index = self.cursor;
        while let Some(token) = self.tokens.get(index) {
            match &token.kind {
                TokenKind::Symbol(symbol) if ["(", "[", "{"].contains(&symbol.as_str()) => {
                    depth += 1;
                }
                TokenKind::Symbol(symbol) if [")", "]", "}"].contains(&symbol.as_str()) => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                TokenKind::Eof => break,
                _ => {}
            }
            if let (TokenKind::Ident(word), Some(next)) = (&token.kind, self.tokens.get(index + 1))
                && self.spells_kw(word, Kw::Input)
                && let TokenKind::Ident(name) = &next.kind
            {
                tokens.push(Token {
                    kind: TokenKind::Ident(format!("{INPUT_MARK}{name}")),
                    span: join_span(token.span, next.span),
                });
                origins.push(index);
                index += 2;
                continue;
            }
            tokens.push(token.clone());
            origins.push(index);
            index += 1;
        }
        let end = self
            .tokens
            .get(index)
            .map_or_else(|| self.last_span(), |token| token.span);
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: end,
        });
        origins.push(index);
        let mut cursor = 0;
        let expression =
            parse_tokens_expression(&tokens, &mut cursor, self.expression_mode(), false)?
                .into_kernel()?;
        self.cursor = origins[cursor];
        Ok(expression)
    }

    /// `scenario ID "text" { steps then check }` or
    /// `forbidden ID "text" { steps then rejected }`.
    fn trace_declaration(&mut self) -> Result<TraceDeclaration, ParseError> {
        let forbidden = self.peek_kw(Kw::Forbidden);
        let start = self.bump().span;
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        let span = join_span(start, text_span);
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut steps = Vec::new();
        while !(self.peek_kw(Kw::Then) || (forbidden && self.peek_ident("expect"))) {
            steps.push(self.trace_step()?);
        }
        let check_span = self.bump().span;
        let check = if forbidden {
            self.expect_kw(Kw::Rejected)?;
            None
        } else if matches!(self.peek().kind, TokenKind::Ident(_))
            && self.peek_kw_at(1, Kw::ThenIs)
            && matches!(self.peek_n(2).kind, TokenKind::Ident(_))
        {
            let instance = self.word()?;
            self.bump();
            let stage = self.word()?;
            if self.peek_kw(Kw::ThenAnd) {
                return Err(self.error(
                    "a scenario checks one instance's stage; write the other checks as an \
                     expression after `then`",
                ));
            }
            Some((Check::Stage { instance, stage }, check_span))
        } else {
            Some((Check::Expr(self.expression(0)?), check_span))
        };
        self.expect_symbol("}")?;
        Ok(TraceDeclaration {
            forbidden,
            id,
            text,
            steps,
            check,
            span,
            annotations,
        })
    }

    /// `Actor verb instance [with input value, …]`, `Actor instance verb …`,
    /// the same with particles (`Actor が instance を [input value で] verb`,
    /// in any order, on one line), or the event call `name(args)`.
    fn trace_step(&mut self) -> Result<Step, ParseError> {
        if matches!(self.peek().kind, TokenKind::Ident(_)) && is_symbol(self.peek_n(1), "(") {
            return self.acceptance_step().map(Step::Call);
        }
        let span = self.peek().span;
        if self.phrase_has_particles(&[]) {
            return self.marked_trace_step(span);
        }
        let actor = self.word()?;
        let words = [self.word()?, self.word()?];
        let mut inputs = Vec::new();
        if self.eat_kw(Kw::StepWith) {
            inputs = self.step_inputs()?;
        }
        Ok(Step::Phrase {
            actor,
            words,
            inputs,
            particles: false,
            span,
        })
    }

    /// `input value {, input value}`, a step's inputs after `with`.
    fn step_inputs(&mut self) -> Result<Vec<(Word, Expr)>, ParseError> {
        let mut inputs = Vec::new();
        loop {
            let input = self.word()?;
            inputs.push((input, self.expression(0)?));
            if !self.eat_symbol(",") {
                return Ok(inputs);
            }
        }
    }

    /// A scenario step written with particles: the words of its line, each
    /// marked が (actor) or を (instance), the inputs `input value で`, and
    /// one verb without a particle.
    fn marked_trace_step(&mut self, span: Span) -> Result<Step, ParseError> {
        const PHRASE: &str = "a scenario step";
        let line = span.start.line;
        let mut roles = Roles::default();
        let mut inputs = Vec::new();
        while matches!(self.peek().kind, TokenKind::Ident(_)) && self.peek().span.start.line == line
        {
            if self.eat_kw(Kw::StepWith) {
                inputs.extend(self.step_inputs()?);
                continue;
            }
            let word = self.word()?;
            match self.particle_here() {
                Some(Particle::De) => {
                    return Err(ParseError::coded(
                        PARTICLE_ROLE_MISMATCH,
                        format!(
                            "'{}' takes で (input) but has no value; write `{} <value> で`",
                            word.text, word.text
                        ),
                        word.span,
                    ));
                }
                Some(particle) => {
                    self.bump();
                    roles.mark(word, particle, PHRASE)?;
                }
                None => {
                    if let Some(mut given) = self.marked_step_inputs(&word, line) {
                        inputs.append(&mut given);
                    } else {
                        roles.unmarked(word)?;
                    }
                }
            }
        }
        let actor = roles.actor(span)?;
        let Some(verb) = roles.verb else {
            return Err(ParseError::new(
                "this scenario step names no verb: the word without a particle",
                span,
            ));
        };
        let Some(instance) = roles.object else {
            return Err(ParseError::new(
                format!(
                    "this scenario step names no instance; mark it with を (`<instance> を {}`)",
                    verb.text
                ),
                span,
            ));
        };
        Ok(Step::Phrase {
            actor,
            words: [verb, instance],
            inputs,
            particles: true,
            span,
        })
    }

    /// After the word `input`, already read: `value {, input value} で` on
    /// `line`, the step's inputs; `None` (nothing consumed) when what
    /// follows is not that, so the word is the verb.
    fn marked_step_inputs(&mut self, input: &Word, line: u32) -> Option<Vec<(Word, Expr)>> {
        let start = self.cursor;
        let mut inputs = Vec::new();
        let mut name = input.clone();
        loop {
            if self.peek().span.start.line != line || self.particle_here().is_some() {
                break;
            }
            let mut cursor = self.cursor;
            let Ok(value) =
                parse_tokens_expression(&self.tokens, &mut cursor, self.expression_mode(), true)
                    .and_then(crate::SyntaxExpr::into_kernel)
            else {
                break;
            };
            self.cursor = cursor;
            inputs.push((name, value));
            if self.particle_here() == Some(Particle::De) {
                self.bump();
                return Some(inputs);
            }
            if !self.eat_symbol(",") {
                break;
            }
            match self.word() {
                Ok(next) => name = next,
                Err(_) => break,
            }
        }
        self.cursor = start;
        None
    }
}

struct Entity {
    stages: Vec<String>,
    edges: Vec<(String, String, Span)>,
    fields: BTreeMap<String, QualifiedName>,
}

impl Entity {
    fn has_stage(&self, stage: &str) -> bool {
        self.stages.iter().any(|candidate| candidate == stage)
    }
}

/// One desugared event, as scenario steps resolve against it.
struct EventSignature {
    name: String,
    actor: String,
    verb: String,
    entity: String,
    from: String,
    to: String,
    inputs: Vec<String>,
}

/// A scenario instance name: its entity, number and simulated stage.
struct Instance {
    entity: String,
    number: i64,
    stage: String,
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (row, left_char) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != *right_char);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn nearest<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|candidate| (levenshtein(word, candidate), candidate))
        .filter(|(distance, candidate)| *distance <= candidate.chars().count().max(2) / 2)
        .min()
        .map(|(_, candidate)| candidate)
}

fn join_names<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    names.into_iter().collect::<Vec<_>>().join(", ")
}

impl Pending {
    /// Desugar the members read into `items` (the published items, in
    /// order), returning the items with each entity's `process` and each
    /// trace placed where it was declared.
    ///
    /// # Errors
    ///
    /// Returns the stage's coded errors and plain errors for undeclared
    /// names.
    #[allow(clippy::too_many_lines)]
    pub(super) fn desugar(
        self,
        items: Vec<RequirementsItem>,
    ) -> Result<(Vec<RequirementsItem>, NextEditionRequirements), ParseError> {
        let mut next = NextEditionRequirements::default();
        let actors = self
            .actors
            .iter()
            .map(|actor| actor.text.as_str())
            .collect::<BTreeSet<_>>();
        let mut instance_bounds = BTreeMap::new();
        for item in &items {
            match item {
                RequirementsItem::Common(SpecItem::VerifyBounds { items, .. }) => {
                    for bound in items {
                        if let VerifyItem::Instances(name, count, _) = bound {
                            instance_bounds.insert(name.clone(), *count);
                        }
                    }
                }
                RequirementsItem::Common(SpecItem::Enum { name, members, .. }) => {
                    for actor in &self.actors {
                        if members.contains(&actor.text) {
                            next.warnings.push(EditionWarning {
                                code: ACTOR_SHADOWS_MEMBER,
                                message: format!(
                                    "actor '{}' is spelled as member '{}' of enum '{name}'; \
                                     a reader cannot tell the two apart",
                                    actor.text, actor.text
                                ),
                                span: actor.span,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        next.actors = self.actors.iter().map(|actor| actor.text.clone()).collect();

        let mut entities = BTreeMap::new();
        for (_, declaration) in &self.entities {
            let mut stages = Vec::<String>::new();
            let mut edges = Vec::new();
            for lifecycle in &declaration.lifecycles {
                for stage in lifecycle {
                    if !stages.contains(&stage.text) {
                        stages.push(stage.text.clone());
                    }
                }
                for pair in lifecycle.windows(2) {
                    if !edges
                        .iter()
                        .any(|(from, to, _)| (from, to) == (&pair[0].text, &pair[1].text))
                    {
                        edges.push((pair[0].text.clone(), pair[1].text.clone(), pair[1].span));
                    }
                }
            }
            let fields = declaration
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.type_name.clone()))
                .collect();
            if entities
                .insert(
                    declaration.name.text.clone(),
                    Entity {
                        stages,
                        edges,
                        fields,
                    },
                )
                .is_some()
            {
                return Err(ParseError::new(
                    format!("duplicate entity '{}'", declaration.name.text),
                    declaration.name.span,
                ));
            }
            next.entities.push(declaration.name.text.clone());
        }

        for ConceptSource {
            name,
            entity,
            stages,
            condition,
            span,
        } in self.concepts
        {
            let Some(declared) = entities.get(&entity.text) else {
                return Err(ParseError::new(
                    format!(
                        "concept '{}' names '{}', which is not an entity with a lifecycle",
                        name.text, entity.text
                    ),
                    entity.span,
                ));
            };
            if let Some(stage) = stages.iter().find(|stage| !declared.has_stage(&stage.text)) {
                return Err(ParseError::new(
                    format!(
                        "concept '{}': '{}' is not a stage of '{}'",
                        name.text, stage.text, entity.text
                    ),
                    stage.span,
                ));
            }
            next.concepts.push(ConceptDeclaration {
                name: name.text,
                entity: entity.text,
                stages: stages.into_iter().map(|stage| stage.text).collect(),
                condition,
                span,
            });
        }

        let mut transitions = BTreeMap::<String, Vec<ProcessItem>>::new();
        let mut signatures = Vec::new();
        for event in self.events {
            if let Some((verb, object)) = &event.marked {
                check_marked_roles(
                    &event.actor,
                    object.as_ref(),
                    verb.as_ref(),
                    &actors,
                    |word| entities.contains_key(word),
                )?;
                if object.is_none() {
                    return Err(ParseError::new(
                        format!(
                            "event '{}' names no entity; mark it with を",
                            event.name.text
                        ),
                        event.actor.span,
                    ));
                }
            }
            if !actors.contains(event.actor.text.as_str()) {
                return Err(actor_not_declared(&event.actor, &actors));
            }
            let (verb, entity_name) = match event.words.as_slice() {
                [entity] if entities.contains_key(&entity.text) => {
                    (event.name.text.clone(), entity.clone())
                }
                [verb, entity] if entities.contains_key(&entity.text) => {
                    (verb.text.clone(), entity.clone())
                }
                [entity, verb] if entities.contains_key(&entity.text) => {
                    (verb.text.clone(), entity.clone())
                }
                _ => {
                    return Err(ParseError::new(
                        format!(
                            "event '{}' names no entity with a lifecycle; declared: {}",
                            event.name.text,
                            join_names(entities.keys().map(String::as_str))
                        ),
                        event.words[event.words.len() - 1].span,
                    ));
                }
            };
            let entity = &entities[&entity_name.text];
            let (from, to, moves) = match &event.movement {
                Move::Between(from, to) => {
                    for stage in [from, to] {
                        if !entity.has_stage(&stage.text) {
                            return Err(not_a_stage(&event.name, stage, &entity_name.text));
                        }
                    }
                    if !entity
                        .edges
                        .iter()
                        .any(|(source, target, _)| (source, target) == (&from.text, &to.text))
                    {
                        return Err(ParseError::coded(
                            EVENT_NOT_ON_LIFECYCLE,
                            format!(
                                "event '{}' moves {} {} -> {}, which is not an edge of its \
                                 lifecycle",
                                event.name.text, entity_name.text, from.text, to.text
                            ),
                            from.span,
                        ));
                    }
                    (from.text.clone(), to.text.clone(), true)
                }
                Move::Stay(stage) => {
                    if !entity.has_stage(&stage.text) {
                        return Err(not_a_stage(&event.name, stage, &entity_name.text));
                    }
                    (stage.text.clone(), stage.text.clone(), false)
                }
            };
            let mut params = Vec::new();
            let mut assignments = Vec::new();
            let mut input_names = Vec::new();
            for (input, type_name) in &event.inputs {
                let field = entity.fields.get(&input.text);
                let Some(type_name) = type_name.clone().or_else(|| field.cloned()) else {
                    return Err(ParseError::new(
                        format!(
                            "input '{}' of event '{}' is not a field of '{}'; give its type \
                             (`with {}: <Type>`)",
                            input.text, event.name.text, entity_name.text, input.text
                        ),
                        input.span,
                    ));
                };
                if field.is_some() {
                    assignments.push((input.text.clone(), Expr::Var(input.text.clone())));
                }
                params.push(Param::Typed(input.text.clone(), type_name));
                input_names.push(input.text.clone());
            }
            for (field, expression) in event.sets {
                if !entity.fields.contains_key(&field.text) {
                    return Err(ParseError::new(
                        format!(
                            "event '{}' sets '{}', which is not a field of '{}'",
                            event.name.text, field.text, entity_name.text
                        ),
                        field.span,
                    ));
                }
                let expression = resolve_event_names(
                    expression,
                    &input_names,
                    entity,
                    &entity_name.text,
                    &event.name.text,
                    field.span,
                )?;
                assignments.push((field.text, expression));
            }
            let mut covers = event.covers.into_iter();
            let cover = covers.next();
            let mut annotations = Annotations::default();
            for extra in covers {
                annotations.push(Annotation::Requirement {
                    id: extra.id,
                    text: Some(extra.text),
                    span: extra.span,
                });
            }
            signatures.push(EventSignature {
                name: event.name.text.clone(),
                actor: event.actor.text.clone(),
                verb: verb.clone(),
                entity: entity_name.text.clone(),
                from: from.clone(),
                to: to.clone(),
                inputs: input_names,
            });
            transitions
                .entry(entity_name.text.clone())
                .or_default()
                .push(ProcessItem::Transition(Box::new(ProcessTransition {
                    name: event.name.text,
                    source: from,
                    target: to,
                    actor: event.actor.text,
                    inputs: params,
                    guard: None,
                    assignments,
                    covers: cover,
                    span: event.span,
                    annotations,
                    event: Some(EventForm {
                        verb,
                        fair: !event.fault,
                        moves,
                        rule_guards: Vec::new(),
                    }),
                })));
        }

        // Decision tables, then rules: each adds its guards to the events'
        // `rule_guards`, one `requires` line per table and per statement.
        next.decisions = decide::apply(
            self.decisions,
            &mut transitions,
            &signatures,
            &entities,
            &next.concepts,
            &actors,
        )?;
        let concepts = next.concepts.clone();
        let mut ruled = rules::apply(
            self.rules,
            &rules::Scope {
                actors: &actors,
                entities: &entities,
                concepts: &concepts,
                events: &signatures,
            },
            &mut transitions,
            &mut next,
        )?;

        let mut generated = std::mem::take(&mut ruled.generated);
        for (position, declaration) in self.entities {
            let entity = &entities[&declaration.name.text];
            for (from, to, span) in &entity.edges {
                if !signatures.iter().any(|event| {
                    event.entity == declaration.name.text && (&event.from, &event.to) == (from, to)
                }) {
                    next.warnings.push(EditionWarning {
                        code: LIFECYCLE_EDGE_WITHOUT_EVENT,
                        message: format!(
                            "lifecycle edge {from} -> {to} of '{}' has no event that moves \
                             along it",
                            declaration.name.text
                        ),
                        span: *span,
                    });
                }
            }
            let mut process_items = vec![
                ProcessItem::Stages(entity.stages.clone(), declaration.span),
                ProcessItem::Initial(
                    declaration.lifecycles[0][0].text.clone(),
                    declaration.lifecycles[0][0].span,
                ),
            ];
            process_items.extend(
                transitions
                    .remove(&declaration.name.text)
                    .unwrap_or_default(),
            );
            let name = SymbolPath::from_idents(
                vec![SyntaxIdent {
                    text: declaration.name.text.clone(),
                    span: declaration.name.span,
                }],
                declaration.name.span,
            )
            .map_err(|error| ParseError::new(error.message, error.span))?;
            let mut fields = declaration.fields;
            fields.extend(
                ruled
                    .ghosts
                    .remove(&declaration.name.text)
                    .unwrap_or_default(),
            );
            let fields = (!fields.is_empty()).then_some(ProcessFields {
                fields,
                span: declaration.span,
            });
            generated.push((
                position,
                RequirementsItem::Process(BusinessItem::Process {
                    name,
                    fields,
                    items: process_items,
                    span: declaration.span,
                }),
            ));
        }

        for (position, trace) in self.traces {
            let item = desugar_trace(trace, &actors, &signatures, &entities, &instance_bounds)?;
            generated.push((position, item));
        }
        generated.sort_by_key(|(position, _)| *position);

        let mut merged = Vec::with_capacity(items.len() + generated.len());
        let mut generated = generated.into_iter().peekable();
        for (position, item) in items.into_iter().enumerate() {
            while let Some((_, generated_item)) =
                generated.next_if(|(declared, _)| *declared <= position)
            {
                merged.push(generated_item);
            }
            merged.push(item);
        }
        merged.extend(generated.map(|(_, item)| item));
        rules::warn_unprobed(&ruled, &merged, &mut next);
        next.warnings
            .sort_by_key(|warning| (warning.span.start.line, warning.span.start.column));
        next.notes
            .sort_by_key(|note| (note.span.start.line, note.span.start.column));
        Ok((merged, next))
    }
}

fn actor_not_declared(actor: &Word, actors: &BTreeSet<&str>) -> ParseError {
    ParseError::coded(
        ACTOR_NOT_DECLARED,
        format!(
            "actor '{}' is not declared; declared actors: {}",
            actor.text,
            if actors.is_empty() {
                "none (declare `actor <Name>, …`)".to_owned()
            } else {
                join_names(actors.iter().copied())
            }
        ),
        actor.span,
    )
}

fn not_a_stage(event: &Word, stage: &Word, entity: &str) -> ParseError {
    ParseError::coded(
        EVENT_NOT_ON_LIFECYCLE,
        format!(
            "event '{}': '{}' is not a stage of the lifecycle of '{entity}'",
            event.text, stage.text
        ),
        stage.span,
    )
}

/// Resolve the names of an event's expression: `input name` is the input,
/// `Entity.field` becomes the dotted name [`entity_field`] reads as the field,
/// and a bare name that is both an input and a field is ambiguous.
fn resolve_event_names(
    expression: Expr,
    inputs: &[String],
    entity: &Entity,
    entity_name: &str,
    event: &str,
    span: Span,
) -> Result<Expr, ParseError> {
    let recurse = |expression: Expr| {
        resolve_event_names(expression, inputs, entity, entity_name, event, span)
    };
    let boxed = |expression: Box<Expr>| recurse(*expression).map(Box::new);
    Ok(match expression {
        Expr::Var(name) => {
            if let Some(input) = name.strip_prefix(INPUT_MARK) {
                if !inputs.iter().any(|candidate| candidate == input) {
                    return Err(ParseError::new(
                        format!("event '{event}' has no input '{input}'"),
                        span,
                    ));
                }
                Expr::Var(input.to_owned())
            } else if inputs.contains(&name) && entity.fields.contains_key(&name) {
                return Err(ParseError::coded(
                    AMBIGUOUS_INPUT_FIELD,
                    format!(
                        "'{name}' in event '{event}' is both its input and a field of \
                         '{entity_name}'; write `input {name}` or `{entity_name}.{name}`"
                    ),
                    span,
                ));
            } else {
                Expr::Var(name)
            }
        }
        Expr::EnumMember { type_name, member } if type_name == entity_name => {
            entity_field(entity, entity_name, &member, span)?
        }
        Expr::Field(receiver, field) if matches!(receiver.as_ref(), Expr::Var(name) if name == entity_name) => {
            entity_field(entity, entity_name, &field, span)?
        }
        Expr::Field(receiver, field) => Expr::Field(boxed(receiver)?, field),
        Expr::Some(value) => Expr::Some(boxed(value)?),
        Expr::Set(values) => Expr::Set(values.into_iter().map(recurse).collect::<Result<_, _>>()?),
        Expr::Seq(values) => Expr::Seq(values.into_iter().map(recurse).collect::<Result<_, _>>()?),
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
        other => other,
    })
}

/// `Entity.field`, spelled as one dotted name: no identifier contains a dot,
/// and the lowering substitutes the field's state for it as it does for a
/// bare field name (which an input of the same name shadows).
fn entity_field(
    entity: &Entity,
    entity_name: &str,
    field: &str,
    span: Span,
) -> Result<Expr, ParseError> {
    if entity.fields.contains_key(field) {
        Ok(Expr::Var(format!("{entity_name}.{field}")))
    } else {
        Err(ParseError::new(
            format!("'{field}' is not a field of '{entity_name}'"),
            span,
        ))
    }
}

#[allow(clippy::too_many_lines)]
fn desugar_trace(
    trace: TraceDeclaration,
    actors: &BTreeSet<&str>,
    events: &[EventSignature],
    entities: &BTreeMap<String, Entity>,
    instance_bounds: &BTreeMap<String, i64>,
) -> Result<RequirementsItem, ParseError> {
    let verbs = events
        .iter()
        .map(|event| event.verb.as_str())
        .collect::<BTreeSet<_>>();
    let mut instances = BTreeMap::<String, Instance>::new();
    let mut counts = BTreeMap::<String, i64>::new();
    let mut steps = Vec::new();
    for step in trace.steps {
        let (actor, words, inputs, particles, span) = match step {
            Step::Call(call) => {
                steps.push(call);
                continue;
            }
            Step::Phrase {
                actor,
                words,
                inputs,
                particles,
                span,
            } => (actor, words, inputs, particles, span),
        };
        if particles {
            let [verb, instance] = &words;
            check_marked_roles(&actor, Some(instance), Some(verb), actors, |word| {
                !verbs.contains(word)
            })?;
        }
        if !actors.contains(actor.text.as_str()) {
            return Err(actor_not_declared(&actor, actors));
        }
        let [first, second] = words;
        let (verb, instance) = if particles {
            if !verbs.contains(first.text.as_str()) {
                let suggestion = nearest(&first.text, verbs.iter().copied())
                    .map_or_else(String::new, |verb| format!("; did you mean '{verb}'?"));
                return Err(ParseError::coded(
                    UNKNOWN_VERB,
                    format!("no event has the verb '{}'{suggestion}", first.text),
                    first.span,
                ));
            }
            (first, second)
        } else if verbs.contains(first.text.as_str()) {
            (first, second)
        } else if verbs.contains(second.text.as_str()) {
            (second, first)
        } else {
            let suggestion = nearest(&first.text, verbs.iter().copied())
                .map_or_else(String::new, |verb| format!("; did you mean '{verb}'?"));
            return Err(ParseError::coded(
                UNKNOWN_VERB,
                format!(
                    "no event has the verb '{}' or '{}'{suggestion}",
                    first.text, second.text
                ),
                first.span,
            ));
        };
        let phrase = format!("'{} {}'", actor.text, verb.text);
        let bound = instances
            .get(&instance.text)
            .map(|bound| bound.entity.clone());
        let matching = events
            .iter()
            .filter(|event| event.actor == actor.text && event.verb == verb.text)
            .filter(|event| bound.as_ref().is_none_or(|entity| &event.entity == entity))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            let pairs = events
                .iter()
                .filter(|event| event.actor == actor.text)
                .map(|event| event.verb.as_str())
                .collect::<BTreeSet<_>>();
            return Err(ParseError::coded(
                UNKNOWN_EVENT_PHRASE,
                format!(
                    "no event matches {phrase}{}; verbs of '{}': {}",
                    bound
                        .as_ref()
                        .map_or_else(String::new, |entity| format!(" on {entity}")),
                    actor.text,
                    if pairs.is_empty() {
                        "none".to_owned()
                    } else {
                        join_names(pairs)
                    }
                ),
                verb.span,
            ));
        }
        if bound.is_none() {
            let kinds = matching
                .iter()
                .map(|event| event.entity.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let &[entity] = kinds.as_slice() else {
                return Err(ParseError::coded(
                    AMBIGUOUS_STEP,
                    format!(
                        "step {phrase} {}: '{}' could be an instance of {}",
                        instance.text,
                        instance.text,
                        join_names(kinds)
                    ),
                    span,
                ));
            };
            let count = counts.entry(entity.to_owned()).or_default();
            if let Some(&limit) = instance_bounds.get(entity)
                && *count >= limit
            {
                return Err(ParseError::coded(
                    SCENARIO_INSTANCE_OVERFLOW,
                    format!(
                        "'{}' {} '{}' is instance {} of '{entity}', but verify bounds \
                         '{entity}' to {limit} instance(s)",
                        trace.id,
                        if trace.forbidden {
                            "forbidden trace names"
                        } else {
                            "scenario names"
                        },
                        instance.text,
                        *count + 1
                    ),
                    instance.span,
                ));
            }
            let stage = entities[entity].stages[0].clone();
            instances.insert(
                instance.text.clone(),
                Instance {
                    entity: entity.to_owned(),
                    number: *count,
                    stage,
                },
            );
            *count += 1;
        }
        let state = instances
            .get_mut(&instance.text)
            .expect("the step's instance is bound");
        let from_here = matching
            .iter()
            .copied()
            .filter(|event| event.from == state.stage)
            .collect::<Vec<_>>();
        let chosen = match (from_here.as_slice(), matching.as_slice()) {
            ([event], _) | ([], [event]) => *event,
            (several, all) => {
                let candidates = if several.is_empty() {
                    all.iter()
                        .map(|event| event.name.as_str())
                        .collect::<Vec<_>>()
                } else {
                    several.iter().map(|event| event.name.as_str()).collect()
                };
                return Err(ParseError::coded(
                    AMBIGUOUS_STEP,
                    format!(
                        "step {phrase} {} matches events {} from stage {}; call the event \
                         (`name(args)`) instead",
                        instance.text,
                        join_names(candidates),
                        state.stage
                    ),
                    span,
                ));
            }
        };
        if chosen.from == state.stage {
            state.stage.clone_from(&chosen.to);
        }
        let mut given = inputs
            .into_iter()
            .map(|(name, value)| (name.text.clone(), (name, value)))
            .collect::<BTreeMap<_, _>>();
        let mut args = vec![Expr::Num(state.number)];
        for input in &chosen.inputs {
            let Some((_, value)) = given.remove(input) else {
                return Err(ParseError::new(
                    format!(
                        "step {phrase} {} gives no value for input '{input}' of event '{}' \
                         (`with {input} <value>`)",
                        instance.text, chosen.name
                    ),
                    span,
                ));
            };
            args.push(value);
        }
        if let Some((name, (word, _))) = given.into_iter().next() {
            return Err(ParseError::new(
                format!("event '{}' has no input '{name}'", chosen.name),
                word.span,
            ));
        }
        steps.push(AcceptanceStep {
            name: chosen.name.clone(),
            args,
            span,
        });
    }
    if trace.forbidden {
        return Ok(RequirementsItem::Forbidden {
            id: trace.id,
            text: trace.text,
            steps,
            span: trace.span,
            annotations: trace.annotations,
        });
    }
    let (check, check_span) = trace.check.expect("a scenario has a check");
    let expectation = match check {
        Check::Expr(expression) => AcceptanceExpectation::Expr(expression, check_span),
        Check::Stage { instance, stage } => {
            let Some(bound) = instances.get(&instance.text) else {
                return Err(ParseError::new(
                    format!(
                        "'{}' is not an instance a step of scenario '{}' names",
                        instance.text, trace.id
                    ),
                    instance.span,
                ));
            };
            if !entities[&bound.entity].has_stage(&stage.text) {
                return Err(ParseError::new(
                    format!("'{}' is not a stage of '{}'", stage.text, bound.entity),
                    stage.span,
                ));
            }
            AcceptanceExpectation::Stage {
                entity: bound.entity.clone(),
                instance: bound.number,
                stage: stage.text,
                span: check_span,
            }
        }
    };
    Ok(RequirementsItem::Acceptance {
        id: trace.id,
        text: trace.text,
        steps,
        expectation,
        span: trace.span,
        annotations: trace.annotations,
    })
}
