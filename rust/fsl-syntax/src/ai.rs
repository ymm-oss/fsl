// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};

use crate::annotation_parse;
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
    fn python_ast(self) -> Value {
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
    pub fn python_ast(&self) -> Value {
        json!({
            "$type":"AiComponent","name":self.name,"model":self.model,"prompt":self.prompt,
            "retriever":self.retriever,"temperature":self.temperature,
            "input_schema":self.input_schema,"output_schema":self.output_schema,
            "tools":self.tools.iter().map(AiTool::python_ast).collect::<Vec<_>>(),
            "authority":self.authority.python_ast(),
            "fallback":self.fallback.iter().map(AiFallback::python_ast).collect::<Vec<_>>(),
            "check":self.check.python_ast(),"loc":self.loc.python_ast(),
        })
    }
}

impl AiTool {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"AiTool","name":self.name,"schema":self.schema,
            "irreversible":self.irreversible,"preconditions":self.preconditions,
            "effect":self.effect,"loc":self.loc.map(AiLoc::python_ast),
        })
    }
}

impl AiAuthorityRule {
    fn names(rules: &[Self]) -> Vec<&str> {
        rules.iter().map(|rule| rule.name.as_str()).collect()
    }
}

impl AiAuthority {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"AiAuthority","may_suggest":AiAuthorityRule::names(&self.may_suggest),
            "may_execute":AiAuthorityRule::names(&self.may_execute),
            "requires_human_approval":AiAuthorityRule::names(&self.requires_human_approval),
            "forbidden":AiAuthorityRule::names(&self.forbidden),"loc":self.loc.map(AiLoc::python_ast),
        })
    }
}

impl AiFallback {
    fn python_ast(&self) -> Value {
        json!({"$type":"AiFallback","reason":self.reason,"target":self.target,"loc":self.loc.python_ast()})
    }
}

impl AiHardCheck {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"AiHardCheck",
            "rules":self.rules.iter().map(|rule| rule.name.as_str()).collect::<Vec<_>>(),
            "loc":self.loc.map(AiLoc::python_ast),
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
