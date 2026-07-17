// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Standalone surface parser for the review-only `causal` profile.
//!
//! `causal Name { ... }` is deliberately not registered in the dialect
//! dispatch registry (`frontends!`): a causal model is a sidecar hypothesis
//! graph, never a kernel spec, and registering it would force the frozen
//! Python dialect registry to move (see `docs/DESIGN-causal.md` §2 and the
//! parity gate in `tests/test_coupled_change_meta.py`). Consumers detect a
//! causal document with [`is_causal_source`] before dialect dispatch, the
//! same pre-dispatch sniff pattern used for legacy AI project files.

use crate::lexer::{Token, TokenKind, lex};
use crate::parser::ParseError;
use crate::{Span, declaration_keyword};

/// One `min..max` interval in model timebase units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CausalInterval {
    pub min: u64,
    pub max: u64,
    pub span: Span,
}

/// A `lag` value: a known interval or explicit `unknown`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CausalLag {
    Known(CausalInterval),
    Unknown(Span),
}

/// A `persists` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CausalPersistence {
    Known(CausalInterval),
    Unknown(Span),
    Unbounded(Span),
}

/// `uses <alias> from "<path>"`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalUse {
    pub alias: String,
    pub path: String,
    pub span: Span,
}

/// An alias-qualified two-segment reference `alias.name`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalRef {
    pub alias: String,
    pub name: String,
    pub span: Span,
}

/// A scope-token relation declared inside a `scope` dimension block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeRelationKind {
    SubsetOf,
    Overlaps,
    DisjointWith,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeRelation {
    pub left: String,
    pub kind: ScopeRelationKind,
    pub right: String,
    pub span: Span,
}

/// One `scope <dimension> { token ... }` block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalScopeDimension {
    pub dimension: String,
    pub dimension_span: Span,
    pub tokens: Vec<(String, Span)>,
    pub relations: Vec<ScopeRelation>,
}

/// `<dimension> <token>` rows inside `default_scope { ... }` or `scope { ... }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeSelection {
    pub dimension: String,
    pub token: String,
    pub span: Span,
}

/// `clock <name> { kernel <alias>  <k> tick = <n> <unit> }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalClock {
    pub name: String,
    pub name_span: Span,
    pub kernel_alias: String,
    pub kernel_alias_span: Span,
    pub ticks: u64,
    pub units: u64,
    pub unit_name: String,
    pub ratio_span: Span,
}

/// Measurement binding kind for `observes` / `proxy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementKind {
    Kpi,
    State,
    Property,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementRef {
    pub kind: MeasurementKind,
    pub target: CausalRef,
}

/// A `variable <id> { ... }` declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalVariableDecl {
    pub id: String,
    pub id_span: Span,
    pub role: Option<(String, Span)>,
    pub binds_action: Option<CausalRef>,
    pub observes: Option<MeasurementRef>,
    pub proxy: Option<MeasurementRef>,
    pub latent: Option<Span>,
    pub cadence: Option<(u64, Span)>,
    pub deadline: Option<(u64, Span)>,
    pub window: Option<CausalInterval>,
    pub covers: Vec<(String, Span)>,
    pub scope: Vec<ScopeSelection>,
}

/// A `claim <id> <source> -> <target> { ... }` declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalClaimDecl {
    pub id: String,
    pub id_span: Span,
    pub source: (String, Span),
    pub target: (String, Span),
    pub version: Option<(u64, Span)>,
    pub status: Option<(String, Span)>,
    pub superseded_by: Option<(String, Span)>,
    pub polarity: Option<(String, Span)>,
    pub lag: Option<CausalLag>,
    pub persists: Option<CausalPersistence>,
    pub basis: Option<(String, Span)>,
    pub evidence: Vec<(String, Span)>,
    pub covers: Vec<(String, Span)>,
    pub scope: Vec<ScopeSelection>,
}

/// A `feedback <id> { claims a, b, c }` declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalFeedbackDecl {
    pub id: String,
    pub id_span: Span,
    pub claims: Vec<(String, Span)>,
}

/// An `evidence <id> from "<path>"` reference declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalEvidenceDecl {
    pub id: String,
    pub id_span: Span,
    pub path: String,
    pub span: Span,
}

