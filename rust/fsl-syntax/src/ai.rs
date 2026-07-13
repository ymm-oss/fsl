// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};

use crate::{ParseError, Span, Token, TokenKind, lex};

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
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AiAuthority {
    pub may_suggest: Vec<String>,
    pub may_execute: Vec<String>,
    pub requires_human_approval: Vec<String>,
    pub forbidden: Vec<String>,
    pub loc: Option<AiLoc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiFallback {
    pub reason: String,
    pub target: String,
    pub loc: AiLoc,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AiHardCheck {
    pub rules: Vec<String>,
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

impl AiAuthority {
    fn python_ast(&self) -> Value {
        json!({
            "$type":"AiAuthority","may_suggest":self.may_suggest,
            "may_execute":self.may_execute,
            "requires_human_approval":self.requires_human_approval,
            "forbidden":self.forbidden,"loc":self.loc.map(AiLoc::python_ast),
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
        json!({"$type":"AiHardCheck","rules":self.rules,"loc":self.loc.map(AiLoc::python_ast)})
    }
}

/// Parse one specialized `ai_component` source into typed frontend IR.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical or syntactic analysis fails.
pub fn parse_ai_component(source: &str) -> Result<AiComponent, ParseError> {
    let tokens = lex(source).map_err(|error| ParseError {
        message: error.message,
        span: error.span,
    })?;
    let mut parser = AiParser {
        source,
        tokens,
        cursor: 0,
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
}

impl AiParser<'_> {
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
            if self.eat_ident("model") {
                component.model = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("prompt") {
                component.prompt = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("retriever") {
                component.retriever = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("temperature") {
                component.temperature = Some(self.number()?);
                self.eat_symbol(";");
            } else if self.eat_ident("input") {
                component.input_schema = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("output") {
                component.output_schema = Some(self.atom()?);
                self.eat_symbol(";");
            } else if self.eat_ident("tools") {
                for name in self.names()? {
                    component.tools.push(AiTool {
                        name,
                        schema: None,
                        irreversible: false,
                        preconditions: Vec::new(),
                        effect: None,
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
        let loc = self.loc();
        self.bump();
        if !self.peek_symbol("{") {
            self.expect_ident()?;
        }
        self.expect_symbol("{")?;
        let mut authority = AiAuthority {
            loc: Some(loc),
            ..AiAuthority::default()
        };
        while !self.eat_symbol("}") {
            let kind = self.expect_ident()?;
            let names = self.names()?;
            match kind.as_str() {
                "may_suggest" => authority.may_suggest.extend(names),
                "may_execute" => authority.may_execute.extend(names),
                "requires_human_approval" => authority.requires_human_approval.extend(names),
                "forbidden" => authority.forbidden.extend(names),
                _ => return Err(self.error("unknown authority declaration")),
            }
            self.eat_symbol(";");
        }
        Ok(authority)
    }

    fn fallback(&mut self) -> Result<Vec<AiFallback>, ParseError> {
        self.bump();
        self.expect_symbol("{")?;
        let mut items = Vec::new();
        while !self.eat_symbol("}") {
            let loc = self.loc();
            self.expect_ident_value("when")?;
            let reason = self.expect_ident()?;
            self.expect_ident_value("require")?;
            items.push(AiFallback {
                reason,
                target: self.expect_ident()?,
                loc,
            });
            self.eat_symbol(";");
        }
        Ok(items)
    }

    fn check(&mut self) -> Result<AiHardCheck, ParseError> {
        let loc = self.loc();
        self.bump();
        self.expect_ident_value("hard")?;
        self.expect_symbol("{")?;
        let mut rules = Vec::new();
        while !self.eat_symbol("}") {
            self.expect_ident_value("rule")?;
            rules.push(self.expect_ident()?);
            self.eat_symbol(";");
        }
        Ok(AiHardCheck {
            rules,
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
            _ => Err(ParseError {
                message: "expected name or string".to_owned(),
                span: token.span,
            }),
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
            _ => Err(ParseError {
                message: "expected identifier".to_owned(),
                span: token.span,
            }),
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
        ParseError {
            message: message.to_owned(),
            span: self.peek().span,
        }
    }
}
