// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde::Serialize;
use serde_json::{Value, json};

use crate::annotation_parse;
use crate::syntax_expr::{
    ExpressionMode, SyntaxExpr, SyntaxExprKind, SyntaxIdent, SyntaxLValue, SyntaxTypeExpr,
    parse_tokens_expression, parse_tokens_lvalue,
};
use crate::{Annotations, ParseError, Span, Token, TokenKind, lex};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DomainLoc {
    pub line: u32,
    pub column: u32,
    #[serde(skip)]
    span: Span,
}

impl From<Span> for DomainLoc {
    fn from(span: Span) -> Self {
        Self {
            line: span.start.line,
            column: span.start.column,
            span,
        }
    }
}

impl DomainLoc {
    #[must_use]
    pub fn span(self) -> Span {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainField {
    pub name: SyntaxIdent,
    pub type_name: SyntaxTypeExpr,
    pub default: Option<SyntaxExpr>,
    pub span: Span,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainTypeSourceForm {
    CanonicalRange,
    CanonicalEnum,
    LegacyEnumUnion,
    ValueObject,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainType {
    pub name: String,
    pub kind: String,
    pub members: Vec<String>,
    pub member_spans: Vec<Span>,
    pub lo: Option<SyntaxExpr>,
    pub hi: Option<SyntaxExpr>,
    pub fields: Vec<DomainField>,
    pub invariants: Vec<DomainInvariant>,
    pub source_form: DomainTypeSourceForm,
    pub span: Span,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainCommand {
    pub name: String,
    pub inputs: Vec<DomainField>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainEvent {
    pub name: String,
    pub fields: Vec<DomainField>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainError {
    pub name: String,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainReject {
    pub error: String,
    pub condition: SyntaxExpr,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainDecide {
    pub command: String,
    pub requires: Vec<SyntaxExpr>,
    pub rejects: Vec<DomainReject>,
    pub emits: Vec<String>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainAssignment {
    pub target: SyntaxLValue,
    pub value: SyntaxExpr,
    pub span: Span,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainEvolve {
    pub event: String,
    pub requires: Vec<SyntaxExpr>,
    pub assignments: Vec<DomainAssignment>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainInvariant {
    pub name: SyntaxIdent,
    pub expr: SyntaxExpr,
    pub span: Span,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainProjection {
    pub name: String,
    pub source: String,
    pub fields: Vec<String>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainStalePolicy {
    pub event: String,
    pub condition: SyntaxExpr,
    pub emits: Vec<String>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainAggregate {
    pub name: String,
    pub id_type: Option<String>,
    pub state: Vec<DomainField>,
    pub commands: Vec<DomainCommand>,
    pub events: Vec<DomainEvent>,
    pub errors: Vec<DomainError>,
    pub decides: Vec<DomainDecide>,
    pub evolves: Vec<DomainEvolve>,
    pub invariants: Vec<DomainInvariant>,
    pub stale_policies: Vec<DomainStalePolicy>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomainRetry {
    pub max_attempts: Option<i64>,
    pub backoff: Option<String>,
    pub loc: Option<DomainLoc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainEffect {
    pub name: String,
    pub async_effect: bool,
    pub reliable: bool,
    pub irreversible: bool,
    pub idempotency_key: Option<SyntaxExpr>,
    pub correlation_id: Option<SyntaxExpr>,
    pub handles: Option<String>,
    pub outcomes: Vec<String>,
    pub request_event: Option<String>,
    pub success_event: Option<String>,
    pub failure_event: Option<String>,
    pub timeout_event: Option<String>,
    pub retry: DomainRetry,
    pub timeout_after: Option<String>,
    pub compensation_events: Vec<String>,
    pub outbox: Option<String>,
    pub inbox: Option<String>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainAwait {
    pub name: String,
    pub mode: String,
    pub events: Vec<String>,
    pub branches: Vec<(String, String)>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainSagaStep {
    pub name: String,
    pub async_step: bool,
    pub requires: Vec<SyntaxExpr>,
    pub emits: Vec<String>,
    pub awaits_mode: String,
    pub awaits: Vec<String>,
    pub timeout_after: Option<String>,
    pub timeout_event: Option<String>,
    pub annotations: Annotations,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainSagaCompensation {
    pub trigger_event: String,
    pub after_event: String,
    pub emits: Vec<String>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainSaga {
    pub name: String,
    pub starts_on: Option<String>,
    pub steps: Vec<DomainSagaStep>,
    pub compensations: Vec<DomainSagaCompensation>,
    pub invariants: Vec<DomainInvariant>,
    pub outboxes: Vec<String>,
    pub inboxes: Vec<String>,
    pub loc: DomainLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainSpec {
    pub name: String,
    pub implementation_profile: Option<String>,
    pub types: Vec<DomainType>,
    pub aggregates: Vec<DomainAggregate>,
    pub effects: Vec<DomainEffect>,
    pub awaits: Vec<DomainAwait>,
    pub sagas: Vec<DomainSaga>,
    pub projections: Vec<DomainProjection>,
    pub loc: DomainLoc,
}

macro_rules! ast_list {
    ($values:expr, $method:ident) => {
        $values
            .iter()
            .map(|value| value.$method())
            .collect::<Vec<_>>()
    };
}

impl DomainSpec {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainSpec", "name":self.name,
            "implementation_profile":self.implementation_profile,
            "types":ast_list!(self.types, python_ast),
            "aggregates":ast_list!(self.aggregates, python_ast),
            "effects":ast_list!(self.effects, python_ast),
            "awaits":ast_list!(self.awaits, python_ast),
            "sagas":ast_list!(self.sagas, python_ast),
            "projections":ast_list!(self.projections, python_ast), "loc":self.loc,
        })
    }
}

impl DomainField {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainField","name":self.name.text,"type_name":self.type_name.render_source(),"default":render_optional_expr(self.default.as_ref()),"loc":self.loc})
    }
}

impl DomainInvariant {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainInvariant","name":self.name.text,"expr":self.expr.render_source(),"loc":self.loc})
    }
}

impl DomainType {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainType","name":self.name,"kind":self.kind,"members":self.members,
            "lo":render_optional_expr(self.lo.as_ref()),"hi":render_optional_expr(self.hi.as_ref()),"fields":ast_list!(self.fields, python_ast),
            "invariants":ast_list!(self.invariants, python_ast),"loc":self.loc,
        })
    }
}

impl DomainCommand {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainCommand","name":self.name,"inputs":ast_list!(self.inputs, python_ast),"loc":self.loc})
    }
}

impl DomainEvent {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainEvent","name":self.name,"fields":ast_list!(self.fields, python_ast),"loc":self.loc})
    }
}

impl DomainError {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainError","name":self.name,"loc":self.loc})
    }
}

impl DomainReject {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainReject","error":self.error,"condition":self.condition.render_source(),"loc":self.loc})
    }
}

impl DomainDecide {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainDecide","command":self.command,"requires":render_exprs(&self.requires),
            "rejects":ast_list!(self.rejects, python_ast),"emits":self.emits,"loc":self.loc,
        })
    }
}

impl DomainAssignment {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainAssignment","target":self.target.render_source(),"expr":self.value.render_source(),"loc":self.loc})
    }
}

impl DomainEvolve {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainEvolve","event":self.event,"requires":render_exprs(&self.requires),
            "assignments":ast_list!(self.assignments, python_ast),"loc":self.loc,
        })
    }
}

impl DomainProjection {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainProjection","name":self.name,"source":self.source,"fields":self.fields,"loc":self.loc})
    }
}

impl DomainStalePolicy {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainStalePolicy","event":self.event,"condition":self.condition.render_source(),"emits":self.emits,"loc":self.loc})
    }
}

impl DomainAggregate {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainAggregate","name":self.name,"id_type":self.id_type,
            "state":ast_list!(self.state, python_ast),"commands":ast_list!(self.commands, python_ast),
            "events":ast_list!(self.events, python_ast),"errors":ast_list!(self.errors, python_ast),
            "decides":ast_list!(self.decides, python_ast),"evolves":ast_list!(self.evolves, python_ast),
            "invariants":ast_list!(self.invariants, python_ast),
            "stale_policies":ast_list!(self.stale_policies, python_ast),"loc":self.loc,
        })
    }
}