/// `trigger` of an expectation: a kernel action or an inline predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectationTrigger {
    Action(CausalRef),
    Predicate {
        alias: String,
        source: String,
        span: Span,
    },
}

/// An `expectation <Id> { ... }` declaration (issue #323).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalExpectationDecl {
    pub id: String,
    pub id_span: Span,
    pub trigger: Option<ExpectationTrigger>,
    pub response: Option<(String, String, Span)>,
    pub within: Option<(u64, Span)>,
    pub clock: Option<(String, Span)>,
    pub derived_from_claim: Option<(String, Span)>,
}

/// Parsed surface form of one `causal` document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalSource {
    pub name: String,
    pub name_span: Span,
    pub uses: Vec<CausalUse>,
    pub timebase: Option<(String, Span)>,
    pub horizon: Option<(u64, Span)>,
    pub scopes: Vec<CausalScopeDimension>,
    pub default_scope: Vec<ScopeSelection>,
    pub clocks: Vec<CausalClock>,
    pub variables: Vec<CausalVariableDecl>,
    pub claims: Vec<CausalClaimDecl>,
    pub feedbacks: Vec<CausalFeedbackDecl>,
    pub evidence: Vec<CausalEvidenceDecl>,
    pub expectations: Vec<CausalExpectationDecl>,
}

/// Pre-dispatch sniff: is this document a standalone `causal` model?
#[must_use]
pub fn is_causal_source(source: &str) -> bool {
    declaration_keyword(source).is_ok_and(|keyword| keyword == "causal")
}

/// Parse a standalone `causal Name { ... }` document.
///
/// # Errors
///
/// Returns [`ParseError`] (code `FSL-CAUSAL-PARSE`) when the source is not a
/// syntactically valid causal model. Semantic validation (roles, scope
/// closure, interval bounds, references) is the typed model's job, not the
/// parser's.
pub fn parse_causal(source: &str) -> Result<CausalSource, ParseError> {
    let tokens = lex(source).map_err(ParseError::from)?;
    CausalParser {
        source: source.to_owned(),
        tokens,
        cursor: 0,
    }
    .document()
}

struct CausalParser {
    source: String,
    tokens: Vec<Token>,
    cursor: usize,
}

