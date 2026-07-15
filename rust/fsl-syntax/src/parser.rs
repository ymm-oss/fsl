// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fmt;

use crate::annotation_parse;
use crate::syntax_expr::{ExpressionMode, parse_tokens_expression};
use crate::{
    AcceptanceExpectation, AcceptanceStep, ActionItem, ActionTarget, Annotations, Binder,
    BusinessGoalBody, BusinessItem, BusinessPolicyBody, ComposeItem, ControlAttribute,
    CorrespondenceOrigin, Expr, GovernanceArtifactRef, GovernanceDelegateItem, GovernanceItem,
    HelpfulAction, LValue, MapsClause, MetaTag, Param, PreservationItem, ProcessCover,
    ProcessField, ProcessFields, ProcessItem, ProcessTransition, QualifiedName, RefinementItem,
    RefinementParam, RequirementAction, RequirementActionItem, RequirementBlockItem,
    RequirementBranch, RequirementsItem, Span, SpecItem, Statement, SurfaceBusiness,
    SurfaceCompose, SurfaceDocument, SurfaceGovernance, SurfaceRefinement, SurfaceRequirements,
    SurfaceSpec, SymbolPath, SyncAction, SyncRef, SyntaxIdent, TimeItem, Token, TokenKind,
    TypeExpr, VerifyItem, lex,
};

fn join_span(start: Span, end: Span) -> Span {
    Span {
        start: start.start,
        end: end.end,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
    code: &'static str,
}

impl ParseError {
    #[must_use]
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self::coded("FSL-PARSE", message, span)
    }

    #[must_use]
    pub fn coded(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            code,
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl From<crate::LexError> for ParseError {
    fn from(error: crate::LexError) -> Self {
        Self::new(error.message, error.span)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}:{}",
            self.message, self.span.start.line, self.span.start.column
        )
    }
}

impl std::error::Error for ParseError {}

/// Parse one standalone FSL expression.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical analysis fails or the token sequence is
/// not accepted by the FSL expression grammar.
pub fn parse_expr(source: &str) -> Result<Expr, ParseError> {
    let tokens = lex(source).map_err(ParseError::from)?;
    let mut parser = Parser::new(tokens, 0);
    let expr = parser.expression(0)?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after expression"));
    }
    Ok(expr)
}

/// Parse one kernel `spec` without semantic lowering.
///
/// # Errors
///
/// Returns [`ParseError`] when the source is not a syntactically valid kernel
/// spec. Other top-level dialects are rejected by this phase-0 entrypoint.
pub fn parse_surface_spec(source: &str) -> Result<SurfaceSpec, ParseError> {
    let tokens = lex(source).map_err(ParseError::from)?;
    let mut parser = Parser::new(tokens, 0);
    let spec = parser.surface_spec()?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after spec"));
    }
    Ok(spec)
}

/// Parse a supported shared-grammar surface document.
///
/// # Errors
///
/// Returns [`ParseError`] for invalid syntax or a top-level dialect that has
/// not yet reached the Phase-0 parser gate.
pub fn parse_surface_document(source: &str) -> Result<SurfaceDocument, ParseError> {
    crate::parse_document(crate::SourceFile::new(source)).map(|parsed| parsed.surface)
}

pub(crate) fn parse_shared_tokens(
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    let mut parser = Parser::new(tokens, cursor);
    let document = if parser.peek_ident("spec") {
        SurfaceDocument::Spec(parser.surface_spec()?)
    } else if parser.peek_ident("refinement") {
        SurfaceDocument::Refinement(parser.surface_refinement()?)
    } else if parser.peek_ident("business") {
        SurfaceDocument::Business(parser.surface_business()?)
    } else if parser.peek_ident("governance") {
        SurfaceDocument::Governance(parser.surface_governance()?)
    } else if parser.peek_ident("requirements") {
        SurfaceDocument::Requirements(parser.surface_requirements()?)
    } else if parser.peek_ident("compose") {
        SurfaceDocument::Compose(parser.surface_compose()?)
    } else {
        return Err(parser.error("unsupported shared-grammar top-level declaration"));
    };
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after surface document"));
    }
    Ok(document)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    pending_annotations: Annotations,
}

impl Parser {
    fn new(tokens: Vec<Token>, cursor: usize) -> Self {
        Self {
            tokens,
            cursor,
            pending_annotations: Annotations::default(),
        }
    }

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
    /// Returns [`ParseError`] when the drained group fails validation (empty
    /// requirement ID, reserved `undecided` ID, conflicting requirement
    /// text, etc.).
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