impl DomainRetry {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainRetry","max_attempts":self.max_attempts,"backoff":self.backoff,"loc":self.loc})
    }
}

impl DomainEffect {
    fn new(name: String, loc: DomainLoc) -> Self {
        Self {
            name,
            async_effect: false,
            reliable: false,
            irreversible: false,
            idempotency_key: None,
            correlation_id: None,
            handles: None,
            outcomes: Vec::new(),
            request_event: None,
            success_event: None,
            failure_event: None,
            timeout_event: None,
            retry: DomainRetry::default(),
            timeout_after: None,
            compensation_events: Vec::new(),
            outbox: None,
            inbox: None,
            annotations: Annotations::default(),
            loc,
        }
    }

    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainEffect","name":self.name,"async_effect":self.async_effect,
            "reliable":self.reliable,"irreversible":self.irreversible,
            "idempotency_key":render_optional_expr(self.idempotency_key.as_ref()),"correlation_id":render_optional_expr(self.correlation_id.as_ref()),
            "handles":self.handles,"outcomes":self.outcomes,"request_event":self.request_event,
            "success_event":self.success_event,"failure_event":self.failure_event,
            "timeout_event":self.timeout_event,"retry":self.retry.python_ast(),
            "timeout_after":self.timeout_after,"compensation_events":self.compensation_events,
            "outbox":self.outbox,"inbox":self.inbox,"loc":self.loc,
        })
    }
}