impl CausalParser {
    fn peek(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.cursor < self.tokens.len() - 1 {
            self.cursor += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::coded("FSL-CAUSAL-PARSE", message, self.peek().span)
    }

    fn ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match self.peek().kind.clone() {
            TokenKind::Ident(name) => {
                let span = self.peek().span;
                self.advance();
                Ok((name, span))
            }
            _ => Err(self.error(format!("expected {what}"))),
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<Span, ParseError> {
        match &self.peek().kind {
            TokenKind::Ident(name) if name == keyword => Ok(self.advance().span),
            _ => Err(self.error(format!("expected '{keyword}'"))),
        }
    }

    fn symbol(&mut self, symbol: &str) -> Result<Span, ParseError> {
        match &self.peek().kind {
            TokenKind::Symbol(text) if text == symbol => Ok(self.advance().span),
            _ => Err(self.error(format!("expected '{symbol}'"))),
        }
    }

    fn int(&mut self, what: &str) -> Result<(u64, Span), ParseError> {
        match self.peek().kind.clone() {
            TokenKind::Int(value) => {
                let span = self.peek().span;
                let value = u64::try_from(value)
                    .map_err(|_| self.error(format!("{what} must be non-negative")))?;
                self.advance();
                Ok((value, span))
            }
            _ => Err(self.error(format!("expected {what} (a non-negative integer)"))),
        }
    }

    fn string(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match self.peek().kind.clone() {
            TokenKind::String(value) => {
                let span = self.peek().span;
                self.advance();
                Ok((value, span))
            }
            _ => Err(self.error(format!("expected {what}"))),
        }
    }

    /// An identifier possibly containing dashes/digits, e.g. `REQ-ONBOARDING`
    /// or `REQ-7`, joined back into one ID string.
    fn dashed_id(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let (mut id, mut span) = self.ident(what)?;
        while matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "-") {
            self.advance();
            match self.peek().kind.clone() {
                TokenKind::Ident(part) => {
                    span.end = self.peek().span.end;
                    id = format!("{id}-{part}");
                    self.advance();
                }
                TokenKind::Int(part) if part >= 0 => {
                    span.end = self.peek().span.end;
                    id = format!("{id}-{part}");
                    self.advance();
                }
                _ => return Err(self.error(format!("expected {what} segment after '-'"))),
            }
        }
        Ok((id, span))
    }

    fn causal_ref(&mut self, what: &str) -> Result<CausalRef, ParseError> {
        let (alias, alias_span) = self.ident(&format!("{what} alias"))?;
        self.symbol(".")?;
        let (name, name_span) = self.ident(&format!("{what} name"))?;
        Ok(CausalRef {
            alias,
            name,
            span: Span {
                start: alias_span.start,
                end: name_span.end,
            },
        })
    }

    fn interval(&mut self, what: &str) -> Result<CausalInterval, ParseError> {
        let (min, min_span) = self.int(&format!("{what} lower bound"))?;
        self.symbol("..")?;
        let (max, max_span) = self.int(&format!("{what} upper bound"))?;
        Ok(CausalInterval {
            min,
            max,
            span: Span {
                start: min_span.start,
                end: max_span.end,
            },
        })
    }

    fn measurement_kind(&mut self) -> Result<MeasurementKind, ParseError> {
        let (kind, _) = self.ident("measurement kind (kpi | state | property)")?;
        match kind.as_str() {
            "kpi" => Ok(MeasurementKind::Kpi),
            "state" => Ok(MeasurementKind::State),
            "property" => Ok(MeasurementKind::Property),
            other => Err(self.error(format!(
                "unsupported measurement kind '{other}' (expected kpi | state | property)"
            ))),
        }
    }

    fn scope_selections(&mut self) -> Result<Vec<ScopeSelection>, ParseError> {
        self.symbol("{")?;
        let mut selections = Vec::new();
        while !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "}") {
            let (dimension, dimension_span) = self.ident("scope dimension")?;
            let (token, token_span) = self.ident("scope token")?;
            selections.push(ScopeSelection {
                dimension,
                token,
                span: Span {
                    start: dimension_span.start,
                    end: token_span.end,
                },
            });
        }
        self.symbol("}")?;
        Ok(selections)
    }

    fn document(mut self) -> Result<CausalSource, ParseError> {
        self.keyword("causal")?;
        let (name, name_span) = self.ident("causal model name")?;
        self.symbol("{")?;
        let mut model = CausalSource {
            name,
            name_span,
            uses: Vec::new(),
            timebase: None,
            horizon: None,
            scopes: Vec::new(),
            default_scope: Vec::new(),
            clocks: Vec::new(),
            variables: Vec::new(),
            claims: Vec::new(),
            feedbacks: Vec::new(),
            evidence: Vec::new(),
            expectations: Vec::new(),
        };
        loop {
            match self.peek().kind.clone() {
                TokenKind::Symbol(text) if text == "}" => {
                    self.advance();
                    break;
                }
                TokenKind::Eof => return Err(self.error("unexpected end of causal model")),
                TokenKind::Ident(keyword) => match keyword.as_str() {
                    "uses" => self.uses_item(&mut model)?,
                    "timebase" => {
                        self.advance();
                        let unit = self.ident("timebase unit")?;
                        if model.timebase.replace(unit).is_some() {
                            return Err(self.error("duplicate timebase declaration"));
                        }
                    }
                    "horizon" => {
                        self.advance();
                        let value = self.int("horizon")?;
                        if model.horizon.replace(value).is_some() {
                            return Err(self.error("duplicate horizon declaration"));
                        }
                    }
                    "scope" => self.scope_dimension(&mut model)?,
                    "default_scope" => {
                        self.advance();
                        if !model.default_scope.is_empty() {
                            return Err(self.error("duplicate default_scope declaration"));
                        }
                        model.default_scope = self.scope_selections()?;
                    }
                    "clock" => self.clock_item(&mut model)?,
                    "variable" => self.variable_item(&mut model)?,
                    "claim" => self.claim_item(&mut model)?,
                    "feedback" => self.feedback_item(&mut model)?,
                    "evidence" => self.evidence_item(&mut model)?,
                    "expectation" => self.expectation_item(&mut model)?,
                    other => {
                        return Err(
                            self.error(format!("unknown causal declaration keyword '{other}'"))
                        );
                    }
                },
                _ => return Err(self.error("expected a causal declaration keyword")),
            }
        }
        if !matches!(self.peek().kind, TokenKind::Eof) {
            return Err(self.error("unexpected token after causal model"));
        }
        Ok(model)
    }

