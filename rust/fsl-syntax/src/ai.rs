// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};

use crate::annotation_parse;
use crate::surface::SurfaceAgent;
use crate::{Annotations, ParseError, Span, Token, TokenKind, lex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AiLoc {
    pub line: u32,
    pub column: u32,
}

impl From<Span> for AiLoc {
    fn from(span: Span) -> Self {
        Self {
            line: span.start.line,
            column: span.start.column,
        }
    }
}

impl AiLoc {
    fn kernel_ast_v1(self) -> Value {
        json!({"line":self.line,"column":self.column})
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiTool {
    pub name: String,
    pub schema: Option<String>,
    pub irreversible: bool,
    pub preconditions: Vec<String>,
    pub effect: Option<String>,
    pub annotations: Annotations,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiAuthorityRule {
    pub name: String,
    pub annotations: Annotations,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AiAuthority {
    pub may_suggest: Vec<AiAuthorityRule>,
    pub may_execute: Vec<AiAuthorityRule>,
    pub requires_human_approval: Vec<AiAuthorityRule>,
    pub forbidden: Vec<AiAuthorityRule>,
    pub annotations: Annotations,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiFallback {
    pub reason: String,
    pub target: String,
    pub annotations: Annotations,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiCheckRule {
    pub name: String,
    pub annotations: Annotations,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AiHardCheck {
    pub rules: Vec<AiCheckRule>,
    pub annotations: Annotations,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiComponent {
    pub name: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub retriever: Option<String>,
    pub temperature: Option<f64>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    pub tools: Vec<AiTool>,
    pub authority: AiAuthority,
    pub fallback: Vec<AiFallback>,
    pub check: AiHardCheck,
    pub loc: AiLoc,
}

impl AiComponent {
    #[must_use]
    pub fn kernel_ast_v1(&self) -> Value {
        json!({
            "$type":"AiComponent","name":self.name,"model":self.model,"prompt":self.prompt,
            "retriever":self.retriever,"temperature":self.temperature,
            "input_schema":self.input_schema,"output_schema":self.output_schema,
            "tools":self.tools.iter().map(AiTool::kernel_ast_v1).collect::<Vec<_>>(),
            "authority":self.authority.kernel_ast_v1(),
            "fallback":self.fallback.iter().map(AiFallback::kernel_ast_v1).collect::<Vec<_>>(),
            "check":self.check.kernel_ast_v1(),"loc":self.loc.kernel_ast_v1(),
        })
    }
}

impl AiTool {
    fn kernel_ast_v1(&self) -> Value {
        json!({
            "$type":"AiTool","name":self.name,"schema":self.schema,
            "irreversible":self.irreversible,"preconditions":self.preconditions,
            "effect":self.effect,"loc":self.loc.map(AiLoc::kernel_ast_v1),
        })
    }
}

impl AiAuthorityRule {
    fn names(rules: &[Self]) -> Vec<&str> {
        rules.iter().map(|rule| rule.name.as_str()).collect()
    }
}

impl AiAuthority {
    fn kernel_ast_v1(&self) -> Value {
        json!({
            "$type":"AiAuthority","may_suggest":AiAuthorityRule::names(&self.may_suggest),
            "may_execute":AiAuthorityRule::names(&self.may_execute),
            "requires_human_approval":AiAuthorityRule::names(&self.requires_human_approval),
            "forbidden":AiAuthorityRule::names(&self.forbidden),"loc":self.loc.map(AiLoc::kernel_ast_v1),
        })
    }
}

// --- Recursive `agent` dialect IR (issue #468) -----------------------------
//
// These types are structurally distinct from `ai_component`'s IR above:
// `agent` bodies nest (via `children`), separate a bare `tools [X, Y];` list
// (`tool_names`) from full `tool X { ... }` blocks (`tools`), and add
// grant/orchestration/failure-policy/contract/trust/review-gate declarations
// that `ai_component` does not have.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiAgentGrant {
    /// `"authority"` or `"context"`.
    pub kind: String,
    pub names: Vec<String>,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiAgentOutput {
    pub name: String,
    pub visibility: Vec<String>,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiDelegationEdge {
    pub source: String,
    pub target: String,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiFailurePolicy {
    pub agent: String,
    pub condition: String,
    pub action: String,
    pub target: Option<String>,
    pub retry_limit: Option<i64>,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiAgentContract {
    pub hard_rules: Vec<String>,
    pub loc: AiLoc,
}

impl AiFallback {
    fn kernel_ast_v1(&self) -> Value {
        json!({"$type":"AiFallback","reason":self.reason,"target":self.target,"loc":self.loc.kernel_ast_v1()})
    }
}

impl AiHardCheck {
    fn kernel_ast_v1(&self) -> Value {
        json!({
            "$type":"AiHardCheck",
            "rules":self.rules.iter().map(|rule| rule.name.as_str()).collect::<Vec<_>>(),
            "loc":self.loc.map(AiLoc::kernel_ast_v1),
        })
    }
}

/// Parse one specialized `ai_component` source into typed frontend IR.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical or syntactic analysis fails.
pub fn parse_ai_component(source: &str) -> Result<AiComponent, ParseError> {
    let tokens = lex(source).map_err(ParseError::from)?;
    parse_ai_component_tokens(source, tokens, 0)
}

pub(crate) fn parse_ai_component_tokens(
    source: &str,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<AiComponent, ParseError> {
    let mut parser = AiParser {
        source,
        tokens,
        cursor,
        pending_annotations: Annotations::default(),
    };
    let component = parser.component()?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after ai_component"));
    }
    Ok(component)
}

/// Parse one recursive `agent { ... }` document (issue #468) into its typed
/// tree. This only enforces per-block grammar (including "declare at most
/// once" duplicate checks, mirroring the frozen reference's parse-time
/// transformer). Cross-node structural checks -- grant boundaries, unknown
/// orchestration/`review_gate`/`failure_policy` child references, output
/// visibility targets, and the graph-based finding kinds -- are a separate
/// pass over the parsed tree (`fsl_tools`' agent analyzer), matching
/// `docs/DESIGN-ai-hard.md`'s separation of parse/grammar from structural
/// analysis.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical or syntactic analysis fails.
pub(crate) fn parse_agent_tokens(
    source: &str,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceAgent, ParseError> {
    let mut parser = AiParser {
        source,
        tokens,
        cursor,
        pending_annotations: Annotations::default(),
    };
    let agent = parser.agent()?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after agent"));
    }
    Ok(agent)
}

struct AiParser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
    pending_annotations: Annotations,
}

impl AiParser<'_> {
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

    fn component(&mut self) -> Result<AiComponent, ParseError> {
        let loc = self.loc();
        self.expect_ident_value("ai_component")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut component = AiComponent {
            name,
            model: None,
            prompt: None,
            retriever: None,
            temperature: None,
            input_schema: None,
            output_schema: None,
            tools: Vec::new(),
            authority: AiAuthority::default(),
            fallback: Vec::new(),
            check: AiHardCheck::default(),
            loc,
        };
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            if self.eat_ident("model") {
                self.expect_no_pending_annotations()?;
                component.model = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("prompt") {
                self.expect_no_pending_annotations()?;
                component.prompt = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("retriever") {
                self.expect_no_pending_annotations()?;
                component.retriever = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("temperature") {
                self.expect_no_pending_annotations()?;
                component.temperature = Some(self.number()?);
                self.eat_symbol(";");
            } else if self.eat_ident("input") {
                self.expect_no_pending_annotations()?;
                component.input_schema = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("output") {
                self.expect_no_pending_annotations()?;
                component.output_schema = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("tools") {
                let annotations = self.take_annotations()?;
                for name in self.names()? {
                    component.tools.push(AiTool {
                        name,
                        schema: None,
                        irreversible: false,
                        preconditions: Vec::new(),
                        effect: None,
                        annotations: annotations.clone(),
                        loc: None,
                    });
                }
                self.eat_symbol(";");
            } else if self.peek_ident("tool") {
                component.tools.push(self.tool()?);
            } else if self.peek_ident("authority") {
                component.authority = self.authority()?;
            } else if self.peek_ident("fallback") {
                component.fallback.extend(self.fallback()?);
            } else if self.peek_ident("check") {
                component.check = self.check()?;
            } else {
                return Err(self.error("expected ai_component declaration"));
            }
        }
        Ok(component)
    }

    fn tool(&mut self) -> Result<AiTool, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        let irreversible = self.eat_ident("irreversible");
        self.expect_symbol("{")?;
        let mut tool = AiTool {
            name,
            schema: None,
            irreversible,
            preconditions: Vec::new(),
            effect: None,
            annotations,
            loc: Some(loc),
        };
        while !self.eat_symbol("}") {
            if self.eat_ident("schema") {
                tool.schema = Some(self.atom()?);
            } else if self.eat_ident("precondition") {
                tool.preconditions.push(self.expect_ident()?);
            } else if self.eat_ident("effect") {
                tool.effect = Some(self.expect_ident()?);
            } else {
                return Err(self.error("expected tool declaration"));
            }
            self.eat_symbol(";");
        }
        Ok(tool)
    }

    fn authority(&mut self) -> Result<AiAuthority, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        if !self.peek_symbol("{") {
            self.expect_ident()?;
        }
        self.expect_symbol("{")?;
        let mut authority = AiAuthority {
            loc: Some(loc),
            annotations,
            ..AiAuthority::default()
        };
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let rule_loc = self.loc();
            let kind = self.expect_ident()?;
            let rule_annotations = self.take_annotations()?;
            let names = self.names()?;
            let rules = names
                .into_iter()
                .map(|name| AiAuthorityRule {
                    name,
                    annotations: rule_annotations.clone(),
                    loc: Some(rule_loc),
                })
                .collect::<Vec<_>>();
            match kind.as_str() {
                "may_suggest" => authority.may_suggest.extend(rules),
                "may_execute" => authority.may_execute.extend(rules),
                "requires_human_approval" => authority.requires_human_approval.extend(rules),
                "forbidden" => authority.forbidden.extend(rules),
                _ => return Err(self.error("unknown authority declaration")),
            }
            self.eat_symbol(";");
        }
        Ok(authority)
    }

    fn fallback(&mut self) -> Result<Vec<AiFallback>, ParseError> {
        let block_annotations = self.take_annotations()?;
        self.bump();
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let item_annotations = self.take_annotations()?;
            let loc = self.loc();
            self.expect_ident_value("when")?;
            let reason = self.expect_ident()?;
            self.expect_ident_value("require")?;
            let mut annotations = block_annotations.clone();
            annotations.extend(item_annotations.source_order().iter().cloned());
            annotations.validate().map_err(|error| {
                ParseError::coded("FSL-ANNOTATION-INVALID", error.message, error.span)
            })?;
            items.push(AiFallback {
                reason,
                target: self.expect_ident()?,
                annotations,
                loc,
            });
            self.eat_symbol(";");
        }
        Ok(items)
    }

    fn check(&mut self) -> Result<AiHardCheck, ParseError> {
        let annotations = self.take_annotations()?;
        let loc = self.loc();
        self.bump();
        self.expect_ident_value("hard")?;
        self.expect_symbol("{")?;
        let mut rules = Vec::new();
        while !self.eat_symbol("}") {
            self.take_leading_annotations()?;
            let rule_loc = self.loc();
            let rule_annotations = self.take_annotations()?;
            self.expect_ident_value("rule")?;
            rules.push(AiCheckRule {
                name: self.expect_ident()?,
                annotations: rule_annotations,
                loc: Some(rule_loc),
            });
            self.eat_symbol(";");
        }
        Ok(AiHardCheck {
            rules,
            annotations,
            loc: Some(loc),
        })
    }

    /// Parse one `agent { ... }` body, recursing into nested `agent`
    /// declarations for `children`. `docs/LANGUAGE.md` §13.6: nested agents
    /// are ordinary agents scoped by their parent, not a distinct
    /// `sub_agent` type.
    #[allow(clippy::too_many_lines)]
    fn agent(&mut self) -> Result<SurfaceAgent, ParseError> {
        let start = self.peek().span;
        let loc: AiLoc = start.into();
        self.expect_ident_value("agent")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut agent = SurfaceAgent {
            name,
            span: start,
            model: None,
            prompt: None,
            context: Vec::new(),
            tool_names: Vec::new(),
            tools: Vec::new(),
            authority: AiAuthority::default(),
            grants: Vec::new(),
            outputs: Vec::new(),
            orchestration: Vec::new(),
            failure_policy: Vec::new(),
            contracts: Vec::new(),
            children: Vec::new(),
            trust: None,
            review_gates: Vec::new(),
            loc,
        };
        let (mut seen_authority, mut seen_context, mut seen_tools) = (false, false, false);
        let (mut seen_orchestration, mut seen_failure_policy) = (false, false);
        while !self.peek_symbol("}") {
            self.take_leading_annotations()?;
            if self.eat_ident("model") {
                self.expect_no_pending_annotations()?;
                if agent.model.is_some() {
                    return Err(self.error("agent may declare model at most once"));
                }
                agent.model = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("prompt") {
                self.expect_no_pending_annotations()?;
                if agent.prompt.is_some() {
                    return Err(self.error("agent may declare prompt at most once"));
                }
                agent.prompt = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("context") {
                self.expect_no_pending_annotations()?;
                if seen_context {
                    return Err(self.error("agent may declare context at most once"));
                }
                agent.context = self.names()?;
                seen_context = true;
                self.eat_symbol(";");
            } else if self.eat_ident("tools") {
                self.expect_no_pending_annotations()?;
                if seen_tools {
                    return Err(self.error("agent may declare tools at most once"));
                }
                agent.tool_names = self.names()?;
                seen_tools = true;
                self.eat_symbol(";");
            } else if self.peek_ident("tool") {
                agent.tools.push(self.tool()?);
            } else if self.eat_ident("trust") {
                self.expect_no_pending_annotations()?;
                if agent.trust.is_some() {
                    return Err(self.error("agent may declare trust at most once"));
                }
                agent.trust = Some(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.eat_ident("review_gate") {
                self.expect_no_pending_annotations()?;
                agent.review_gates.push(self.expect_ident()?);
                self.eat_symbol(";");
            } else if self.peek_ident("authority") {
                let authority = self.authority()?;
                if seen_authority {
                    return Err(self.error("agent may declare authority at most once"));
                }
                agent.authority = authority;
                seen_authority = true;
            } else if self.peek_ident("grant") {
                self.expect_no_pending_annotations()?;
                agent.grants.push(self.grant()?);
            } else if self.peek_ident("output") {
                agent.outputs.push(self.agent_output()?);
            } else if self.eat_ident("orchestration") {
                self.expect_no_pending_annotations()?;
                if seen_orchestration {
                    return Err(self.error("agent may declare orchestration at most once"));
                }
                agent.orchestration = self.orchestration()?;
                seen_orchestration = true;
            } else if self.eat_ident("failure_policy") {
                self.expect_no_pending_annotations()?;
                if seen_failure_policy {
                    return Err(self.error("agent may declare failure_policy at most once"));
                }
                agent.failure_policy = self.failure_policy()?;
                seen_failure_policy = true;
            } else if self.eat_ident("contract") {
                self.expect_no_pending_annotations()?;
                agent.contracts.push(self.agent_contract()?);
            } else if self.peek_ident("agent") {
                agent.children.push(self.agent()?);
            } else {
                return Err(self.error("expected agent declaration"));
            }
        }
        let end = self.peek().span;
        self.bump();
        agent.span = Span {
            start: start.start,
            end: end.end,
        };
        Ok(agent)
    }

    fn grant(&mut self) -> Result<AiAgentGrant, ParseError> {
        let loc = self.loc();
        self.bump();
        let kind = if self.eat_ident("authority") {
            "authority".to_owned()
        } else if self.eat_ident("context") {
            "context".to_owned()
        } else {
            return Err(self.error("expected 'authority' or 'context' after grant"));
        };
        let names = self.names()?;
        self.eat_symbol(";");
        Ok(AiAgentGrant { kind, names, loc })
    }

    fn agent_output(&mut self) -> Result<AiAgentOutput, ParseError> {
        let loc = self.loc();
        self.bump();
        let name = self.expect_ident()?;
        self.expect_ident_value("visibility")?;
        let visibility = self.names()?;
        self.eat_symbol(";");
        Ok(AiAgentOutput {
            name,
            visibility,
            loc,
        })
    }

    fn orchestration(&mut self) -> Result<Vec<AiDelegationEdge>, ParseError> {
        self.expect_symbol("{")?;
        let mut edges = Vec::new();
        while !self.peek_symbol("}") {
            self.take_leading_annotations()?;
            self.expect_no_pending_annotations()?;
            let loc = self.loc();
            let source = self.expect_ident()?;
            self.expect_symbol("->")?;
            let target = self.expect_ident()?;
            self.eat_symbol(";");
            edges.push(AiDelegationEdge {
                source,
                target,
                loc,
            });
        }
        self.bump();
        Ok(edges)
    }

    fn failure_policy(&mut self) -> Result<Vec<AiFailurePolicy>, ParseError> {
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.peek_symbol("}") {
            self.take_leading_annotations()?;
            self.expect_no_pending_annotations()?;
            let loc = self.loc();
            self.expect_ident_value("when")?;
            let agent_name = self.expect_ident()?;
            self.expect_symbol(".")?;
            let condition = self.expect_ident()?;
            self.expect_symbol("->")?;
            let (action, retry_limit, target) = if self.eat_ident("retry") {
                self.expect_ident_value("up_to")?;
                let limit = self.expect_int()?;
                ("retry".to_owned(), Some(limit), None)
            } else {
                ("target".to_owned(), None, Some(self.expect_ident()?))
            };
            self.eat_symbol(";");
            items.push(AiFailurePolicy {
                agent: agent_name,
                condition,
                action,
                target,
                retry_limit,
                loc,
            });
        }
        self.bump();
        Ok(items)
    }

    fn agent_contract(&mut self) -> Result<AiAgentContract, ParseError> {
        let loc = self.loc();
        self.expect_symbol("{")?;
        let mut hard_rules = Vec::new();
        while !self.peek_symbol("}") {
            self.take_leading_annotations()?;
            self.expect_no_pending_annotations()?;
            if self.eat_ident("hard") {
                self.expect_symbol("{")?;
                while !self.peek_symbol("}") {
                    self.take_leading_annotations()?;
                    self.expect_no_pending_annotations()?;
                    self.expect_ident_value("rule")?;
                    hard_rules.push(self.expect_ident()?);
                    self.eat_symbol(";");
                }
                self.bump();
            } else if self.eat_ident("rule") {
                hard_rules.push(self.expect_ident()?);
                self.eat_symbol(";");
            } else {
                return Err(self.error("expected 'hard' or 'rule' inside contract"));
            }
        }
        self.bump();
        Ok(AiAgentContract { hard_rules, loc })
    }

    fn expect_int(&mut self) -> Result<i64, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(value),
            _ => Err(ParseError::new("expected integer", token.span)),
        }
    }

    fn names(&mut self) -> Result<Vec<String>, ParseError> {
        if self.eat_symbol("[") {
            let mut names = vec![self.expect_ident()?];
            while self.eat_symbol(",") {
                if self.peek_symbol("]") {
                    break;
                }
                names.push(self.expect_ident()?);
            }
            self.expect_symbol("]")?;
            return Ok(names);
        }
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

    fn atom(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(value) | TokenKind::String(value) => Ok(value),
            _ => Err(ParseError::new("expected name or string", token.span)),
        }
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        let start = self.peek().span.start.offset;
        let line = self.peek().span.start.line;
        while self.peek().span.start.line == line {
            let is_part = matches!(self.peek().kind, TokenKind::Int(_)) || self.peek_symbol(".");
            if !is_part {
                break;
            }
            self.bump();
        }
        let end = self.peek().span.start.offset;
        self.source[start..end]
            .trim()
            .parse()
            .map_err(|_| self.error("expected number"))
    }

    fn loc(&self) -> AiLoc {
        self.peek().span.into()
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
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

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_ident(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
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