    fn surface_spec(&mut self) -> Result<SurfaceSpec, ParseError> {
        self.expect_ident_value("spec")?;
        let name = self.expect_ident()?;
        let meta = self.take_meta();
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item = self.spec_item()?;
            self.expect_no_pending_annotations()?;
            items.push(item);
        }
        if self.peek_ident("verify") {
            items.push(self.verify_bounds()?);
        }
        Ok(SurfaceSpec { name, meta, items })
    }

    fn surface_refinement(&mut self) -> Result<SurfaceRefinement, ParseError> {
        self.expect_ident_value("refinement")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            items.push(self.refinement_item(CorrespondenceOrigin::RefinementFile)?);
        }
        Ok(SurfaceRefinement { name, items })
    }

    fn surface_business(&mut self) -> Result<SurfaceBusiness, ParseError> {
        self.expect_ident_value("business")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item = self.business_item()?;
            self.expect_no_pending_annotations()?;
            items.push(item);
        }
        if self.peek_ident("verify") {
            let SpecItem::VerifyBounds {
                items: verify_items,
                span,
            } = self.verify_bounds()?
            else {
                unreachable!()
            };
            items.push(BusinessItem::VerifyBounds {
                items: verify_items,
                span,
            });
        }
        Ok(SurfaceBusiness { name, items })
    }

    fn surface_governance(&mut self) -> Result<SurfaceGovernance, ParseError> {
        self.expect_ident_value("governance")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            items.push(self.governance_item()?);
        }
        Ok(SurfaceGovernance { name, items })
    }

    fn surface_compose(&mut self) -> Result<SurfaceCompose, ParseError> {
        self.expect_ident_value("compose")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item = self.compose_item()?;
            self.expect_no_pending_annotations()?;
            items.push(item);
        }
        Ok(SurfaceCompose { name, items })
    }

    fn compose_item(&mut self) -> Result<ComposeItem, ParseError> {
        if self.peek_ident("use") {
            let span = self.bump().span;
            let spec_name = self.expect_ident()?;
            self.expect_ident_value("as")?;
            let alias = self.expect_ident()?;
            self.expect_ident_value("from")?;
            return Ok(ComposeItem::Use {
                spec_name,
                alias,
                path: self.expect_string()?,
                span,
            });
        }
        if self.peek_ident("internal") {
            let span = self.bump().span;
            let alias = self.expect_ident()?;
            self.expect_symbol(".")?;
            return Ok(ComposeItem::Internal {
                alias,
                action: self.expect_ident()?,
                span,
            });
        }
        if self.peek_ident("fair") || self.peek_ident("action") {
            if self.action_is_sync() {
                return Ok(ComposeItem::SyncAction(self.sync_action()?));
            }
            return Ok(ComposeItem::Common(self.action_item()?));
        }
        if [
            "def",
            "state",
            "init",
            "invariant",
            "trans",
            "reachable",
            "until",
            "unless",
            "leadsTo",
        ]
        .iter()
        .any(|keyword| self.peek_ident(keyword))
        {
            return Ok(ComposeItem::Common(self.spec_item()?));
        }
        Err(self.error("expected compose declaration"))
    }

    fn action_is_sync(&self) -> bool {
        let mut index = self.cursor;
        if matches!(&self.peek_n(index - self.cursor).kind, TokenKind::Ident(value) if value == "fair")
        {
            index += 1;
        }
        index += 2;
        if !matches!(
            self.tokens.get(index).map(|token| &token.kind),
            Some(TokenKind::Symbol(symbol)) if symbol == "("
        ) {
            return false;
        }
        let mut depth = 0_u32;
        while let Some(token) = self.tokens.get(index) {
            match &token.kind {
                TokenKind::Symbol(symbol) if symbol == "(" => depth += 1,
                TokenKind::Symbol(symbol) if symbol == ")" => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.tokens.get(index + 1).map(|next| &next.kind),
                            Some(TokenKind::Symbol(symbol)) if symbol == "="
                        );
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    fn sync_action(&mut self) -> Result<SyncAction, ParseError> {
        let span = self.peek().span;
        let fair = self.eat_ident("fair");
        self.expect_ident_value("action")?;
        let name = self.expect_ident()?;
        self.expect_symbol("(")?;
        let params = self.params()?;
        self.expect_symbol("=")?;
        let mut refs = vec![self.sync_ref()?];
        while self.eat_symbol("||") {
            refs.push(self.sync_ref()?);
        }
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("requires") || self.peek_ident("ensures") {
                let token = self.bump().clone();
                let expr = self.expression(0)?;
                items.push(
                    if matches!(&token.kind, TokenKind::Ident(value) if value == "requires") {
                        ActionItem::Requires(expr, token.span)
                    } else {
                        ActionItem::Ensures(expr, token.span)
                    },
                );
            } else if self.peek_ident("let") {
                let item_span = self.bump().span;
                let name = self.expect_ident()?;
                self.expect_symbol("=")?;
                items.push(ActionItem::Let(name, self.expression(0)?, item_span));
            } else {
                items.push(ActionItem::Statement(self.statement()?));
            }
        }
        Ok(SyncAction {
            name,
            params,
            refs,
            items,
            span,
            fair,
            meta,
            annotations,
        })
    }

    fn sync_ref(&mut self) -> Result<SyncRef, ParseError> {
        let alias = self.expect_ident()?;
        self.expect_symbol(".")?;
        let action = self.expect_ident()?;
        self.expect_symbol("(")?;
        Ok(SyncRef {
            alias,
            action,
            args: self.expression_list(")")?,
        })
    }

    fn surface_requirements(&mut self) -> Result<SurfaceRequirements, ParseError> {
        self.expect_ident_value("requirements")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item = self.requirements_item()?;
            self.expect_no_pending_annotations()?;
            items.push(item);
        }
        if self.peek_ident("verify") {
            items.push(RequirementsItem::Common(self.verify_bounds()?));
        }
        Ok(SurfaceRequirements { name, items })
    }

    fn requirements_item(&mut self) -> Result<RequirementsItem, ParseError> {
        if self.peek_ident("implements") {
            return self.requirements_implements();
        }
        if self.peek_ident("requirement") {
            return self.requirement_block();
        }
        if self.peek_ident("acceptance") {
            return self.acceptance_block();
        }
        if self.peek_ident("forbidden") {
            return self.forbidden_block();
        }
        if self.peek_ident("process") {
            return Ok(RequirementsItem::Process(self.business_process()?));
        }
        if self.peek_ident("kpi") {
            return Ok(RequirementsItem::Kpi(self.business_item()?));
        }
        if self.peek_ident("fair") || self.peek_ident("action") {
            return Ok(RequirementsItem::Action(self.requirement_action()?));
        }
        if self.peek_ident("time") {
            return self.requirements_time();
        }
        if self.common_spec_item_starts() {
            return Ok(RequirementsItem::Common(self.spec_item()?));
        }
        Err(self.error("expected requirements declaration"))
    }

    fn requirements_implements(&mut self) -> Result<RequirementsItem, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_ident_value("from")?;
        let path = self.expect_string()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("impl") || self.peek_ident("abs") {
                return Err(self.error("impl and abs are not valid inside implements"));
            }
            items.push(self.refinement_item(CorrespondenceOrigin::ImplementsBlock)?);
        }
        Ok(RequirementsItem::Implements {
            name,
            path,
            items,
            span,
        })
    }

    fn requirement_block(&mut self) -> Result<RequirementsItem, ParseError> {
        let start = self.bump().span;
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        let span = join_span(start, text_span);
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.peek_ident("fair") || self.peek_ident("action") {
                items.push(RequirementBlockItem::Action(self.requirement_action()?));
            } else if self.peek_ident("deadline") {
                let deadline_span = self.bump().span;
                let name = self.expect_ident()?;
                self.expect_symbol("<=")?;
                items.push(RequirementBlockItem::Deadline {
                    name,
                    bound: self.expression(0)?,
                    span: deadline_span,
                });
            } else if self.requirement_property_starts() {
                items.push(RequirementBlockItem::Property(self.spec_item()?));
            } else {
                return Err(self.error("expected requirement declaration"));
            }
            self.expect_no_pending_annotations()?;
        }
        Ok(RequirementsItem::Requirement {
            id,
            text,
            items,
            span,
            annotations,
        })
    }

    fn requirement_action(&mut self) -> Result<RequirementAction, ParseError> {
        let span = self.peek().span;
        let fair = self.eat_ident("fair");
        self.expect_ident_value("action")?;
        let name = self.expect_ident()?;
        self.expect_symbol("(")?;
        let params = self.params()?;
        let maps = if self.peek_ident("maps") {
            Some(self.maps_clause()?)
        } else {
            None
        };
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("requires") || self.peek_ident("ensures") {
                let token = self.bump().clone();
                let expr = self.expression(0)?;
                let item = if matches!(&token.kind, TokenKind::Ident(value) if value == "requires")
                {
                    ActionItem::Requires(expr, token.span)
                } else {
                    ActionItem::Ensures(expr, token.span)
                };
                items.push(RequirementActionItem::Action(Box::new(item)));
            } else if self.peek_ident("let") {
                let let_span = self.bump().span;
                let name = self.expect_ident()?;
                self.expect_symbol("=")?;
                items.push(RequirementActionItem::Action(Box::new(ActionItem::Let(
                    name,
                    self.expression(0)?,
                    let_span,
                ))));
            } else if self.peek_ident("branches") {
                items.push(self.requirement_branches()?);
            } else {
                items.push(RequirementActionItem::Action(Box::new(
                    ActionItem::Statement(self.statement()?),
                )));
            }
        }
        Ok(RequirementAction {
            name,
            params,
            items,
            span,
            fair,
            meta,
            maps,
            annotations,
        })
    }

    fn maps_clause(&mut self) -> Result<MapsClause, ParseError> {
        let span = self.bump().span;
        let target = if self.eat_ident("stutter") {
            ActionTarget::Stutter
        } else {
            let name = self.expect_ident()?;
            self.expect_symbol("(")?;
            ActionTarget::Action(name, self.expression_list(")")?)
        };
        Ok(MapsClause { target, span })
    }

    fn requirement_branches(&mut self) -> Result<RequirementActionItem, ParseError> {
        let span = self.bump().span;
        self.expect_symbol("{")?;
        let mut branches = Vec::new();
        while !self.eat_symbol("}") {
            let branch_span = self.peek().span;
            self.expect_ident_value("when")?;
            let condition = self.expression(0)?;
            self.expect_symbol("{")?;
            let statements = self.statement_list()?;
            let maps = self.maps_clause()?;
            branches.push(RequirementBranch {
                condition,
                statements,
                maps,
                span: branch_span,
            });
        }
        Ok(RequirementActionItem::Branches { branches, span })
    }

    fn acceptance_block(&mut self) -> Result<RequirementsItem, ParseError> {
        let start = self.bump().span;
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        let span = join_span(start, text_span);
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut steps = Vec::new();
        while !self.peek_ident("expect") {
            steps.push(self.acceptance_step()?);
        }
        let expect_span = self.bump().span;
        let expectation = if matches!(self.peek().kind, TokenKind::Ident(_))
            && matches!(self.peek_n(1).kind, TokenKind::Int(_))
        {
            let entity = self.expect_ident()?;
            let TokenKind::Int(instance) = self.bump().kind else {
                unreachable!()
            };
            self.expect_ident_value("in")?;
            AcceptanceExpectation::Stage {
                entity,
                instance,
                stage: self.expect_ident()?,
                span: expect_span,
            }
        } else {
            AcceptanceExpectation::Expr(self.expression(0)?, expect_span)
        };
        self.expect_symbol("}")?;
        Ok(RequirementsItem::Acceptance {
            id,
            text,
            steps,
            expectation,
            span,
            annotations,
        })
    }

    fn forbidden_block(&mut self) -> Result<RequirementsItem, ParseError> {
        let start = self.bump().span;
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        let span = join_span(start, text_span);
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut steps = Vec::new();
        while !self.peek_ident("expect") {
            steps.push(self.acceptance_step()?);
        }
        self.bump();
        self.expect_ident_value("rejected")?;
        self.expect_symbol("}")?;
        Ok(RequirementsItem::Forbidden {
            id,
            text,
            steps,
            span,
            annotations,
        })
    }

    fn acceptance_step(&mut self) -> Result<AcceptanceStep, ParseError> {
        let span = self.peek().span;
        let name = self.expect_ident()?;
        self.expect_symbol("(")?;
        Ok(AcceptanceStep {
            name,
            args: self.expression_list(")")?,
            span,
        })
    }

    fn requirements_time(&mut self) -> Result<RequirementsItem, ParseError> {
        let span = self.bump().span;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("urgent") {
                let item_span = self.bump().span;
                let mut names = vec![self.expect_ident()?];
                while self.eat_symbol(",") {
                    if self.peek_symbol("}") || self.peek_ident("urgent") || self.peek_ident("age")
                    {
                        break;
                    }
                    names.push(self.expect_ident()?);
                }
                items.push(TimeItem::Urgent(names, item_span));
            } else if self.peek_ident("age") {
                let item_span = self.bump().span;
                let name = self.expect_ident()?;
                let binder = if self.eat_symbol("[") {
                    let binder = self.binder()?;
                    self.expect_symbol("]")?;
                    Some(binder)
                } else {
                    None
                };
                self.expect_ident_value("while")?;
                items.push(TimeItem::Age {
                    name,
                    binder,
                    condition: self.expression(0)?,
                    span: item_span,
                });
            } else {
                return Err(self.error("expected time declaration"));
            }
        }
        Ok(RequirementsItem::Time { items, span })
    }

    fn common_spec_item_starts(&self) -> bool {
        [
            "const",
            "def",
            "symmetric",
            "type",
            "enum",
            "struct",
            "entity",
            "number",
            "state",
            "init",
            "invariant",
            "trans",
            "reachable",
            "terminal",
            "until",
            "unless",
            "leadsTo",
        ]
        .iter()
        .any(|keyword| self.peek_ident(keyword))
    }

    fn requirement_property_starts(&self) -> bool {
        [
            "invariant",
            "trans",
            "reachable",
            "until",
            "unless",
            "leadsTo",
        ]
        .iter()
        .any(|keyword| self.peek_ident(keyword))
    }

    fn governance_item(&mut self) -> Result<GovernanceItem, ParseError> {
        if self.peek_ident("authority") {
            let span = self.bump().span;
            let authority = self.expect_ident()?;
            self.expect_ident_value("owns")?;
            let control_ids = self.open_req_id_list()?;
            return Ok(GovernanceItem::Authority {
                authority,
                control_ids,
                span,
            });
        }
        if self.peek_ident("control") {
            let BusinessItem::Control {
                id,
                text,
                attributes,
                span,
            } = self.business_control()?
            else {
                unreachable!()
            };
            return Ok(GovernanceItem::Control {
                id,
                text,
                attributes,
                span,
            });
        }
        if self.peek_ident("delegates") {
            return self.governance_delegates();
        }
        if self.peek_ident("preservation") {
            return self.governance_preservation();
        }
        Err(self.error("expected governance declaration"))
    }

    fn governance_delegates(&mut self) -> Result<GovernanceItem, ParseError> {
        let span = self.bump().span;
        let business_name = self.expect_ident()?;
        self.expect_ident_value("from")?;
        let path = self.expect_string()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            let item_span = self.peek().span;
            if self.eat_ident("require") {
                items.push(GovernanceDelegateItem::Require(self.req_id()?, item_span));
                continue;
            }
            let control_id = self.req_id()?;
            self.expect_ident_value("is")?;
            self.expect_ident_value("satisfied_by")?;
            let mut artifacts = vec![self.governance_artifact_ref()?];
            while self.eat_symbol(",") {
                if self.peek_symbol("}") || self.peek_ident("require") {
                    break;
                }
                artifacts.push(self.governance_artifact_ref()?);
            }
            items.push(GovernanceDelegateItem::Satisfaction {
                control_id,
                artifacts,
                span: item_span,
            });
        }
        Ok(GovernanceItem::Delegates {
            business_name,
            path,
            items,
            span,
        })
    }

    fn governance_artifact_ref(&mut self) -> Result<GovernanceArtifactRef, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(kind) if kind == "policy" => {
                Ok(GovernanceArtifactRef::Policy(self.req_id()?, token.span))
            }
            TokenKind::Ident(kind) if kind == "goal" => {
                Ok(GovernanceArtifactRef::Goal(self.req_id()?, token.span))
            }
            _ => Err(ParseError::new(
                "expected policy or goal reference",
                token.span,
            )),
        }
    }

    fn governance_preservation(&mut self) -> Result<GovernanceItem, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            let item_span = self.peek().span;
            if self.eat_ident("before") {
                let spec_name = self.expect_ident()?;
                self.expect_ident_value("from")?;
                items.push(PreservationItem::Before {
                    spec_name,
                    path: self.expect_string()?,
                    span: item_span,
                });
            } else if self.eat_ident("after") {
                let spec_name = self.expect_ident()?;
                self.expect_ident_value("from")?;
                items.push(PreservationItem::After {
                    spec_name,
                    path: self.expect_string()?,
                    span: item_span,
                });
            } else if self.eat_ident("preserve") {
                items.push(PreservationItem::Preserve(self.req_id()?, item_span));
            } else if self.eat_ident("checked_by") {
                self.expect_ident_value("refinement")?;
                items.push(PreservationItem::Refinement(
                    self.expect_string()?,
                    item_span,
                ));
            } else {
                return Err(self.error("expected preservation declaration"));
            }
        }
        Ok(GovernanceItem::Preservation { name, items, span })
    }

    fn business_item(&mut self) -> Result<BusinessItem, ParseError> {
        if self.peek_ident("actor") {
            let span = self.bump().span;
            return Ok(BusinessItem::Actor(self.open_ident_list()?, span));
        }
        if self.peek_ident("entity") {
            let span = self.bump().span;
            return Ok(BusinessItem::Entity(self.expect_ident()?, span));
        }
        if self.peek_ident("process") {
            return self.business_process();
        }
        if self.peek_ident("kpi") {
            let span = self.bump().span;
            let name = self.expect_ident()?;
            self.expect_symbol("=")?;
            self.expect_ident_value("count")?;
            let case_name = self.expect_ident()?;
            self.expect_ident_value("in")?;
            return Ok(BusinessItem::Kpi {
                name,
                case_name,
                stage: self.expect_ident()?,
                span,
            });
        }
        if self.peek_ident("control") {
            return self.business_control();
        }
        if self.peek_ident("policy") {
            return self.business_policy();
        }
        if self.peek_ident("goal") {
            return self.business_goal();
        }
        Err(self.error("expected business declaration"))
    }

    fn business_process(&mut self) -> Result<BusinessItem, ParseError> {
        let span = self.bump().span;
        let mut segments = Vec::new();
        loop {
            let segment_span = self.peek().span;
            segments.push(SyntaxIdent {
                text: self.expect_ident()?,
                span: segment_span,
            });
            if !self.eat_symbol(".") {
                break;
            }
        }
        let path_span = join_span(
            segments.first().expect("process path is non-empty").span,
            segments.last().expect("process path is non-empty").span,
        );
        let name = SymbolPath::from_idents(segments, path_span)
            .map_err(|error| ParseError::new(error.message, error.span))?;
        let fields = if self.peek_ident("with") {
            let fields_span = self.bump().span;
            let mut fields = Vec::new();
            loop {
                let field_start = self.peek().span;
                let field_name = self.expect_ident()?;
                self.expect_symbol(":")?;
                let type_start = self.peek().span;
                let type_name = self.qualified_name()?;
                let type_span = join_span(type_start, self.last_span());
                let initial = if self.eat_symbol("=") {
                    Some(self.expression(0)?)
                } else {
                    None
                };
                fields.push(ProcessField {
                    name: field_name,
                    type_name,
                    initial,
                    span: join_span(field_start, self.last_span()),
                    type_span,
                });
                if !self.eat_symbol(",") {
                    break;
                }
            }
            Some(ProcessFields {
                fields,
                span: fields_span,
            })
        } else {
            None
        };
        self.expect_no_pending_annotations()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item = self.process_item()?;
            self.expect_no_pending_annotations()?;
            items.push(item);
        }
        Ok(BusinessItem::Process {
            name,
            fields,
            items,
            span,
        })
    }

    fn process_item(&mut self) -> Result<ProcessItem, ParseError> {
        if self.peek_ident("stages") {
            let span = self.bump().span;
            return Ok(ProcessItem::Stages(self.open_ident_list()?, span));
        }
        if self.peek_ident("initial") {
            let span = self.bump().span;
            return Ok(ProcessItem::Initial(self.expect_ident()?, span));
        }
        if !self.peek_ident("transition") {
            return Err(self.error("expected process declaration"));
        }
        let span = self.bump().span;
        let name = self.expect_ident()?;
        let source = self.expect_ident()?;
        self.expect_symbol("->")?;
        let target = self.expect_ident()?;
        self.expect_ident_value("by")?;
        let actor = self.expect_ident()?;

        let mut inputs = Vec::new();
        if self.eat_ident("with") {
            loop {
                inputs.push(self.param()?);
                if !self.eat_symbol(",") {
                    break;
                }
            }
        }
        let guard = if self.eat_ident("when") {
            Some(self.expression(0)?)
        } else {
            None
        };
        let mut assignments = Vec::new();
        if self.eat_ident("set") {
            loop {
                let assignment_name = self.expect_ident()?;
                self.expect_symbol("=")?;
                assignments.push((assignment_name, self.expression(0)?));
                if !self.eat_symbol(",") {
                    break;
                }
            }
        }
        let covers = if self.peek_ident("covers") {
            let start = self.bump().span;
            let id = self.req_id()?;
            let token = self.bump().clone();
            let TokenKind::String(text) = token.kind else {
                return Err(ParseError::new("expected string literal", token.span));
            };
            Some(ProcessCover {
                id,
                text,
                span: join_span(start, token.span),
            })
        } else {
            None
        };
        let annotations = self.take_annotations()?;
        Ok(ProcessItem::Transition(Box::new(ProcessTransition {
            name,
            source,
            target,
            actor,
            inputs,
            guard,
            assignments,
            covers,
            span,
            annotations,
        })))
    }

    fn business_control(&mut self) -> Result<BusinessItem, ParseError> {
        let span = self.bump().span;
        let id = self.req_id()?;
        let text = self.expect_string()?;
        let mut attributes = Vec::new();
        loop {
            if self.eat_ident("owner") {
                attributes.push(ControlAttribute::Owner(self.expect_ident()?));
            } else if self.eat_ident("severity") {
                attributes.push(ControlAttribute::Severity(self.expect_ident()?));
            } else if self.eat_ident("applies_to") {
                attributes.push(ControlAttribute::AppliesTo(self.expect_ident()?));
            } else {
                break;
            }
        }
        Ok(BusinessItem::Control {
            id,
            text,
            attributes,
            span,
        })
    }

    fn satisfies_clause(&mut self) -> Result<Vec<String>, ParseError> {
        if !self.eat_ident("satisfies") {
            return Ok(Vec::new());
        }
        let mut ids = vec![self.req_id()?];
        while self.eat_symbol(",") {
            if self.business_body_starts() {
                break;
            }
            ids.push(self.req_id()?);
        }
        Ok(ids)
    }

    fn business_policy(&mut self) -> Result<BusinessItem, ParseError> {
        let span = self.bump().span;
        let id = self.req_id()?;
        let text = self.expect_string()?;
        let satisfies = self.satisfies_clause()?;
        let body = if self.eat_ident("invariant") {
            self.expect_symbol("{")?;
            let expr = self.expression(0)?;
            self.expect_symbol("}")?;
            BusinessPolicyBody::Invariant(expr)
        } else if self.eat_ident("responds") {
            self.expect_symbol("{")?;
            let mut binders = Vec::new();
            let (before, after, within) = self.leads_to_body(&mut binders)?;
            self.expect_symbol("}")?;
            BusinessPolicyBody::Responds {
                binders,
                before: Box::new(before),
                after: Box::new(after),
                within: within.map(Box::new),
            }
        } else if self.eat_ident("every") {
            let case_name = self.expect_ident()?;
            if self.eat_ident("in") {
                let source_stage = self.expect_ident()?;
                self.expect_ident_value("must")?;
                self.expect_ident_value("eventually")?;
                self.expect_ident_value("be")?;
                BusinessPolicyBody::Eventually {
                    case_name,
                    source_stage,
                    target_stages: self.stage_disjunction()?,
                }
            } else {
                self.expect_ident_value("reaching")?;
                let target_stages = self.stage_disjunction()?;
                self.expect_ident_value("must")?;
                self.expect_ident_value("have")?;
                self.expect_ident_value("passed")?;
                self.expect_ident_value("through")?;
                BusinessPolicyBody::Precedence {
                    case_name,
                    target_stages,
                    waypoints: self.stage_disjunction()?,
                }
            }
        } else {
            return Err(self.error("expected business policy body"));
        };
        Ok(BusinessItem::Policy {
            id,
            text,
            body: Box::new(body),
            span,
            satisfies,
            annotations: self.take_annotations()?,
        })
    }

    fn business_goal(&mut self) -> Result<BusinessItem, ParseError> {
        let span = self.bump().span;
        let id = self.req_id()?;
        let text = self.expect_string()?;
        let satisfies = self.satisfies_clause()?;
        let body = if self.eat_symbol("{") {
            let expr = self.expression(0)?;
            self.expect_symbol("}")?;
            BusinessGoalBody::Expr(expr)
        } else if self.eat_ident("some") {
            let case_name = self.expect_ident()?;
            self.expect_ident_value("can")?;
            self.expect_ident_value("reach")?;
            BusinessGoalBody::SomeStage {
                case_name,
                stage: self.expect_ident()?,
            }
        } else if self.eat_ident("all") {
            let case_name = self.expect_ident()?;
            self.expect_ident_value("can")?;
            self.expect_ident_value("be")?;
            BusinessGoalBody::AllStage {
                case_name,
                stages: self.stage_disjunction()?,
            }
        } else {
            return Err(self.error("expected business goal body"));
        };
        Ok(BusinessItem::Goal {
            id,
            text,
            body,
            span,
            satisfies,
            annotations: self.take_annotations()?,
        })
    }

    fn stage_disjunction(&mut self) -> Result<Vec<String>, ParseError> {
        let mut stages = vec![self.expect_ident()?];
        while self.eat_ident("or") {
            stages.push(self.expect_ident()?);
        }
        Ok(stages)
    }

    fn business_body_starts(&self) -> bool {
        ["invariant", "responds", "every", "some", "all"]
            .iter()
            .any(|keyword| self.peek_ident(keyword))
            || self.peek_symbol("{")
    }

    fn refinement_item(
        &mut self,
        origin: CorrespondenceOrigin,
    ) -> Result<RefinementItem, ParseError> {
        if self.eat_ident("impl") {
            return Ok(RefinementItem::Impl(self.expect_ident()?));
        }
        if self.eat_ident("abs") {
            return Ok(RefinementItem::Abs(self.expect_ident()?));
        }
        if self.peek_ident("maps") {
            let span = self.bump().span;
            self.expect_ident_value("auto")?;
            return Ok(RefinementItem::MapsAuto(span));
        }
        if self.peek_ident("map") {
            let span = self.bump().span;
            let name = self.expect_ident()?;
            let binder = if self.eat_symbol("[") {
                let binder = self.binder()?;
                self.expect_symbol("]")?;
                Some(binder)
            } else {
                None
            };
            self.expect_symbol("=")?;
            return Ok(RefinementItem::Map {
                name,
                binder,
                expr: Box::new(self.expression(0)?),
                span,
            });
        }
        if self.peek_ident("action") {
            let span = self.bump().span;
            let name = self.expect_ident()?;
            self.expect_symbol("(")?;
            let mut params = Vec::new();
            if !self.eat_symbol(")") {
                loop {
                    let param_name = self.expect_ident()?;
                    let ty = if self.eat_symbol(":") {
                        Some(self.type_expr()?)
                    } else {
                        None
                    };
                    params.push(RefinementParam {
                        name: param_name,
                        ty,
                    });
                    if self.eat_symbol(")") {
                        break;
                    }
                    self.expect_symbol(",")?;
                }
            }
            self.expect_symbol("->")?;
            let target = if self.eat_ident("stutter") {
                ActionTarget::Stutter
            } else {
                let target_name = self.expect_ident()?;
                self.expect_symbol("(")?;
                ActionTarget::Action(target_name, self.expression_list(")")?)
            };
            return Ok(RefinementItem::Action {
                name,
                params,
                target,
                origin,
                span,
            });
        }
        if self.peek_ident("preserve") {
            let span = self.bump().span;
            self.expect_ident_value("progress")?;
            self.expect_symbol("{")?;
            let mut responses = Vec::new();
            while !self.eat_symbol("}") {
                let response_span = self.peek().span;
                self.expect_ident_value("respond")?;
                let name = self.expect_ident()?;
                self.expect_ident_value("by")?;
                let mut actions = vec![self.expect_ident()?];
                while self.eat_symbol(",") {
                    if self.peek_ident("respond") || self.peek_symbol("}") {
                        break;
                    }
                    actions.push(self.expect_ident()?);
                }
                responses.push((name, actions, response_span));
            }
            return Ok(RefinementItem::PreserveProgress { responses, span });
        }
        Err(self.error("expected refinement item"))
    }

    #[allow(clippy::too_many_lines)]
    fn spec_item(&mut self) -> Result<SpecItem, ParseError> {
        if self.peek_ident("const") {
            self.bump();
            let name = self.expect_ident()?;
            self.expect_symbol("=")?;
            return Ok(SpecItem::Const {
                name,
                value: Box::new(self.expression(0)?),
            });
        }
        if self.peek_ident("def") {
            return self.def_item();
        }
        let symmetric = if self.peek_ident("symmetric") {
            self.bump();
            true
        } else {
            false
        };
        if self.peek_ident("type") {
            self.bump();
            let name = self.expect_ident()?;
            self.expect_symbol("=")?;
            let lo = self.expression(0)?;
            self.expect_symbol("..")?;
            let hi = self.expression(0)?;
            return Ok(SpecItem::Type {
                name,
                lo: Box::new(lo),
                hi: Box::new(hi),
                symmetric,
            });
        }
        if self.peek_ident("enum") {
            self.bump();
            let name = self.expect_ident()?;
            self.expect_symbol("{")?;
            let members = self.ident_list("}")?;
            return Ok(SpecItem::Enum {
                name,
                members,
                symmetric,
            });
        }
        if symmetric {
            return Err(self.error("symmetric must precede type or enum"));
        }
        if self.peek_ident("struct") {
            self.bump();
            let name = self.expect_ident()?;
            self.expect_symbol("{")?;
            let fields = self.field_list()?;
            return Ok(SpecItem::Struct { name, fields });
        }
        if self.peek_ident("entity") || self.peek_ident("number") {
            let token = self.bump().clone();
            let TokenKind::Ident(kind) = token.kind else {
                unreachable!()
            };
            let name = self.expect_ident()?;
            return Ok(if kind == "entity" {
                SpecItem::Entity(name, token.span)
            } else {
                SpecItem::Number(name, token.span)
            });
        }
        if self.peek_ident("state") {
            self.bump();
            self.expect_symbol("{")?;
            let declarations = self.state_field_list()?;
            return Ok(SpecItem::State(declarations));
        }
        if self.peek_ident("init") {
            self.bump();
            let meta = self.take_meta();
            let annotations = self.take_annotations()?;
            self.expect_symbol("{")?;
            return Ok(SpecItem::Init {
                statements: self.statement_list()?,
                meta,
                annotations,
            });
        }
        if self.peek_ident("fair") || self.peek_ident("action") {
            return self.action_item();
        }
        if self.peek_ident("invariant") {
            return self.simple_property("invariant");
        }
        if self.peek_ident("trans") {
            return self.simple_property("trans");
        }
        if self.peek_ident("reachable") {
            return self.simple_property("reachable");
        }
        if self.peek_ident("terminal") {
            let span = self.bump().span;
            self.expect_symbol("{")?;
            let expr = self.expression(0)?;
            self.expect_symbol("}")?;
            return Ok(SpecItem::Terminal {
                expr: Box::new(expr),
                span,
            });
        }
        if self.peek_ident("until") || self.peek_ident("unless") {
            return self.binary_temporal_property();
        }
        if self.peek_ident("leadsTo") {
            return self.leads_to_item();
        }
        Err(self.error("expected kernel spec declaration"))
    }

    fn def_item(&mut self) -> Result<SpecItem, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("(")?;
        let mut params = Vec::new();
        if !self.eat_symbol(")") {
            loop {
                let param_name = self.expect_ident()?;
                self.expect_symbol(":")?;
                params.push((param_name, self.qualified_name()?));
                if self.eat_symbol(")") {
                    break;
                }
                self.expect_symbol(",")?;
            }
        }
        self.expect_symbol("=")?;
        Ok(SpecItem::Def {
            name,
            params,
            value: Box::new(self.expression(0)?),
            span,
        })
    }

    fn field_list(&mut self) -> Result<Vec<(String, TypeExpr)>, ParseError> {
        let mut fields = Vec::new();
        if self.eat_symbol("}") {
            return Ok(fields);
        }
        loop {
            let name = self.expect_ident()?;
            self.expect_symbol(":")?;
            fields.push((name, self.type_expr()?));
            if self.eat_symbol("}") {
                return Ok(fields);
            }
            self.expect_symbol(",")?;
            if self.eat_symbol("}") {
                return Ok(fields);
            }
        }
    }

    fn state_field_list(&mut self) -> Result<Vec<crate::StateField>, ParseError> {
        let mut fields = Vec::new();
        if self.eat_symbol("}") {
            return Ok(fields);
        }
        loop {
            let start = self.peek().span;
            let name = self.expect_ident()?;
            self.expect_symbol(":")?;
            let ty = self.type_expr()?;
            let (initializer, initializer_span) = if self.eat_symbol("=") {
                let initializer_start = self.peek().span;
                let initializer = self.expression(0)?;
                let initializer_end = self.last_span();
                (
                    Some(initializer),
                    Some(join_span(initializer_start, initializer_end)),
                )
            } else {
                (None, None)
            };
            fields.push(crate::StateField {
                name,
                ty,
                initializer,
                span: join_span(start, self.last_span()),
                initializer_span,
            });
            if self.eat_symbol("}") {
                return Ok(fields);
            }
            self.expect_symbol(",")?;
            if self.eat_symbol("}") {
                return Ok(fields);
            }
        }
    }

    fn type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        if self.eat_ident("Int") {
            return Ok(TypeExpr::Int);
        }
        if self.eat_ident("Bool") {
            return Ok(TypeExpr::Bool);
        }
        if self.eat_ident("relation") {
            let source = self.type_expr()?;
            self.expect_symbol("->")?;
            let target = self.type_expr()?;
            return Ok(TypeExpr::Relation(Box::new(source), Box::new(target)));
        }
        for (keyword, arity) in [("Map", 2_u8), ("Set", 1), ("Seq", 2), ("Option", 1)] {
            if self.eat_ident(keyword) {
                self.expect_symbol("<")?;
                let first = self.type_expr()?;
                let result = match (keyword, arity) {
                    ("Map", 2) => {
                        self.expect_symbol(",")?;
                        let second = self.type_expr()?;
                        TypeExpr::Map(Box::new(first), Box::new(second))
                    }
                    ("Seq", 2) => {
                        self.expect_symbol(",")?;
                        let cap = match self.bump().clone().kind {
                            TokenKind::Int(value) => Expr::Num(value),
                            TokenKind::Ident(name) => Expr::Var(name),
                            _ => return Err(self.error("expected Seq capacity")),
                        };
                        TypeExpr::Seq(Box::new(first), cap)
                    }
                    ("Set", 1) => TypeExpr::Set(Box::new(first)),
                    ("Option", 1) => TypeExpr::Option(Box::new(first)),
                    _ => unreachable!(),
                };
                self.expect_symbol(">")?;
                return Ok(result);
            }
        }
        if let TokenKind::Ident(name) = &self.peek().kind {
            if self.next_is_type_delimiter() {
                let name = name.clone();
                self.bump();
                return Ok(TypeExpr::Name(name));
            }
        }
        let lo = self.expression(0)?;
        self.expect_symbol("..")?;
        let hi = self.expression(0)?;
        Ok(TypeExpr::Range(lo, hi))
    }

    fn next_is_type_delimiter(&self) -> bool {
        matches!(
            &self.peek_n(1).kind,
            TokenKind::Symbol(symbol) if matches!(symbol.as_str(), "," | ")" | ">" | "}" | "->" | "=")
        )
    }

    fn action_item(&mut self) -> Result<SpecItem, ParseError> {
        let span = self.peek().span;
        let fair = self.eat_ident("fair");
        self.expect_ident_value("action")?;
        let name = self.expect_ident()?;
        self.expect_symbol("(")?;
        let params = self.params()?;
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            if self.peek_ident("requires") || self.peek_ident("ensures") {
                let token = self.bump().clone();
                let expr = self.expression(0)?;
                items.push(
                    if matches!(token.kind, TokenKind::Ident(ref value) if value == "requires") {
                        ActionItem::Requires(expr, token.span)
                    } else {
                        ActionItem::Ensures(expr, token.span)
                    },
                );
            } else if self.peek_ident("let") {
                let span = self.bump().span;
                let name = self.expect_ident()?;
                self.expect_symbol("=")?;
                items.push(ActionItem::Let(name, self.expression(0)?, span));
            } else {
                items.push(ActionItem::Statement(self.statement()?));
            }
        }
        Ok(SpecItem::Action {
            name,
            params,
            items,
            span,
            fair,
            meta,
            sync: false,
            annotations,
        })
    }

    fn params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.eat_symbol(")") {
            return Ok(params);
        }
        loop {
            params.push(self.param()?);
            if self.eat_symbol(")") {
                return Ok(params);
            }
            self.expect_symbol(",")?;
        }
    }

    fn param(&mut self) -> Result<Param, ParseError> {
        let name = self.expect_ident()?;
        if self.eat_symbol(":") {
            Ok(Param::Typed(name, self.qualified_name()?))
        } else {
            self.expect_ident_value("in")?;
            let lo = self.expression(0)?;
            self.expect_symbol("..")?;
            Ok(Param::Range(name, lo, self.expression(0)?))
        }
    }

    fn statement_list(&mut self) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();
        while !self.eat_symbol("}") {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Statement, ParseError> {
        if self.peek_ident("if") {
            let span = self.bump().span;
            let condition = self.expression(0)?;
            self.expect_symbol("{")?;
            let then_statements = self.statement_list()?;
            let else_statements = if self.eat_ident("else") {
                self.expect_symbol("{")?;
                self.statement_list()?
            } else {
                Vec::new()
            };
            return Ok(Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            });
        }
        if self.peek_ident("forall") {
            let span = self.bump().span;
            let binder = self.binder()?;
            self.eat_symbol(":");
            self.expect_symbol("{")?;
            return Ok(Statement::ForAll {
                binder,
                statements: self.statement_list()?,
                span,
            });
        }
        let span = self.peek().span;
        let name = self.expect_ident()?;
        let mut target = if self.eat_symbol("[") {
            let index = self.expression(0)?;
            self.expect_symbol("]")?;
            LValue::Index(name, index)
        } else {
            LValue::Var(name)
        };
        if self.eat_symbol(".") {
            target = LValue::Field(Box::new(target), self.expect_ident()?);
        }
        self.expect_symbol("=")?;
        Ok(Statement::Assign {
            target,
            value: self.expression(0)?,
            span,
        })
    }

    fn simple_property(&mut self, kind: &str) -> Result<SpecItem, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let expr = self.expression(0)?;
        self.expect_symbol("}")?;
        Ok(match kind {
            "invariant" => SpecItem::Invariant {
                name,
                expr: Box::new(expr),
                span,
                meta,
                annotations,
            },
            "trans" => SpecItem::Trans {
                name,
                expr: Box::new(expr),
                span,
                meta,
                annotations,
            },
            "reachable" => SpecItem::Reachable {
                name,
                expr: Box::new(expr),
                span,
                meta,
                annotations,
            },
            _ => unreachable!(),
        })
    }

    fn binary_temporal_property(&mut self) -> Result<SpecItem, ParseError> {
        let token = self.bump().clone();
        let TokenKind::Ident(kind) = token.kind else {
            unreachable!()
        };
        let name = self.expect_ident()?;
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let before = self.expression(0)?;
        self.expect_ident_value(&kind)?;
        let after = self.expression(0)?;
        self.expect_symbol("}")?;
        Ok(if kind == "until" {
            SpecItem::Until {
                name,
                before: Box::new(before),
                after: Box::new(after),
                span: token.span,
                meta,
                annotations,
            }
        } else {
            SpecItem::Unless {
                name,
                before: Box::new(before),
                after: Box::new(after),
                span: token.span,
                meta,
                annotations,
            }
        })
    }

    fn leads_to_item(&mut self) -> Result<SpecItem, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        let meta = self.take_meta();
        let annotations = self.take_annotations()?;
        self.expect_symbol("{")?;
        let mut binders = Vec::new();
        let (before, after, within) = self.leads_to_body(&mut binders)?;
        let mut decreases = None;
        let mut helpful = Vec::new();
        while !self.eat_symbol("}") {
            if self.eat_ident("decreases") {
                decreases = Some(self.expression(0)?);
            } else if self.peek_ident("helpful") {
                let helpful_span = self.bump().span;
                let action = self.expect_ident()?;
                self.expect_symbol("(")?;
                helpful.push(HelpfulAction {
                    action,
                    args: self.expression_list(")")?,
                    span: helpful_span,
                });
            } else {
                return Err(self.error("expected decreases, helpful, or '}' in leadsTo"));
            }
        }
        Ok(SpecItem::LeadsTo {
            name,
            binders,
            before: Box::new(before),
            after: Box::new(after),
            span,
            meta,
            decreases: decreases.map(Box::new),
            within: within.map(Box::new),
            helpful,
            annotations,
        })
    }

    fn leads_to_body(
        &mut self,
        binders: &mut Vec<Binder>,
    ) -> Result<(Expr, Expr, Option<Expr>), ParseError> {
        if self.eat_ident("forall") {
            binders.push(self.binder()?);
            self.eat_symbol(":");
            self.expect_symbol("{")?;
            let body = self.leads_to_body(binders)?;
            self.expect_symbol("}")?;
            return Ok(body);
        }
        let before = self.expression(0)?;
        self.expect_symbol("~>")?;
        let within = if self.eat_ident("within") {
            Some(self.expression(0)?)
        } else {
            None
        };
        let after = self.expression(0)?;
        Ok((before, after, within))
    }

    fn verify_bounds(&mut self) -> Result<SpecItem, ParseError> {
        let span = self.bump().span;
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            let token = self.bump().clone();
            let TokenKind::Ident(kind) = token.kind else {
                return Err(self.error("expected verify item"));
            };
            let name = self.expect_ident()?;
            self.expect_symbol("=")?;
            if kind == "instances" {
                let TokenKind::Int(value) = self.bump().clone().kind else {
                    return Err(self.error("instances bound must be an integer"));
                };
                items.push(VerifyItem::Instances(name, value, token.span));
            } else if kind == "values" {
                let lo = self.expression(0)?;
                self.expect_symbol("..")?;
                items.push(VerifyItem::Values(
                    name,
                    Box::new(lo),
                    Box::new(self.expression(0)?),
                    token.span,
                ));
            } else {
                return Err(ParseError::new("expected instances or values", token.span));
            }
            self.eat_symbol(";");
        }
        Ok(SpecItem::VerifyBounds { items, span })
    }

    fn take_meta(&mut self) -> Option<MetaTag> {
        let token = self.peek().clone();
        let TokenKind::String(value) = token.kind else {
            return None;
        };
        self.bump();
        Some(MetaTag::parse(&value, token.span))
    }

    fn ident_list(&mut self, close: &str) -> Result<Vec<String>, ParseError> {
        let mut values = Vec::new();
        if self.eat_symbol(close) {
            return Ok(values);
        }
        loop {
            values.push(self.expect_ident()?);
            if self.eat_symbol(close) {
                return Ok(values);
            }
            self.expect_symbol(",")?;
            if self.eat_symbol(close) {
                return Ok(values);
            }
        }
    }

    fn open_ident_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut values = vec![self.expect_ident()?];
        while self.eat_symbol(",") {
            if self.peek_symbol("}") || self.open_ident_list_ended() {
                break;
            }
            values.push(self.expect_ident()?);
        }
        Ok(values)
    }

    fn open_req_id_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut values = vec![self.req_id()?];
        while self.eat_symbol(",") {
            if self.peek_symbol("}") || self.governance_item_starts() {
                break;
            }
            values.push(self.req_id()?);
        }
        Ok(values)
    }

    fn governance_item_starts(&self) -> bool {
        ["authority", "control", "delegates", "preservation"]
            .iter()
            .any(|keyword| self.peek_ident(keyword))
    }

    fn open_ident_list_ended(&self) -> bool {
        [
            "actor",
            "entity",
            "process",
            "kpi",
            "control",
            "policy",
            "goal",
            "stages",
            "initial",
            "transition",
        ]
        .iter()
        .any(|keyword| self.peek_ident(keyword))
    }

    fn expression(&mut self, min_binding_power: u8) -> Result<Expr, ParseError> {
        debug_assert_eq!(
            min_binding_power, 0,
            "shared parser owns recursive precedence"
        );
        let mut cursor = self.cursor;
        let expression =
            parse_tokens_expression(&self.tokens, &mut cursor, ExpressionMode::Kernel, false)?;
        self.cursor = cursor;
        expression.into_kernel()
    }

    fn binder(&mut self) -> Result<Binder, ParseError> {
        let name = self.expect_ident()?;
        if self.eat_symbol(":") {
            let type_name = self.qualified_name()?;
            let where_expr = if self.eat_ident("where") {
                Some(Box::new(self.expression(0)?))
            } else {
                None
            };
            return Ok(Binder::Typed {
                name,
                type_name,
                where_expr,
            });
        }
        if !self.eat_ident("in") {
            return Err(self.error("expected ':' or 'in' in binder"));
        }
        let first = self.expression(0)?;
        if self.eat_symbol("..") {
            let hi = self.expression(0)?;
            return Ok(Binder::Range {
                name,
                lo: Box::new(first),
                hi: Box::new(hi),
            });
        }
        let where_expr = if self.eat_ident("where") {
            Some(Box::new(self.expression(0)?))
        } else {
            None
        };
        Ok(Binder::Collection {
            name,
            collection: Box::new(first),
            where_expr,
        })
    }

    fn qualified_name(&mut self) -> Result<QualifiedName, ParseError> {
        let first = self.expect_ident()?;
        if self.eat_symbol(".") {
            Ok(QualifiedName {
                namespace: Some(first),
                name: self.expect_ident()?,
            })
        } else {
            Ok(QualifiedName {
                namespace: None,
                name: first,
            })
        }
    }

    fn expression_list(&mut self, close: &str) -> Result<Vec<Expr>, ParseError> {
        let mut items = Vec::new();
        if self.eat_symbol(close) {
            return Ok(items);
        }
        loop {
            items.push(self.expression(0)?);
            if self.eat_symbol(close) {
                return Ok(items);
            }
            self.expect_symbol(",")?;
            if self.eat_symbol(close) {
                return Ok(items);
            }
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn last_span(&self) -> Span {
        self.tokens[self.cursor.saturating_sub(1)].span
    }

    fn peek_n(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.cursor + offset)
            .unwrap_or_else(|| self.tokens.last().expect("lexer always emits EOF"))
    }

    fn peek_ident(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == expected)
    }

    fn peek_symbol(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Symbol(value) if value == expected)
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
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
        if matches!(&self.peek().kind, TokenKind::Symbol(value) if value == expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_symbol(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        if let TokenKind::Ident(value) = token.kind {
            Ok(value)
        } else {
            Err(ParseError::new("expected identifier", token.span))
        }
    }

    fn expect_string(&mut self) -> Result<String, ParseError> {
        self.expect_string_with_span().map(|(value, _)| value)
    }

    fn expect_string_with_span(&mut self) -> Result<(String, Span), ParseError> {
        let token = self.bump().clone();
        if let TokenKind::String(value) = token.kind {
            Ok((value, token.span))
        } else {
            Err(ParseError::new("expected string literal", token.span))
        }
    }

    fn req_id(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        let mut value = match token.kind {
            TokenKind::Ident(value) => value,
            TokenKind::Int(value) => value.to_string(),
            _ => {
                return Err(ParseError::new(
                    "expected requirement identifier",
                    token.span,
                ));
            }
        };
        while self.peek_symbol("-") {
            let component = match &self.peek_n(1).kind {
                TokenKind::Ident(component) => component.clone(),
                TokenKind::Int(component) => component.to_string(),
                _ => break,
            };
            self.bump();
            self.bump();
            value.push('-');
            value.push_str(&component);
        }
        Ok(value)
    }

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_ident(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError::new(message, self.peek().span)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ast(source: &str) -> serde_json::Value {
        parse_expr(source).unwrap().python_ast()
    }

    #[test]
    fn precedence_matches_the_python_cascade() {
        assert_eq!(
            ast("1 + 2 * 3 == 7 and not false"),
            json!([
                "bin",
                "and",
                [
                    "bin",
                    "==",
                    ["bin", "+", ["num", 1], ["bin", "*", ["num", 2], ["num", 3]]],
                    ["num", 7]
                ],
                ["not", ["bool", false]]
            ])
        );
    }

    #[test]
    fn conditional_is_available_at_every_expression_precedence() {
        assert_eq!(
            ast("1 + if true then 2 else 3 * 4"),
            json!([
                "bin",
                "+",
                ["num", 1],
                [
                    "ite",
                    ["bool", true],
                    ["num", 2],
                    ["bin", "*", ["num", 3], ["num", 4]]
                ]
            ])
        );
        assert_eq!(
            ast("if true then if false then 1 else 2 else 3"),
            json!([
                "ite",
                ["bool", true],
                ["ite", ["bool", false], ["num", 1], ["num", 2]],
                ["num", 3]
            ])
        );
        assert_eq!(
            ast("max(if true then 1 else 2, 3)"),
            json!([
                "max",
                ["ite", ["bool", true], ["num", 1], ["num", 2]],
                ["num", 3]
            ])
        );
    }

    #[test]
    fn postfix_and_pattern_projection_matches_python() {
        assert_eq!(
            ast("orders[i].buyer is some(u)"),
            json!([
                "is",
                ["field", ["index", ["var", "orders"], ["var", "i"]], "buyer"],
                ["pat_some", "u"]
            ])
        );
    }

    #[test]
    fn quantifier_and_binder_projection_matches_python() {
        assert_eq!(
            ast("forall i: Item where i > 0 { stock[i] >= 0 }"),
            json!([
                "forall",
                [
                    "binder_typed",
                    "i",
                    "Item",
                    ["bin", ">", ["var", "i"], ["num", 0]]
                ],
                [
                    "bin",
                    ">=",
                    ["index", ["var", "stock"], ["var", "i"]],
                    ["num", 0]
                ]
            ])
        );
    }

    #[test]
    fn parses_a_typed_kernel_spec_surface() {
        let source = r#"spec Demo "MODEL: parser fixture" {
  type Id = 0..1
  state { x: Id }
  init { x = 0 }
  fair action inc(i: Id) "REQ-1: increment" {
    requires x < 1
    x = x + 1
    ensures x == old(x) + 1
  }
  invariant Bound { x <= 1 }
  reachable One { x == 1 }
}
verify { values Id = 0..2 }
"#;
        let ast = parse_surface_spec(source).unwrap().python_ast();
        assert_eq!(ast[0], "spec");
        assert_eq!(ast[1], "Demo");
        assert_eq!(
            ast[2][0],
            json!(["__spec_meta", {"id": "MODEL", "text": "parser fixture"}])
        );
        assert_eq!(ast[2][4][0], "action");
        assert_eq!(ast[2][4][5], true);
        assert_eq!(ast[2][7][0], "verify_bounds");
    }
}