    fn uses_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        let start = self.advance().span;
        let (alias, _) = self.ident("uses alias")?;
        self.keyword("from")?;
        let (path, path_span) = self.string("uses path string")?;
        model.uses.push(CausalUse {
            alias,
            path,
            span: Span {
                start: start.start,
                end: path_span.end,
            },
        });
        Ok(())
    }

    fn scope_dimension(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (dimension, dimension_span) = self.ident("scope dimension name")?;
        self.symbol("{")?;
        let mut block = CausalScopeDimension {
            dimension,
            dimension_span,
            tokens: Vec::new(),
            relations: Vec::new(),
        };
        while !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "}") {
            if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "token") {
                self.advance();
                let (token, token_span) = self.ident("scope token name")?;
                block.tokens.push((token.clone(), token_span));
                if let Some(kind) = self.peek_relation() {
                    let relation_span = self.advance().span;
                    let (right, right_span) = self.ident("scope relation target")?;
                    block.relations.push(ScopeRelation {
                        left: token,
                        kind,
                        right,
                        span: Span {
                            start: relation_span.start,
                            end: right_span.end,
                        },
                    });
                }
            } else {
                let (left, left_span) = self.ident("scope token name")?;
                let Some(kind) = self.peek_relation() else {
                    return Err(self
                        .error("expected subset_of | overlaps | disjoint_with after scope token"));
                };
                self.advance();
                let (right, right_span) = self.ident("scope relation target")?;
                block.relations.push(ScopeRelation {
                    left,
                    kind,
                    right,
                    span: Span {
                        start: left_span.start,
                        end: right_span.end,
                    },
                });
            }
        }
        self.symbol("}")?;
        model.scopes.push(block);
        Ok(())
    }

    fn peek_relation(&self) -> Option<ScopeRelationKind> {
        match &self.peek().kind {
            TokenKind::Ident(name) => match name.as_str() {
                "subset_of" => Some(ScopeRelationKind::SubsetOf),
                "overlaps" => Some(ScopeRelationKind::Overlaps),
                "disjoint_with" => Some(ScopeRelationKind::DisjointWith),
                _ => None,
            },
            _ => None,
        }
    }

    fn clock_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (name, name_span) = self.ident("clock name")?;
        self.symbol("{")?;
        self.keyword("kernel")?;
        let (kernel_alias, kernel_alias_span) = self.ident("clock kernel alias")?;
        let (ticks, ticks_span) = self.int("kernel tick count")?;
        self.keyword("tick")?;
        self.symbol("=")?;
        let (units, _) = self.int("timebase unit count")?;
        let (unit_name, unit_span) = self.ident("timebase unit name")?;
        self.symbol("}")?;
        model.clocks.push(CausalClock {
            name,
            name_span,
            kernel_alias,
            kernel_alias_span,
            ticks,
            units,
            unit_name,
            ratio_span: Span {
                start: ticks_span.start,
                end: unit_span.end,
            },
        });
        Ok(())
    }

    fn variable_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (id, id_span) = self.ident("variable id")?;
        self.symbol("{")?;
        let mut decl = CausalVariableDecl {
            id,
            id_span,
            role: None,
            binds_action: None,
            observes: None,
            proxy: None,
            latent: None,
            cadence: None,
            deadline: None,
            window: None,
            covers: Vec::new(),
            scope: Vec::new(),
        };
        while !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "}") {
            let (field, field_span) = self.ident("variable field")?;
            match field.as_str() {
                "role" => {
                    let role = self.ident("variable role")?;
                    if decl.role.replace(role).is_some() {
                        return Err(self.error("duplicate role field"));
                    }
                }
                "binds" => {
                    self.keyword("action")?;
                    let target = self.causal_ref("action")?;
                    if decl.binds_action.replace(target).is_some() {
                        return Err(self.error("duplicate binds field"));
                    }
                }
                "observes" => {
                    let kind = self.measurement_kind()?;
                    let target = self.causal_ref("measurement")?;
                    if decl
                        .observes
                        .replace(MeasurementRef { kind, target })
                        .is_some()
                    {
                        return Err(self.error("duplicate observes field"));
                    }
                }
                "proxy" => {
                    let kind = self.measurement_kind()?;
                    let target = self.causal_ref("proxy measurement")?;
                    if decl
                        .proxy
                        .replace(MeasurementRef { kind, target })
                        .is_some()
                    {
                        return Err(self.error("duplicate proxy field"));
                    }
                }
                "latent" => {
                    if decl.latent.replace(field_span).is_some() {
                        return Err(self.error("duplicate latent field"));
                    }
                }
                "cadence" => {
                    let value = self.int("cadence")?;
                    if decl.cadence.replace(value).is_some() {
                        return Err(self.error("duplicate cadence field"));
                    }
                }
                "deadline" => {
                    let value = self.int("deadline")?;
                    if decl.deadline.replace(value).is_some() {
                        return Err(self.error("duplicate deadline field"));
                    }
                }
                "window" => {
                    let value = self.interval("window")?;
                    if decl.window.replace(value).is_some() {
                        return Err(self.error("duplicate window field"));
                    }
                }
                "covers" => loop {
                    decl.covers.push(self.dashed_id("requirement id")?);
                    if !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == ",") {
                        break;
                    }
                    self.advance();
                },
                "scope" => {
                    if !decl.scope.is_empty() {
                        return Err(self.error("duplicate scope field"));
                    }
                    decl.scope = self.scope_selections()?;
                }
                other => return Err(self.error(format!("unknown variable field '{other}'"))),
            }
        }
        self.symbol("}")?;
        model.variables.push(decl);
        Ok(())
    }

    fn claim_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (id, id_span) = self.ident("claim id")?;
        let source = self.ident("claim source variable")?;
        self.symbol("->")?;
        let target = self.ident("claim target variable")?;
        self.symbol("{")?;
        let mut decl = CausalClaimDecl {
            id,
            id_span,
            source,
            target,
            version: None,
            status: None,
            superseded_by: None,
            polarity: None,
            lag: None,
            persists: None,
            basis: None,
            evidence: Vec::new(),
            covers: Vec::new(),
            scope: Vec::new(),
        };
        while !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "}") {
            let (field, _) = self.ident("claim field")?;
            match field.as_str() {
                "version" => {
                    let value = self.int("claim version")?;
                    if decl.version.replace(value).is_some() {
                        return Err(self.error("duplicate version field"));
                    }
                }
                "status" => {
                    let value = self.ident("claim status")?;
                    if decl.status.replace(value).is_some() {
                        return Err(self.error("duplicate status field"));
                    }
                }
                "superseded_by" => {
                    let value = self.ident("successor claim id")?;
                    if decl.superseded_by.replace(value).is_some() {
                        return Err(self.error("duplicate superseded_by field"));
                    }
                }
                "polarity" => {
                    let value = self.ident("claim polarity")?;
                    if decl.polarity.replace(value).is_some() {
                        return Err(self.error("duplicate polarity field"));
                    }
                }
                "lag" => {
                    if decl.lag.is_some() {
                        return Err(self.error("duplicate lag field"));
                    }
                    decl.lag = Some(self.lag_value()?);
                }
                "persists" => {
                    if decl.persists.is_some() {
                        return Err(self.error("duplicate persists field"));
                    }
                    decl.persists = Some(self.persistence_value()?);
                }
                "basis" => {
                    let value = self.ident("claim basis")?;
                    if decl.basis.replace(value).is_some() {
                        return Err(self.error("duplicate basis field"));
                    }
                }
                "evidence" => loop {
                    decl.evidence.push(self.ident("evidence id")?);
                    if !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == ",") {
                        break;
                    }
                    self.advance();
                },
                "covers" => loop {
                    decl.covers.push(self.dashed_id("requirement id")?);
                    if !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == ",") {
                        break;
                    }
                    self.advance();
                },
                "scope" => {
                    if !decl.scope.is_empty() {
                        return Err(self.error("duplicate scope field"));
                    }
                    decl.scope = self.scope_selections()?;
                }
                other => return Err(self.error(format!("unknown claim field '{other}'"))),
            }
        }
        self.symbol("}")?;
        model.claims.push(decl);
        Ok(())
    }

    fn lag_value(&mut self) -> Result<CausalLag, ParseError> {
        if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "unknown") {
            Ok(CausalLag::Unknown(self.advance().span))
        } else {
            Ok(CausalLag::Known(self.interval("lag")?))
        }
    }

    fn persistence_value(&mut self) -> Result<CausalPersistence, ParseError> {
        Ok(match &self.peek().kind {
            TokenKind::Ident(name) if name == "unknown" => {
                CausalPersistence::Unknown(self.advance().span)
            }
            TokenKind::Ident(name) if name == "unbounded" => {
                CausalPersistence::Unbounded(self.advance().span)
            }
            _ => CausalPersistence::Known(self.interval("persists")?),
        })
    }

    fn feedback_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (id, id_span) = self.ident("feedback id")?;
        self.symbol("{")?;
        self.keyword("claims")?;
        let mut claims = Vec::new();
        loop {
            claims.push(self.ident("feedback claim id")?);
            if matches!(&self.peek().kind, TokenKind::Symbol(text) if text == ",") {
                self.advance();
            } else {
                break;
            }
        }
        self.symbol("}")?;
        model.feedbacks.push(CausalFeedbackDecl {
            id,
            id_span,
            claims,
        });
        Ok(())
    }

    /// Consume a brace-delimited block and return the raw source between the
    /// braces (used for inline kernel predicate expressions, which are parsed
    /// and type-checked against the target spec by the expectation compiler).
    fn brace_source(&mut self) -> Result<(String, Span), ParseError> {
        let open = self.symbol("{")?;
        let mut depth = 1_usize;
        let start = self.peek().span.start;
        let mut end = start;
        loop {
            match &self.peek().kind {
                TokenKind::Symbol(text) if text == "{" => depth += 1,
                TokenKind::Symbol(text) if text == "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Eof => return Err(self.error("unterminated predicate block")),
                _ => {}
            }
            end = self.peek().span.end;
            self.advance();
        }
        let close = self.symbol("}")?;
        let text = self
            .source
            .get(start.offset..end.offset.max(start.offset))
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            return Err(ParseError::coded(
                "FSL-CAUSAL-PARSE",
                "empty predicate block",
                Span {
                    start: open.start,
                    end: close.end,
                },
            ));
        }
        Ok((text, Span { start, end }))
    }

    #[allow(clippy::too_many_lines)]
    fn expectation_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        self.advance();
        let (id, id_span) = self.ident("expectation id")?;
        self.symbol("{")?;
        let mut decl = CausalExpectationDecl {
            id,
            id_span,
            trigger: None,
            response: None,
            within: None,
            clock: None,
            derived_from_claim: None,
        };
        while !matches!(&self.peek().kind, TokenKind::Symbol(text) if text == "}") {
            let (field, _) = self.ident("expectation field")?;
            match field.as_str() {
                "trigger" => {
                    if decl.trigger.is_some() {
                        return Err(self.error("duplicate trigger field"));
                    }
                    let (kind, _) = self.ident("trigger kind (action | predicate)")?;
                    decl.trigger = Some(match kind.as_str() {
                        "action" => ExpectationTrigger::Action(self.causal_ref("action")?),
                        "predicate" => {
                            let (alias, _) = self.ident("trigger predicate alias")?;
                            let (text, span) = self.brace_source()?;
                            ExpectationTrigger::Predicate {
                                alias,
                                source: text,
                                span,
                            }
                        }
                        other => {
                            return Err(self.error(format!(
                                "unsupported trigger kind '{other}' (expected action | predicate)"
                            )));
                        }
                    });
                }
                "response" => {
                    if decl.response.is_some() {
                        return Err(self.error("duplicate response field"));
                    }
                    self.keyword("predicate")?;
                    let (alias, _) = self.ident("response predicate alias")?;
                    let (text, span) = self.brace_source()?;
                    decl.response = Some((alias, text, span));
                }
                "within" => {
                    let value = self.int("within")?;
                    if decl.within.replace(value).is_some() {
                        return Err(self.error("duplicate within field"));
                    }
                }
                "clock" => {
                    let value = self.ident("clock name")?;
                    if decl.clock.replace(value).is_some() {
                        return Err(self.error("duplicate clock field"));
                    }
                }
                "derived_from_claim" => {
                    let value = self.ident("claim id")?;
                    if decl.derived_from_claim.replace(value).is_some() {
                        return Err(self.error("duplicate derived_from_claim field"));
                    }
                }
                "supports" | "supports_claim" => {
                    return Err(self.error(
                        "'supports' is not an expectation field; use derived_from_claim (traceability only, never evidence support)",
                    ));
                }
                other => return Err(self.error(format!("unknown expectation field '{other}'"))),
            }
        }
        self.symbol("}")?;
        model.expectations.push(decl);
        Ok(())
    }

    fn evidence_item(&mut self, model: &mut CausalSource) -> Result<(), ParseError> {
        let start = self.advance().span;
        let (id, id_span) = self.ident("evidence id")?;
        self.keyword("from")?;
        let (path, path_span) = self.string("evidence path string")?;
        model.evidence.push(CausalEvidenceDecl {
            id,
            id_span,
            path,
            span: Span {
                start: start.start,
                end: path_span.end,
            },
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
causal Retention {
  uses biz from "business.fsl"
  timebase day
  horizon 730
  scope population {
    token all_users
    token new_users subset_of all_users
  }
  default_scope { population all_users }
  clock biz_clock { kernel biz 1 tick = 1 day }
  variable support {
    role intervention
    binds action biz.enable_support
    covers REQ-7
  }
  variable retention {
    role outcome
    observes kpi biz.retention_90d
    cadence 7
    deadline 180
    window 0..365
  }
  claim C1 support -> retention {
    version 1
    status active
    polarity positive
    lag 0..7
    persists 7..30
    basis hypothesis
  }
  evidence E1 from "evidence/e1.causal.json"
}
"#;

    #[test]
    fn parses_minimal_model() {
        let model = parse_causal(MINIMAL).expect("parse");
        assert_eq!(model.name, "Retention");
        assert_eq!(model.uses.len(), 1);
        assert_eq!(
            model.timebase.as_ref().map(|(unit, _)| unit.as_str()),
            Some("day")
        );
        assert_eq!(model.horizon.map(|(value, _)| value), Some(730));
        assert_eq!(model.scopes.len(), 1);
        assert_eq!(model.scopes[0].tokens.len(), 2);
        assert_eq!(model.scopes[0].relations.len(), 1);
        assert_eq!(model.clocks.len(), 1);
        assert_eq!(model.clocks[0].ticks, 1);
        assert_eq!(model.clocks[0].units, 1);
        assert_eq!(model.variables.len(), 2);
        assert_eq!(
            model.variables[0].covers,
            vec![("REQ-7".to_owned(), model.variables[0].covers[0].1)]
        );
        assert_eq!(model.claims.len(), 1);
        assert_eq!(model.evidence.len(), 1);
        let claim = &model.claims[0];
        assert_eq!(claim.source.0, "support");
        assert_eq!(claim.target.0, "retention");
        assert!(
            matches!(claim.lag, Some(CausalLag::Known(interval)) if interval.min == 0 && interval.max == 7)
        );
    }

    #[test]
    fn sniffs_causal_keyword() {
        assert!(is_causal_source(MINIMAL));
        assert!(!is_causal_source(
            "spec Cart { state { x: Int } init { x = 0 } }"
        ));
    }

    #[test]
    fn rejects_unknown_field_with_location() {
        let error = parse_causal("causal M { variable v { wobble 3 } }").expect_err("must fail");
        assert_eq!(error.code(), "FSL-CAUSAL-PARSE");
        assert!(error.message.contains("unknown variable field"));
        assert!(error.span.start.line >= 1);
    }

    #[test]
    fn rejects_negative_interval_bound() {
        let source = "causal M { claim C a -> b { lag 0..-3 } }";
        let error = parse_causal(source).expect_err("must fail");
        assert!(error.message.contains("non-negative integer"));
    }

    #[test]
    fn parses_unknown_lag_and_unbounded_persistence() {
        let source = "causal M { claim C a -> b { lag unknown persists unbounded } }";
        let model = parse_causal(source).expect("parse");
        assert!(matches!(model.claims[0].lag, Some(CausalLag::Unknown(_))));
        assert!(matches!(
            model.claims[0].persists,
            Some(CausalPersistence::Unbounded(_))
        ));
    }
}