impl DomainAwait {
    fn python_ast(&self) -> Value {
        json!({"$type":"DomainAwait","name":self.name,"mode":self.mode,"events":self.events,"branches":self.branches,"loc":self.loc})
    }
}

impl DomainSagaStep {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainSagaStep","name":self.name,"async_step":self.async_step,
            "requires":render_exprs(&self.requires),"emits":self.emits,"awaits_mode":self.awaits_mode,
            "awaits":self.awaits,"timeout_after":self.timeout_after,
            "timeout_event":self.timeout_event,"loc":self.loc,
        })
    }
}

impl DomainSagaCompensation {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainSagaCompensation","trigger_event":self.trigger_event,
            "after_event":self.after_event,"emits":self.emits,"loc":self.loc,
        })
    }
}

impl DomainSaga {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"DomainSaga","name":self.name,"starts_on":self.starts_on,
            "steps":ast_list!(self.steps, python_ast),
            "compensations":ast_list!(self.compensations, python_ast),
            "invariants":ast_list!(self.invariants, python_ast),
            "outboxes":self.outboxes,"inboxes":self.inboxes,"loc":self.loc,
        })
    }
}

// Parser implementation follows the IR/projection definitions so the raw
// expression slicing stays local to this specialized frontend.

/// Parse one specialized `domain` source into typed frontend IR.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical, syntactic, or structural analysis fails.
pub fn parse_domain(source: &str) -> Result<DomainSpec, ParseError> {
    let tokens = lex(source).map_err(ParseError::from)?;
    parse_domain_tokens(tokens, 0)
}

pub(crate) fn parse_domain_tokens(
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<DomainSpec, ParseError> {
    let mut parser = DomainParser {
        tokens,
        cursor,
        pending_annotations: Annotations::default(),
    };
    let domain = parser.domain()?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after domain"));
    }
    Ok(domain)
}

struct DomainParser {
    tokens: Vec<Token>,
    cursor: usize,
    pending_annotations: Annotations,
}

impl DomainParser {
    /// Parse zero or more leading `@name(args...)` annotations into
    /// `self.pending_annotations`, ready to be drained by whichever
    /// declaration constructor runs next.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] for malformed annotation syntax, a failed
    /// validation, or one or more annotations with no following declaration
    /// in the same block.
    fn take_leading_annotations(&mut self) -> Result<(), ParseError> {
        while self.peek_symbol("@") {
            let annotation = annotation_parse::annotation(&self.tokens, &mut self.cursor)?;
            self.pending_annotations.push(annotation);
        }
        if let Some(first) = self.pending_annotations.source_order().first()
            && (self.peek_symbol("}") || matches!(self.peek().kind, TokenKind::Eof))
        {
            let span = first.span();
            self.pending_annotations = Annotations::default();
            return Err(ParseError::coded(
                "FSL-ANNOTATION-TARGET",
                "annotation must be followed by a declaration in the same block",
                span,
            ));
        }
        Ok(())
    }

    /// Drain and validate the annotations collected for the declaration
    /// currently under construction.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the drained group fails validation.
    fn take_annotations(&mut self) -> Result<Annotations, ParseError> {
        let annotations = std::mem::take(&mut self.pending_annotations);
        annotations.validate().map_err(|error| {
            ParseError::coded("FSL-ANNOTATION-INVALID", error.message, error.span)
        })?;
        Ok(annotations)
    }

    /// Reject any annotation left over after parsing a declaration that does
    /// not accept annotations.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] coded `FSL-ANNOTATION-TARGET` when annotations
    /// remain pending.
    fn expect_no_pending_annotations(&mut self) -> Result<(), ParseError> {
        if let Some(first) = self.pending_annotations.source_order().first() {
            let span = first.span();
            self.pending_annotations = Annotations::default();
            return Err(ParseError::coded(
                "FSL-ANNOTATION-TARGET",
                "annotation cannot attach to this declaration",
                span,
            ));
        }
        Ok(())
    }

