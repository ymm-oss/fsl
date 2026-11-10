// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Where a next-edition requirements document spelled its keywords and its
//! phrases, recorded by the parser for the formatter's rewrites
//! (`fmt --lang`, `fmt --names`) and layout.
//!
//! The parser knows which word is a keyword of which context and which word
//! of a phrase is its actor, verb or entity; the token stream does not (fsl
//! has no reserved words). Everything here is a source span, as written:
//! aliases are resolved after the spans are taken, so a span's text is the
//! word as the author wrote it.

use crate::Span;
use crate::keywords::Kw;

/// The spellings of one next-edition requirements document.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceSpellings {
    /// Every keyword read through the keyword table, in source order.
    pub keywords: Vec<KeywordSpelling>,
    /// Every phrase, in source order.
    pub phrases: Vec<PhraseSpelling>,
    /// Every stage check `x is S` / `always C1 is C2`, in source order.
    pub checks: Vec<CheckSpelling>,
    /// Statements that are not brace blocks (an event, a decision table
    /// row, a rule statement, a concept): a line a statement continues on is
    /// indented one level deeper than its first line.
    pub statements: Vec<Span>,
    /// Token ranges of the expressions read in the next edition's members
    /// (not in the published items, whose words stay English): their `and`
    /// / `or` are keywords of [`crate::keywords::KeywordContext::Expression`].
    pub expressions: Vec<ExpressionSpelling>,
}

/// One keyword as written: its row and the span of its words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeywordSpelling {
    pub kw: Kw,
    pub span: Span,
}

/// An expression's extent and whether it belongs to a next-edition member
/// (range B of the keyword table) or to a published item kept in the
/// document (its words stay English).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpressionSpelling {
    pub span: Span,
    pub member: bool,
}

/// What a phrase is part of; it decides the order its words are written in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhraseKind {
    /// `event name = Actor verb Entity [with inputs]`.
    Event,
    /// A rule statement's `Actor verb Entity`.
    Rule,
    /// A decision table row's outcome `Actor verb {or verb}`.
    DecisionRow,
    /// A scenario or forbidden step `Actor verb instance [with inputs]`.
    Step,
}

/// One phrase with its words by role.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhraseSpelling {
    pub kind: PhraseKind,
    /// From the phrase's first word to its last, particles, `with` and
    /// inputs included.
    pub span: Span,
    /// Whether the phrase marks its words with particles.
    pub particles: bool,
    pub actor: Span,
    /// The verbs: none for an event whose verb is its name, several for a
    /// decision table row joined by `or`.
    pub verbs: Vec<Span>,
    /// The entity (event, rule) or instance (step); `None` for a row.
    pub object: Option<Span>,
    /// Each input as written without `with` / で: `name[: Type]` of an event,
    /// `name value` of a step.
    pub inputs: Vec<Span>,
}

/// A stage check: `then x is S` or `always C1 is C2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckSpelling {
    /// The `is` / `は`.
    pub is: Span,
    /// From the first word checked to the last.
    pub span: Span,
}