    fn domain(&mut self) -> Result<DomainSpec, ParseError> {
        let loc = self.loc();
        self.expect_ident_value("domain")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut implementation_profile = None;
        let mut types = Vec::new();
        let mut aggregates = Vec::new();
        let mut effects = Vec::new();
        let mut awaits = Vec::new();
        let mut sagas = Vec::new();
        let mut projections = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.eat_ident("implementation_profile") {
                self.expect_no_pending_annotations()?;
                implementation_profile = Some(self.expect_ident()?);
                if self.eat_symbol("{") {
                    while !self.eat_symbol("}") {
                        self.expect_ident()?;
                        self.expect_ident()?;
                        self.eat_symbol(";");
                    }
                }
                self.eat_symbol(";");
            } else if self.peek_ident("type") {
                self.expect_no_pending_annotations()?;
                types.push(self.domain_type()?);
            } else if self.peek_ident("enum") {
                self.expect_no_pending_annotations()?;
                types.push(self.domain_enum()?);
            } else if self.peek_ident("value_object") {
                self.expect_no_pending_annotations()?;
                types.push(self.value_object()?);
            } else if self.peek_ident("aggregate") {
                self.expect_no_pending_annotations()?;
                let (aggregate, nested_projections) = self.aggregate()?;
                aggregates.push(aggregate);
                projections.extend(nested_projections);
            } else if self.peek_ident("effect") {
                effects.push(self.effect()?);
            } else if self.peek_ident("await") {
                self.expect_no_pending_annotations()?;
                awaits.push(self.await_block()?);
            } else if self.peek_ident("saga") {
                self.expect_no_pending_annotations()?;
                sagas.push(self.saga()?);
            } else if self.peek_ident("projection") {
                projections.push(self.projection()?);
            } else {
                return Err(self.error("expected domain declaration"));
            }
        }
        Ok(DomainSpec {
            name,
            implementation_profile,
            types,
            aggregates,
            effects,
            awaits,
            sagas,
            projections,
            loc,
        })
    }

    fn domain_type(&mut self) -> Result<DomainType, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("=")?;
        let first = self.expression(true)?;
        if self.eat_symbol("|") {
            let first = expression_name(first)?;
            let mut member_spans = vec![first.span];
            let mut members = vec![first.text];
            loop {
                let member = self.expect_syntax_ident()?;
                member_spans.push(member.span);
                members.push(member.text);
                if !self.eat_symbol("|") {
                    break;
                }
            }
            self.eat_symbol(";");
            let span = join(loc.span(), self.previous_span());
            return Ok(DomainType {
                name,
                kind: "enum".to_owned(),
                members,
                member_spans,
                lo: None,
                hi: None,
                fields: Vec::new(),
                invariants: Vec::new(),
                source_form: DomainTypeSourceForm::LegacyEnumUnion,
                span,
                loc,
            });
        }
        self.expect_symbol("..")?;
        let hi = self.expression(true)?;
        self.eat_symbol(";");
        let span = join(loc.span(), self.previous_span());
        Ok(DomainType {
            name,
            kind: "range".to_owned(),
            members: Vec::new(),
            member_spans: Vec::new(),
            lo: Some(first),
            hi: Some(hi),
            fields: Vec::new(),
            invariants: Vec::new(),
            source_form: DomainTypeSourceForm::CanonicalRange,
            span,
            loc,
        })
    }

    fn domain_enum(&mut self) -> Result<DomainType, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut members = Vec::new();
        let mut member_spans = Vec::new();
        while !self.eat_symbol("}") {
            let member = self.expect_syntax_ident()?;
            member_spans.push(member.span);
            members.push(member.text);
            if self.eat_symbol("}") {
                break;
            }
            self.expect_symbol(",")?;
        }
        self.eat_symbol(";");
        let span = join(loc.span(), self.previous_span());
        Ok(DomainType {
            name,
            kind: "enum".to_owned(),
            members,
            member_spans,
            lo: None,
            hi: None,
            fields: Vec::new(),
            invariants: Vec::new(),
            source_form: DomainTypeSourceForm::CanonicalEnum,
            span,
            loc,
        })
    }

    fn value_object(&mut self) -> Result<DomainType, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut fields = Vec::new();
        let mut invariants = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.peek_ident("invariant") {
                invariants.push(self.invariant()?);
            } else {
                self.expect_no_pending_annotations()?;
                fields.push(self.field(false, true)?);
            }
        }
        let span = join(loc.span(), self.previous_span());
        Ok(DomainType {
            name,
            kind: "value_object".to_owned(),
            members: Vec::new(),
            member_spans: Vec::new(),
            lo: None,
            hi: None,
            fields,
            invariants,
            source_form: DomainTypeSourceForm::ValueObject,
            span,
            loc,
        })
    }

    fn aggregate(&mut self) -> Result<(DomainAggregate, Vec<DomainProjection>), ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut id_type = None;
        let mut state = Vec::new();
        let mut commands = Vec::new();
        let mut events = Vec::new();
        let mut errors = Vec::new();
        let mut decides = Vec::new();
        let mut evolves = Vec::new();
        let mut invariants = Vec::new();
        let mut projections = Vec::new();
        let mut stale_policies = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.eat_ident("id") {
                self.expect_no_pending_annotations()?;
                id_type = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("state") {
                self.expect_no_pending_annotations()?;
                self.expect_symbol("{")?;
                while !self.eat_symbol("}") {
                    state.push(self.field(false, true)?);
                }
            } else if self.peek_ident("command") {
                commands.push(self.command()?);
            } else if self.peek_ident("event") {
                self.expect_no_pending_annotations()?;
                events.push(self.event()?);
            } else if self.peek_ident("error") {
                self.expect_no_pending_annotations()?;
                let error_loc = self.loc();
                self.bump();
                errors.push(DomainError {
                    name: self.expect_ident()?,
                    loc: error_loc,
                });
                self.eat_symbol(";");
            } else if self.peek_ident("decide") {
                decides.push(self.decide()?);
            } else if self.peek_ident("evolve") {
                evolves.push(self.evolve()?);
            } else if self.peek_ident("invariant") {
                invariants.push(self.invariant()?);
            } else if self.peek_ident("projection") {
                projections.push(self.projection()?);
            } else if self.peek_ident("on_stale") {
                self.expect_no_pending_annotations()?;
                stale_policies.push(self.stale_policy()?);
            } else {
                return Err(self.error("expected aggregate declaration"));
            }
        }
        Ok((
            DomainAggregate {
                name,
                id_type,
                state,
                commands,
                events,
                errors,
                decides,
                evolves,
                invariants,
                stale_policies,
                loc,
            },
            projections,
        ))
    }

    fn command(&mut self) -> Result<DomainCommand, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut inputs = Vec::new();
        while !self.eat_symbol("}") {
            inputs.push(self.field(true, false)?);
        }
        Ok(DomainCommand {
            name,
            inputs,
            annotations,
            loc,
        })
    }

    fn event(&mut self) -> Result<DomainEvent, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut fields = Vec::new();
        while !self.eat_symbol("}") {
            fields.push(self.field(true, false)?);
        }
        Ok(DomainEvent { name, fields, loc })
    }

    fn field(&mut self, allow_input: bool, allow_default: bool) -> Result<DomainField, ParseError> {
        let loc = self.loc();
        let start = self.peek().span;
        if allow_input {
            self.eat_ident("input");
        }
        let name = self.expect_syntax_ident()?;
        self.expect_symbol(":")?;
        let type_name = self.type_ref()?;
        let default = if allow_default && self.eat_symbol("=") {
            Some(self.expression(true)?)
        } else {
            None
        };
        self.eat_symbol(";");
        let span = join(start, self.previous_span());
        Ok(DomainField {
            name,
            type_name,
            default,
            span,
            loc,
        })
    }

    fn type_ref(&mut self) -> Result<SyntaxTypeExpr, ParseError> {
        let name = self.expect_syntax_ident()?;
        let start = name.span;
        if !self.eat_symbol("<") {
            return Ok(SyntaxTypeExpr::name(name));
        }
        let mut arguments = vec![self.type_ref()?];
        if self.eat_symbol(",") {
            arguments.push(self.type_ref()?);
        }
        self.expect_symbol(">")?;
        Ok(SyntaxTypeExpr::apply(
            name,
            arguments,
            join(start, self.previous_span()),
        ))
    }

    fn decide(&mut self) -> Result<DomainDecide, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let command = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut requires = Vec::new();
        let mut rejects = Vec::new();
        let mut emits = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("requires") {
                requires.push(self.requires()?);
            } else if self.peek_ident("rejects") {
                let reject_loc = self.loc();
                self.bump();
                let error = self.expect_ident()?;
                self.expect_ident_value("when")?;
                rejects.push(DomainReject {
                    error,
                    condition: self.expression(true)?,
                    loc: reject_loc,
                });
                self.eat_symbol(";");
            } else if self.peek_ident("emits") {
                emits.extend(self.emits()?);
            } else {
                return Err(self.error("expected decide declaration"));
            }
        }
        Ok(DomainDecide {
            command,
            requires,
            rejects,
            emits,
            annotations,
            loc,
        })
    }

    fn evolve(&mut self) -> Result<DomainEvolve, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let event = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut requires = Vec::new();
        let mut assignments = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("requires") {
                requires.push(self.requires()?);
            } else {
                assignments.push(self.assignment()?);
            }
        }
        Ok(DomainEvolve {
            event,
            requires,
            assignments,
            annotations,
            loc,
        })
    }

    fn assignment(&mut self) -> Result<DomainAssignment, ParseError> {
        let loc = self.loc();
        let start = self.peek().span;
        let target = self.lvalue()?;
        self.expect_symbol("=")?;
        let value = self.expression(true)?;
        self.eat_symbol(";");
        Ok(DomainAssignment {
            target,
            value,
            span: join(start, self.previous_span()),
            loc,
        })
    }

    fn invariant(&mut self) -> Result<DomainInvariant, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        let start = self.peek().span;
        self.bump();
        let name = self.expect_syntax_ident()?;
        self.expect_symbol("{")?;
        let expr = self.expression(true)?;
        self.expect_symbol("}")?;
        Ok(DomainInvariant {
            name,
            expr,
            span: join(start, self.previous_span()),
            annotations,
            loc,
        })
    }

    fn projection(&mut self) -> Result<DomainProjection, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut source = String::new();
        let mut fields = Vec::new();
        while !self.eat_symbol("}") {
            if self.eat_ident("from") {
                source = self.expect_ident()?;
                self.eat_symbol(";");
            } else if self.eat_ident("fields") {
                fields.extend(self.bracket_names()?);
                self.eat_symbol(";");
            } else {
                return Err(self.error("expected projection declaration"));
            }
        }
        Ok(DomainProjection {
            name,
            source,
            fields,
            annotations,
            loc,
        })
    }

    fn stale_policy(&mut self) -> Result<DomainStalePolicy, ParseError> {
        let loc = self.loc();
        self.bump();
        let event = self.expect_ident()?;
        self.expect_ident_value("when")?;
        let condition = self.expression(true)?;
        self.expect_symbol("{")?;
        let mut emits = Vec::new();
        while !self.eat_symbol("}") {
            emits.extend(self.emits()?);
        }
        Ok(DomainStalePolicy {
            event,
            condition,
            emits,
            loc,
        })
    }

    fn effect(&mut self) -> Result<DomainEffect, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut effect = DomainEffect::new(name, loc);
        effect.annotations = annotations;
        while !self.eat_symbol("}") {
            if self.eat_ident("async") {
                effect.async_effect = true;
                self.eat_symbol(";");
            } else if self.peek_ident("reliable") || self.peek_ident("irreversible") {
                let kind = self.expect_ident()?;
                let value = if self.eat_ident("false") {
                    false
                } else {
                    self.eat_ident("true");
                    true
                };
                if kind == "reliable" {
                    effect.reliable = value;
                } else {
                    effect.irreversible = value;
                }
                self.eat_symbol(";");
            } else if self.eat_ident("idempotency_key") {
                effect.idempotency_key = Some(self.effect_reference()?);
                self.eat_symbol(";");
            } else if self.eat_ident("correlation_id") {
                effect.correlation_id = Some(self.effect_reference()?);
                self.eat_symbol(";");
            } else if self.eat_ident("handles") {
                effect.handles = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.peek_ident("emits") {
                effect.outcomes.extend(self.emits()?);
            } else if self.eat_ident("request_event") {
                effect.request_event = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("success_event") {
                effect.success_event = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("failure_event") {
                effect.failure_event = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("timeout_event") {
                effect.timeout_event = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.peek_ident("retry") {
                effect.retry = self.retry()?;
            } else if self.eat_ident("timeout") {
                self.expect_ident_value("after")?;
                effect.timeout_after = Some(self.time_value()?);
                self.expect_ident_value("emits")?;
                let event = self.expect_ident()?;
                effect.timeout_event.get_or_insert_with(|| event.clone());
                push_unique(&mut effect.outcomes, event);
                self.eat_symbol(";");
            } else if self.eat_ident("compensation") {
                self.expect_symbol("{")?;
                while !self.eat_symbol("}") {
                    effect.compensation_events.extend(self.emits()?);
                }
            } else if self.eat_ident("outbox") {
                effect.outbox = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("inbox") {
                effect.inbox = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.peek_ident("input") || self.field_ahead() {
                self.field(true, false)?;
            } else {
                return Err(self.error("expected effect declaration"));
            }
        }
        for event in [
            effect.success_event.clone(),
            effect.failure_event.clone(),
            effect.timeout_event.clone(),
        ]
        .into_iter()
        .flatten()
        {
            push_unique(&mut effect.outcomes, event);
        }
        if effect.handles.is_none() {
            effect.handles.clone_from(&effect.request_event);
        }
        Ok(effect)
    }

    fn retry(&mut self) -> Result<DomainRetry, ParseError> {
        let loc = self.loc();
        self.bump();
        self.expect_symbol("{")?;
        let mut retry = DomainRetry {
            max_attempts: None,
            backoff: None,
            loc: Some(loc),
        };
        while !self.eat_symbol("}") {
            if self.eat_ident("max_attempts") {
                retry.max_attempts = Some(self.expect_int()?);
            } else if self.eat_ident("backoff") {
                retry.backoff = Some(self.expect_ident()?);
            } else {
                return Err(self.error("expected retry declaration"));
            }
            self.eat_symbol(";");
        }
        Ok(retry)
    }

    fn await_block(&mut self) -> Result<DomainAwait, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut mode = "one_of".to_owned();
        let mut events = Vec::new();
        let mut branches = Vec::new();
        while !self.eat_symbol("}") {
            if self.eat_ident("waits_for") {
                mode = self.await_mode()?;
                events = self.bracket_names()?;
                self.eat_symbol(";");
            } else if self.eat_ident("on") {
                let source = self.expect_ident()?;
                self.expect_symbol("->")?;
                branches.push((source, self.expect_ident()?));
                self.eat_symbol(";");
            } else {
                return Err(self.error("expected await declaration"));
            }
        }
        Ok(DomainAwait {
            name,
            mode,
            events,
            branches,
            loc,
        })
    }

    fn saga(&mut self) -> Result<DomainSaga, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut starts_on = None;
        let mut steps = Vec::new();
        let mut compensations = Vec::new();
        let mut invariants = Vec::new();
        let mut outboxes = Vec::new();
        let mut inboxes = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.eat_ident("starts_on") {
                self.expect_no_pending_annotations()?;
                starts_on = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.peek_ident("step") {
                steps.push(self.saga_step()?);
            } else if self.eat_ident("compensation") {
                self.expect_no_pending_annotations()?;
                self.expect_symbol("{")?;
                while !self.eat_symbol("}") {
                    self.take_leading_annotations()?;
                    self.expect_no_pending_annotations()?;
                    compensations.push(self.saga_compensation()?);
                }
            } else if self.peek_ident("invariant") {
                invariants.push(self.invariant()?);
            } else if self.eat_ident("outbox") {
                self.expect_no_pending_annotations()?;
                outboxes.push(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("inbox") {
                self.expect_no_pending_annotations()?;
                inboxes.push(self.expect_ident()?);
                self.eat_symbol(";");
            } else {
                return Err(self.error("expected saga declaration"));
            }
        }
        Ok(DomainSaga {
            name,
            starts_on,
            steps,
            compensations,
            invariants,
            outboxes,
            inboxes,
            loc,
        })
    }

    fn saga_step(&mut self) -> Result<DomainSagaStep, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut step = DomainSagaStep {
            name,
            async_step: false,
            requires: Vec::new(),
            emits: Vec::new(),
            awaits_mode: "one_of".to_owned(),
            awaits: Vec::new(),
            timeout_after: None,
            timeout_event: None,
            annotations,
            loc,
        };
        while !self.eat_symbol("}") {
            if self.eat_ident("async") {
                step.async_step = true;
                self.eat_symbol(";");
            } else if self.peek_ident("requires") {
                step.requires.push(self.requires()?);
            } else if self.peek_ident("emits") {
                step.emits.extend(self.emits()?);
            } else if self.eat_ident("awaits") {
                step.awaits_mode = self.await_mode()?;
                step.awaits = self.bracket_names()?;
                self.eat_symbol(";");
            } else if self.eat_ident("timeout") {
                self.expect_ident_value("after")?;
                step.timeout_after = Some(self.time_value()?);
                self.expect_ident_value("emits")?;
                step.timeout_event = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else {
                return Err(self.error("expected saga step declaration"));
            }
        }
        Ok(step)
    }

    fn saga_compensation(&mut self) -> Result<DomainSagaCompensation, ParseError> {
        let loc = self.loc();
        self.expect_ident_value("when")?;
        let trigger_event = self.expect_ident()?;
        self.expect_ident_value("after")?;
        let after_event = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut emits = Vec::new();
        while !self.eat_symbol("}") {
            emits.extend(self.emits()?);
        }
        Ok(DomainSagaCompensation {
            trigger_event,
            after_event,
            emits,
            loc,
        })
    }

    fn requires(&mut self) -> Result<SyntaxExpr, ParseError> {
        self.bump();
        let value = self.expression(true)?;
        self.eat_symbol(";");
        Ok(value)
    }

    fn emits(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect_ident_value("emits")?;
        self.eat_ident("one_of");
        let names = if self.peek_symbol("[") {
            self.bracket_names()?
        } else {
            self.line_names()?
        };
        self.eat_symbol(";");
        Ok(names)
    }

    fn await_mode(&mut self) -> Result<String, ParseError> {
        let mode = self.expect_ident()?;
        if matches!(mode.as_str(), "one_of" | "all" | "any") {
            Ok(mode)
        } else {
            Err(self.error("expected await mode"))
        }
    }

    fn bracket_names(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect_symbol("[")?;
        let mut names = vec![self.expect_ident()?];
        while self.eat_symbol(",") {
            if self.peek_symbol("]") {
                break;
            }
            names.push(self.expect_ident()?);
        }
        self.expect_symbol("]")?;
        Ok(names)
    }

    fn line_names(&mut self) -> Result<Vec<String>, ParseError> {
        let line = self.peek().span.start.line;
        let mut names = vec![self.expect_ident()?];
        while self.peek_symbol(",") && self.peek().span.start.line == line {
            self.bump();
            if self.peek().span.start.line != line {
                break;
            }
            names.push(self.expect_ident()?);
        }
        Ok(names)
    }

    fn time_value(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        let TokenKind::Int(value) = token.kind else {
            return Err(ParseError::new("expected time value", token.span));
        };
        let mut output = value.to_string();
        if let TokenKind::Ident(unit) = &self.peek().kind {
            if token.span.end.offset == self.peek().span.start.offset {
                output.push_str(unit);
                self.bump();
            }
        }
        Ok(output)
    }

    fn field_ahead(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
            && matches!(self.peek_n(1).kind, TokenKind::Symbol(ref symbol) if symbol == ":")
    }

    fn expression(&mut self, line_terminated: bool) -> Result<SyntaxExpr, ParseError> {
        parse_tokens_expression(
            &self.tokens,
            &mut self.cursor,
            ExpressionMode::Domain,
            line_terminated,
        )
    }

    fn effect_reference(&mut self) -> Result<SyntaxExpr, ParseError> {
        let expression = self.expression(true)?;
        if is_dotted_reference(&expression) {
            Ok(expression)
        } else {
            Err(ParseError::new(
                "effect reference must be a dotted identifier path",
                expression.span,
            ))
        }
    }

    fn lvalue(&mut self) -> Result<SyntaxLValue, ParseError> {
        parse_tokens_lvalue(&self.tokens, &mut self.cursor, ExpressionMode::Domain)
    }

    fn loc(&self) -> DomainLoc {
        self.peek().span.into()
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn peek_n(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.cursor + offset)
            .unwrap_or_else(|| self.tokens.last().expect("lexer emits EOF"))
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
    }

    fn previous_span(&self) -> Span {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or_else(|| self.peek().span, |token| token.span)
    }

    fn peek_ident(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == expected)
    }

    fn peek_symbol(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Symbol(value) if value == expected)
    }

    fn eat_ident(&mut self, expected: &str) -> bool {
        if self.peek_ident(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_symbol(&mut self, expected: &str) -> bool {
        if self.peek_symbol(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(value) => Ok(value),
            _ => Err(ParseError::new("expected identifier", token.span)),
        }
    }

    fn expect_syntax_ident(&mut self) -> Result<SyntaxIdent, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(text) => Ok(SyntaxIdent {
                text,
                span: token.span,
            }),
            _ => Err(ParseError::new("expected identifier", token.span)),
        }
    }

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_ident(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn expect_int(&mut self) -> Result<i64, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(value),
            _ => Err(ParseError::new("expected integer", token.span)),
        }
    }

    fn expect_symbol(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_symbol(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError::new(message, self.peek().span)
    }
}

fn expression_name(expression: SyntaxExpr) -> Result<SyntaxIdent, ParseError> {
    let span = expression.span;
    match expression.kind {
        SyntaxExprKind::Name(name) => Ok(name),
        _ => Err(ParseError::new(
            "domain enum members must be identifiers",
            span,
        )),
    }
}

fn render_exprs(values: &[SyntaxExpr]) -> Vec<String> {
    values.iter().map(SyntaxExpr::render_source).collect()
}

fn render_optional_expr(value: Option<&SyntaxExpr>) -> Option<String> {
    value.map(SyntaxExpr::render_source)
}

fn is_dotted_reference(expression: &SyntaxExpr) -> bool {
    match &expression.kind {
        SyntaxExprKind::Name(_) => true,
        SyntaxExprKind::Field { receiver, .. } => is_dotted_reference(receiver),
        _ => false,
    }
}

fn join(first: Span, last: Span) -> Span {
    Span {
        start: first.start,
        end: last.end,
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}
