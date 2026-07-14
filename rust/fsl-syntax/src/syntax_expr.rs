// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Loss-aware parse syntax for expressions and assignment targets.
//!
//! These nodes belong to the source parser boundary. They preserve the span of
//! every expression node and identifier while keeping names and calls
//! unresolved. The checked Kernel continues to use [`crate::Expr`]; conversion
//! to that representation happens only for dialects whose expression syntax is
//! already Kernel syntax.

use std::fmt;
use std::ops::Deref;

use crate::{Binder, Expr, ParseError, Pattern, QualifiedName, Span, SymbolPath, Token, TokenKind};

/// An unresolved source identifier with its exact token span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxIdent {
    pub text: String,
    pub span: Span,
}

impl SyntaxIdent {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl Deref for SyntaxIdent {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for SyntaxIdent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An operator's semantic spelling and the source spelling that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxOperator {
    pub canonical: String,
    pub spelling: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxQualifiedName {
    pub path: SymbolPath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxPattern {
    None { span: Span },
    Some { name: SyntaxIdent, span: Span },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxBinder {
    Typed {
        name: SyntaxIdent,
        type_name: SyntaxQualifiedName,
        where_expr: Option<Box<SyntaxExpr>>,
        span: Span,
    },
    Range {
        name: SyntaxIdent,
        lo: Box<SyntaxExpr>,
        hi: Box<SyntaxExpr>,
        span: Span,
    },
    Collection {
        name: SyntaxIdent,
        collection: Box<SyntaxExpr>,
        where_expr: Option<Box<SyntaxExpr>>,
        span: Span,
    },
}

/// An unresolved, structured type reference with spans for every component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxTypeExpr {
    pub kind: SyntaxTypeExprKind,
    pub span: Span,
    canonical: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxTypeExprKind {
    Name(SyntaxIdent),
    Apply {
        constructor: SyntaxIdent,
        arguments: Vec<SyntaxTypeExpr>,
    },
}

/// An unresolved expression parsed directly from the shared source token stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxExpr {
    pub kind: SyntaxExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxExprKind {
    Num(i64),
    Bool(bool),
    None,
    Some(Box<SyntaxExpr>),
    Set(Vec<SyntaxExpr>),
    Seq(Vec<SyntaxExpr>),
    Struct {
        name: SyntaxIdent,
        fields: Vec<(SyntaxIdent, SyntaxExpr)>,
    },
    Name(SyntaxIdent),
    Call {
        callee: SyntaxIdent,
        args: Vec<SyntaxExpr>,
    },
    Index {
        receiver: Box<SyntaxExpr>,
        index: Box<SyntaxExpr>,
    },
    Field {
        receiver: Box<SyntaxExpr>,
        field: SyntaxIdent,
    },
    Method {
        receiver: Box<SyntaxExpr>,
        method: SyntaxIdent,
        args: Vec<SyntaxExpr>,
    },
    Binary {
        op: SyntaxOperator,
        left: Box<SyntaxExpr>,
        right: Box<SyntaxExpr>,
    },
    Membership {
        value: Box<SyntaxExpr>,
        members: Vec<SyntaxExpr>,
    },
    Neg(Box<SyntaxExpr>),
    Not(Box<SyntaxExpr>),
    Group(Box<SyntaxExpr>),
    IfThenElse {
        condition: Box<SyntaxExpr>,
        then_expr: Box<SyntaxExpr>,
        else_expr: Box<SyntaxExpr>,
    },
    Is {
        expr: Box<SyntaxExpr>,
        pattern: SyntaxPattern,
    },
    Quantified {
        quantifier: SyntaxIdent,
        binder: SyntaxBinder,
        body: Box<SyntaxExpr>,
    },
    Count {
        name: SyntaxIdent,
        type_name: SyntaxQualifiedName,
        condition: Box<SyntaxExpr>,
    },
    Sum {
        name: SyntaxIdent,
        type_name: SyntaxQualifiedName,
        body: Box<SyntaxExpr>,
        condition: Option<Box<SyntaxExpr>>,
    },
    BinderNamed {
        name: SyntaxIdent,
        binder: SyntaxBinder,
    },
}

/// A structured assignment target whose identifiers and composite nodes retain spans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxLValue {
    Name(SyntaxIdent),
    Index {
        base: Box<SyntaxLValue>,
        index: Box<SyntaxExpr>,
        span: Span,
    },
    Field {
        base: Box<SyntaxLValue>,
        field: SyntaxIdent,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpressionMode {
    Kernel,
    Domain,
}

impl SyntaxExpr {
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn render_source(&self) -> String {
        match &self.kind {
            SyntaxExprKind::Num(value) => value.to_string(),
            SyntaxExprKind::Bool(value) => value.to_string(),
            SyntaxExprKind::None => "none".to_owned(),
            SyntaxExprKind::Some(value) => format!("some({})", value.render_source()),
            SyntaxExprKind::Set(values) => format!("Set {{ {} }}", render_list(values)),
            SyntaxExprKind::Seq(values) => format!("Seq {{ {} }}", render_list(values)),
            SyntaxExprKind::Struct { name, fields } => format!(
                "{} {{ {} }}",
                name.text,
                fields
                    .iter()
                    .map(|(field, value)| format!("{}: {}", field.text, value.render_source()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            SyntaxExprKind::Name(name) => name.text.clone(),
            SyntaxExprKind::Call { callee, args } => {
                format!("{}({})", callee.text, render_list(args))
            }
            SyntaxExprKind::Index { receiver, index } => {
                format!("{}[{}]", receiver.render_source(), index.render_source())
            }
            SyntaxExprKind::Field { receiver, field } => {
                format!("{}.{}", receiver.render_source(), field.text)
            }
            SyntaxExprKind::Method {
                receiver,
                method,
                args,
            } => format!(
                "{}.{}({})",
                receiver.render_source(),
                method.text,
                render_list(args)
            ),
            SyntaxExprKind::Binary { op, left, right } => format!(
                "{} {} {}",
                left.render_source(),
                op.spelling,
                right.render_source()
            ),
            SyntaxExprKind::Membership { value, members } => {
                format!("{} in [{}]", value.render_source(), render_list(members))
            }
            SyntaxExprKind::Neg(value) => format!("-{}", value.render_source()),
            SyntaxExprKind::Not(value) => format!("not {}", value.render_source()),
            SyntaxExprKind::Group(value) => format!("({})", value.render_source()),
            SyntaxExprKind::IfThenElse {
                condition,
                then_expr,
                else_expr,
            } => format!(
                "if {} then {} else {}",
                condition.render_source(),
                then_expr.render_source(),
                else_expr.render_source()
            ),
            SyntaxExprKind::Is { expr, pattern } => match pattern {
                SyntaxPattern::None { .. } => format!("{} is none", expr.render_source()),
                SyntaxPattern::Some { name, .. } => {
                    format!("{} is some({})", expr.render_source(), name.text)
                }
            },
            SyntaxExprKind::Quantified {
                quantifier,
                binder,
                body,
            } => format!(
                "{} {} {{ {} }}",
                quantifier.text,
                binder.render_source(),
                body.render_source()
            ),
            SyntaxExprKind::Count {
                name,
                type_name,
                condition,
            } => format!(
                "count({}: {} where {})",
                name.text,
                type_name.render_source(),
                condition.render_source()
            ),
            SyntaxExprKind::Sum {
                name,
                type_name,
                body,
                condition,
            } => {
                let suffix = condition.as_deref().map_or_else(String::new, |condition| {
                    format!(" where {}", condition.render_source())
                });
                format!(
                    "sum({}: {} of {}{})",
                    name.text,
                    type_name.render_source(),
                    body.render_source(),
                    suffix
                )
            }
            SyntaxExprKind::BinderNamed { name, binder } => {
                format!("{}({})", name.text, binder.render_source())
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn into_kernel(self) -> Result<Expr, ParseError> {
        let span = self.span;
        Ok(match self.kind {
            SyntaxExprKind::Num(value) => Expr::Num(value),
            SyntaxExprKind::Bool(value) => Expr::Bool(value),
            SyntaxExprKind::None => Expr::None,
            SyntaxExprKind::Some(value) => Expr::Some(Box::new(value.into_kernel()?)),
            SyntaxExprKind::Set(values) => Expr::Set(convert_list(values)?),
            SyntaxExprKind::Seq(values) => Expr::Seq(convert_list(values)?),
            SyntaxExprKind::Struct { name, fields } => Expr::Struct {
                name: name.text,
                fields: fields
                    .into_iter()
                    .map(|(field, value)| Ok((field.text, value.into_kernel()?)))
                    .collect::<Result<Vec<_>, ParseError>>()?,
            },
            SyntaxExprKind::Name(name) => Expr::Var(name.text),
            SyntaxExprKind::Call { callee, args } => {
                let name = callee.text;
                let call_span = callee.span;
                let args = convert_list(args)?;
                match (name.as_str(), args.as_slice()) {
                    ("stage" | "old" | "abs", [expr]) => Expr::UnaryNamed {
                        name,
                        expr: Box::new(expr.clone()),
                        span: call_span,
                    },
                    ("acyclic" | "functional" | "injective" | "domain" | "range", [expr]) => {
                        Expr::UnaryNamed {
                            name: format!("rel_{name}"),
                            expr: Box::new(expr.clone()),
                            span: call_span,
                        }
                    }
                    ("min" | "max", [left, right]) => Expr::BinaryNamed {
                        name,
                        left: Box::new(left.clone()),
                        right: Box::new(right.clone()),
                    },
                    ("reachable", [first, second, third]) => Expr::TernaryNamed {
                        name: "rel_reachable".to_owned(),
                        first: Box::new(first.clone()),
                        second: Box::new(second.clone()),
                        third: Box::new(third.clone()),
                    },
                    _ => Expr::Call {
                        name,
                        args,
                        span: call_span,
                    },
                }
            }
            SyntaxExprKind::Index { receiver, index } => Expr::Index(
                Box::new(receiver.into_kernel()?),
                Box::new(index.into_kernel()?),
            ),
            SyntaxExprKind::Field { receiver, field } => {
                Expr::Field(Box::new(receiver.into_kernel()?), field.text)
            }
            SyntaxExprKind::Method {
                receiver,
                method,
                args,
            } => Expr::Method {
                receiver: Box::new(receiver.into_kernel()?),
                name: method.text,
                args: convert_list(args)?,
            },
            SyntaxExprKind::Binary { op, left, right } => Expr::Binary {
                op: op.canonical,
                left: Box::new(left.into_kernel()?),
                right: Box::new(right.into_kernel()?),
            },
            SyntaxExprKind::Membership { .. } => {
                return Err(ParseError::new(
                    "finite membership is domain syntax and requires domain lowering",
                    span,
                ));
            }
            SyntaxExprKind::Neg(value) => Expr::Neg(Box::new(value.into_kernel()?)),
            SyntaxExprKind::Not(value) => Expr::Not(Box::new(value.into_kernel()?)),
            SyntaxExprKind::Group(value) => value.into_kernel()?,
            SyntaxExprKind::IfThenElse {
                condition,
                then_expr,
                else_expr,
            } => Expr::IfThenElse {
                condition: Box::new(condition.into_kernel()?),
                then_expr: Box::new(then_expr.into_kernel()?),
                else_expr: Box::new(else_expr.into_kernel()?),
            },
            SyntaxExprKind::Is { expr, pattern } => Expr::Is {
                expr: Box::new(expr.into_kernel()?),
                pattern: match pattern {
                    SyntaxPattern::None { .. } => Pattern::None,
                    SyntaxPattern::Some { name, .. } => Pattern::Some(name.text),
                },
            },
            SyntaxExprKind::Quantified {
                quantifier,
                binder,
                body,
            } => Expr::Quantified {
                quantifier: quantifier.text,
                binder: binder.into_kernel()?,
                body: Box::new(body.into_kernel()?),
            },
            SyntaxExprKind::Count {
                name,
                type_name,
                condition,
            } => Expr::Count {
                name: name.text,
                type_name: type_name.into_kernel(),
                condition: Box::new(condition.into_kernel()?),
            },
            SyntaxExprKind::Sum {
                name,
                type_name,
                body,
                condition,
            } => Expr::Sum {
                name: name.text,
                type_name: type_name.into_kernel(),
                body: Box::new(body.into_kernel()?),
                condition: condition
                    .map(|value| value.into_kernel().map(Box::new))
                    .transpose()?,
            },
            SyntaxExprKind::BinderNamed { name, binder } => Expr::BinderNamed {
                name: if name.text == "exactlyOne" {
                    "exactly_one".to_owned()
                } else {
                    name.text
                },
                binder: binder.into_kernel()?,
            },
        })
    }
}

impl fmt::Display for SyntaxExpr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_source())
    }
}

impl SyntaxLValue {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Name(name) => name.span,
            Self::Index { span, .. } | Self::Field { span, .. } => *span,
        }
    }

    #[must_use]
    pub fn render_source(&self) -> String {
        match self {
            Self::Name(name) => name.text.clone(),
            Self::Index { base, index, .. } => {
                format!("{}[{}]", base.render_source(), index.render_source())
            }
            Self::Field { base, field, .. } => {
                format!("{}.{}", base.render_source(), field.text)
            }
        }
    }
}

impl fmt::Display for SyntaxLValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_source())
    }
}

impl SyntaxQualifiedName {
    #[must_use]
    pub fn render_source(&self) -> String {
        self.path.to_string()
    }

    #[must_use]
    pub fn span(&self) -> Span {
        self.path.span()
    }

    #[must_use]
    pub fn name(&self) -> &str {
        self.path.name()
    }

    #[must_use]
    pub fn has_namespace(&self) -> bool {
        self.path.has_namespace()
    }

    fn into_kernel(self) -> QualifiedName {
        let (namespace, name) = self.path.legacy_parts();
        QualifiedName { namespace, name }
    }
}

impl SyntaxTypeExpr {
    pub(crate) fn name(name: SyntaxIdent) -> Self {
        Self {
            canonical: name.text.clone(),
            span: name.span,
            kind: SyntaxTypeExprKind::Name(name),
        }
    }

    pub(crate) fn apply(
        constructor: SyntaxIdent,
        arguments: Vec<SyntaxTypeExpr>,
        span: Span,
    ) -> Self {
        let canonical = format!(
            "{}<{}>",
            constructor.text,
            arguments
                .iter()
                .map(Self::render_source)
                .collect::<Vec<_>>()
                .join(", ")
        );
        Self {
            kind: SyntaxTypeExprKind::Apply {
                constructor,
                arguments,
            },
            span,
            canonical,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    #[must_use]
    pub fn render_source(&self) -> String {
        self.canonical.clone()
    }
}

impl Deref for SyntaxTypeExpr {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for SyntaxTypeExpr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_source())
    }
}

impl SyntaxBinder {
    fn render_source(&self) -> String {
        match self {
            Self::Typed {
                name,
                type_name,
                where_expr,
                ..
            } => format!(
                "{}: {}{}",
                name.text,
                type_name.render_source(),
                where_expr.as_deref().map_or_else(String::new, |value| {
                    format!(" where {}", value.render_source())
                })
            ),
            Self::Range { name, lo, hi, .. } => format!(
                "{} in {}..{}",
                name.text,
                lo.render_source(),
                hi.render_source()
            ),
            Self::Collection {
                name,
                collection,
                where_expr,
                ..
            } => format!(
                "{} in {}{}",
                name.text,
                collection.render_source(),
                where_expr.as_deref().map_or_else(String::new, |value| {
                    format!(" where {}", value.render_source())
                })
            ),
        }
    }

    fn into_kernel(self) -> Result<Binder, ParseError> {
        Ok(match self {
            Self::Typed {
                name,
                type_name,
                where_expr,
                ..
            } => Binder::Typed {
                name: name.text,
                type_name: type_name.into_kernel(),
                where_expr: where_expr
                    .map(|value| value.into_kernel().map(Box::new))
                    .transpose()?,
            },
            Self::Range { name, lo, hi, .. } => Binder::Range {
                name: name.text,
                lo: Box::new(lo.into_kernel()?),
                hi: Box::new(hi.into_kernel()?),
            },
            Self::Collection {
                name,
                collection,
                where_expr,
                ..
            } => Binder::Collection {
                name: name.text,
                collection: Box::new(collection.into_kernel()?),
                where_expr: where_expr
                    .map(|value| value.into_kernel().map(Box::new))
                    .transpose()?,
            },
        })
    }
}

fn render_list(values: &[SyntaxExpr]) -> String {
    values
        .iter()
        .map(SyntaxExpr::render_source)
        .collect::<Vec<_>>()
        .join(", ")
}

fn convert_list(values: Vec<SyntaxExpr>) -> Result<Vec<Expr>, ParseError> {
    values.into_iter().map(SyntaxExpr::into_kernel).collect()
}

pub(crate) fn parse_tokens_expression(
    tokens: &[Token],
    cursor: &mut usize,
    mode: ExpressionMode,
    line_terminated: bool,
) -> Result<SyntaxExpr, ParseError> {
    let line = line_terminated.then(|| tokens[*cursor].span.start.line);
    let mut parser = SyntaxParser {
        tokens,
        cursor: *cursor,
        mode,
        line,
    };
    let expression = parser.expression(0)?;
    *cursor = parser.cursor;
    Ok(expression)
}

pub(crate) fn parse_tokens_lvalue(
    tokens: &[Token],
    cursor: &mut usize,
    mode: ExpressionMode,
) -> Result<SyntaxLValue, ParseError> {
    let mut parser = SyntaxParser {
        tokens,
        cursor: *cursor,
        mode,
        line: None,
    };
    let target = parser.lvalue()?;
    *cursor = parser.cursor;
    Ok(target)
}

struct SyntaxParser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    mode: ExpressionMode,
    line: Option<u32>,
}

impl SyntaxParser<'_> {
    fn expression(&mut self, min_binding_power: u8) -> Result<SyntaxExpr, ParseError> {
        let mut left = self.prefix()?;
        left = self.postfix(left)?;

        loop {
            if self.peek_ident("is") {
                let left_power = 4;
                if left_power < min_binding_power {
                    break;
                }
                self.bump();
                let pattern = if self.eat_ident("none") {
                    SyntaxPattern::None {
                        span: self.previous_span(),
                    }
                } else if self.eat_ident("some") {
                    let start = self.previous_span();
                    self.expect_symbol("(")?;
                    let name = self.expect_ident()?;
                    self.expect_symbol(")")?;
                    SyntaxPattern::Some {
                        name,
                        span: join(start, self.previous_span()),
                    }
                } else {
                    return Err(self.error("expected none or some(name) after is"));
                };
                let span = join(left.span, self.previous_span());
                left = SyntaxExpr {
                    kind: SyntaxExprKind::Is {
                        expr: Box::new(left),
                        pattern,
                    },
                    span,
                };
                continue;
            }

            if self.mode == ExpressionMode::Domain && self.peek_ident("in") {
                let left_power = 5;
                if left_power < min_binding_power {
                    break;
                }
                self.bump();
                self.expect_symbol("[")?;
                let members = self.expression_list("]")?;
                let span = join(left.span, self.previous_span());
                left = SyntaxExpr {
                    kind: SyntaxExprKind::Membership {
                        value: Box::new(left),
                        members,
                    },
                    span,
                };
                continue;
            }

            let Some((canonical, spelling, left_power, right_power)) = self.infix() else {
                break;
            };
            if left_power < min_binding_power {
                break;
            }
            let operator = self.bump().clone();
            let right = self.expression(right_power)?;
            let span = join(left.span, right.span);
            left = SyntaxExpr {
                kind: SyntaxExprKind::Binary {
                    op: SyntaxOperator {
                        canonical,
                        spelling,
                        span: operator.span,
                    },
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<SyntaxExpr, ParseError> {
        if self.peek_ident("forall") || self.peek_ident("exists") {
            let start = self.peek().span;
            let quantifier = self.expect_ident()?;
            let binder = self.binder()?;
            self.eat_symbol(":");
            let body = if self.eat_symbol("{") {
                let body = self.expression(0)?;
                self.expect_symbol("}")?;
                body
            } else {
                self.expression(0)?
            };
            let span = join(start, self.previous_span());
            return Ok(SyntaxExpr {
                kind: SyntaxExprKind::Quantified {
                    quantifier,
                    binder,
                    body: Box::new(body),
                },
                span,
            });
        }
        if self.eat_ident("not") {
            let start = self.previous_span();
            let value = self.expression(4)?;
            return Ok(SyntaxExpr {
                span: join(start, value.span),
                kind: SyntaxExprKind::Not(Box::new(value)),
            });
        }
        if self.eat_symbol("-") {
            let start = self.previous_span();
            let value = self.expression(8)?;
            return Ok(SyntaxExpr {
                span: join(start, value.span),
                kind: SyntaxExprKind::Neg(Box::new(value)),
            });
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<SyntaxExpr, ParseError> {
        if self.at_boundary() {
            return Err(self.boundary_error("expected expression"));
        }
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(SyntaxExpr {
                kind: SyntaxExprKind::Num(value),
                span: token.span,
            }),
            TokenKind::Ident(name) if name == "true" || name == "false" => Ok(SyntaxExpr {
                kind: SyntaxExprKind::Bool(name == "true"),
                span: token.span,
            }),
            TokenKind::Ident(name) if name == "none" => Ok(SyntaxExpr {
                kind: SyntaxExprKind::None,
                span: token.span,
            }),
            TokenKind::Ident(name) if name == "some" => {
                self.expect_symbol("(")?;
                let value = self.expression(0)?;
                self.expect_symbol(")")?;
                Ok(SyntaxExpr {
                    kind: SyntaxExprKind::Some(Box::new(value)),
                    span: join(token.span, self.previous_span()),
                })
            }
            TokenKind::Ident(name) if name == "Set" || name == "Seq" => {
                self.expect_symbol("{")?;
                let values = self.expression_list("}")?;
                Ok(SyntaxExpr {
                    kind: if name == "Set" {
                        SyntaxExprKind::Set(values)
                    } else {
                        SyntaxExprKind::Seq(values)
                    },
                    span: join(token.span, self.previous_span()),
                })
            }
            TokenKind::Ident(name) if name == "count" && self.peek_symbol("(") => {
                self.count(token.span)
            }
            TokenKind::Ident(name) if name == "sum" && self.peek_symbol("(") => {
                self.sum(token.span)
            }
            TokenKind::Ident(name) if name == "unique" || name == "exactlyOne" => {
                let ident = SyntaxIdent {
                    text: name,
                    span: token.span,
                };
                self.expect_symbol("(")?;
                let binder = self.binder()?;
                self.expect_symbol(")")?;
                Ok(SyntaxExpr {
                    kind: SyntaxExprKind::BinderNamed {
                        name: ident,
                        binder,
                    },
                    span: join(token.span, self.previous_span()),
                })
            }
            TokenKind::Ident(name) => self.ident_atom(
                SyntaxIdent {
                    text: name,
                    span: token.span,
                },
                token.span,
            ),
            TokenKind::Symbol(symbol) if symbol == "(" => {
                let value = self.expression(0)?;
                self.expect_symbol(")")?;
                Ok(SyntaxExpr {
                    kind: SyntaxExprKind::Group(Box::new(value)),
                    span: join(token.span, self.previous_span()),
                })
            }
            _ => Err(ParseError::new("expected expression", token.span)),
        }
    }

    fn ident_atom(&mut self, name: SyntaxIdent, start: Span) -> Result<SyntaxExpr, ParseError> {
        if self.starts_struct_fields() {
            self.bump();
            let mut fields = Vec::new();
            if !self.eat_symbol("}") {
                loop {
                    let field = self.expect_ident()?;
                    self.expect_symbol(":")?;
                    fields.push((field, self.expression(0)?));
                    if self.eat_symbol("}") {
                        break;
                    }
                    self.expect_symbol(",")?;
                    if self.eat_symbol("}") {
                        break;
                    }
                }
            }
            return Ok(SyntaxExpr {
                kind: SyntaxExprKind::Struct { name, fields },
                span: join(start, self.previous_span()),
            });
        }
        if !self.eat_symbol("(") {
            return Ok(SyntaxExpr {
                kind: SyntaxExprKind::Name(name),
                span: start,
            });
        }
        let args = self.expression_list(")")?;
        Ok(SyntaxExpr {
            kind: SyntaxExprKind::Call { callee: name, args },
            span: join(start, self.previous_span()),
        })
    }

    fn postfix(&mut self, mut expression: SyntaxExpr) -> Result<SyntaxExpr, ParseError> {
        loop {
            if self.eat_symbol("[") {
                let index = self.expression(0)?;
                self.expect_symbol("]")?;
                let span = join(expression.span, self.previous_span());
                expression = SyntaxExpr {
                    kind: SyntaxExprKind::Index {
                        receiver: Box::new(expression),
                        index: Box::new(index),
                    },
                    span,
                };
                continue;
            }
            if !self.eat_symbol(".") {
                return Ok(expression);
            }
            let start = expression.span;
            let name = self.expect_ident()?;
            let kind = if self.eat_symbol("(") {
                let args = self.expression_list(")")?;
                if !matches!(
                    name.text.as_str(),
                    "contains" | "add" | "remove" | "push" | "pop" | "head" | "at" | "size"
                ) {
                    return Err(self.error("unknown FSL collection method"));
                }
                SyntaxExprKind::Method {
                    receiver: Box::new(expression),
                    method: name,
                    args,
                }
            } else {
                SyntaxExprKind::Field {
                    receiver: Box::new(expression),
                    field: name,
                }
            };
            expression = SyntaxExpr {
                kind,
                span: join(start, self.previous_span()),
            };
        }
    }

    fn lvalue(&mut self) -> Result<SyntaxLValue, ParseError> {
        let name = self.expect_ident()?;
        let mut target = SyntaxLValue::Name(name);
        loop {
            if self.eat_symbol("[") {
                let index = self.expression(0)?;
                self.expect_symbol("]")?;
                let span = join(target.span(), self.previous_span());
                target = SyntaxLValue::Index {
                    base: Box::new(target),
                    index: Box::new(index),
                    span,
                };
                continue;
            }
            if self.eat_symbol(".") {
                let field = self.expect_ident()?;
                let span = join(target.span(), field.span);
                target = SyntaxLValue::Field {
                    base: Box::new(target),
                    field,
                    span,
                };
                continue;
            }
            return Ok(target);
        }
    }

    fn binder(&mut self) -> Result<SyntaxBinder, ParseError> {
        let name = self.expect_ident()?;
        let start = name.span;
        if self.eat_symbol(":") {
            let type_name = self.qualified_name()?;
            let where_expr = if self.eat_ident("where") {
                Some(Box::new(self.expression(0)?))
            } else {
                None
            };
            let end = where_expr
                .as_deref()
                .map_or(type_name.span(), |expression| expression.span);
            return Ok(SyntaxBinder::Typed {
                name,
                type_name,
                where_expr,
                span: join(start, end),
            });
        }
        if !self.eat_ident("in") {
            return Err(self.error("expected ':' or 'in' in binder"));
        }
        let first = self.expression(0)?;
        if self.eat_symbol("..") {
            let hi = Box::new(self.expression(0)?);
            let span = join(start, hi.span);
            return Ok(SyntaxBinder::Range {
                name,
                lo: Box::new(first),
                hi,
                span,
            });
        }
        let where_expr = if self.eat_ident("where") {
            Some(Box::new(self.expression(0)?))
        } else {
            None
        };
        let end = where_expr
            .as_deref()
            .map_or(first.span, |expression| expression.span);
        Ok(SyntaxBinder::Collection {
            name,
            collection: Box::new(first),
            where_expr,
            span: join(start, end),
        })
    }

    fn count(&mut self, start: Span) -> Result<SyntaxExpr, ParseError> {
        self.expect_symbol("(")?;
        let name = self.expect_ident()?;
        self.expect_symbol(":")?;
        let type_name = self.qualified_name()?;
        self.expect_ident_value("where")?;
        let condition = self.expression(0)?;
        self.expect_symbol(")")?;
        Ok(SyntaxExpr {
            kind: SyntaxExprKind::Count {
                name,
                type_name,
                condition: Box::new(condition),
            },
            span: join(start, self.previous_span()),
        })
    }

    fn sum(&mut self, start: Span) -> Result<SyntaxExpr, ParseError> {
        self.expect_symbol("(")?;
        let name = self.expect_ident()?;
        self.expect_symbol(":")?;
        let type_name = self.qualified_name()?;
        self.expect_ident_value("of")?;
        let body = self.expression(0)?;
        let condition = if self.eat_ident("where") {
            Some(Box::new(self.expression(0)?))
        } else {
            None
        };
        self.expect_symbol(")")?;
        Ok(SyntaxExpr {
            kind: SyntaxExprKind::Sum {
                name,
                type_name,
                body: Box::new(body),
                condition,
            },
            span: join(start, self.previous_span()),
        })
    }

    fn qualified_name(&mut self) -> Result<SyntaxQualifiedName, ParseError> {
        let mut segments = vec![self.expect_ident()?];
        while self.eat_symbol(".") {
            segments.push(self.expect_ident()?);
        }
        let span = join(
            segments.first().expect("qualified path is non-empty").span,
            segments.last().expect("qualified path is non-empty").span,
        );
        let path = SymbolPath::from_idents(segments, span)
            .map_err(|error| ParseError::new(error.message, error.span))?;
        Ok(SyntaxQualifiedName { path })
    }

    fn expression_list(&mut self, close: &str) -> Result<Vec<SyntaxExpr>, ParseError> {
        let mut values = Vec::new();
        if self.eat_symbol(close) {
            return Ok(values);
        }
        loop {
            values.push(self.expression(0)?);
            if self.eat_symbol(close) {
                return Ok(values);
            }
            self.expect_symbol(",")?;
            if self.eat_symbol(close) {
                return Ok(values);
            }
        }
    }

    fn infix(&self) -> Option<(String, String, u8, u8)> {
        if self.at_boundary() {
            return None;
        }
        let spelling = match &self.peek().kind {
            TokenKind::Ident(value) | TokenKind::Symbol(value) => value.as_str(),
            _ => return None,
        };
        match spelling {
            "=>" => Some(("=>".to_owned(), spelling.to_owned(), 1, 1)),
            "->" if self.mode == ExpressionMode::Domain => {
                Some(("=>".to_owned(), spelling.to_owned(), 1, 1))
            }
            "or" => Some(("or".to_owned(), spelling.to_owned(), 2, 3)),
            "||" if self.mode == ExpressionMode::Domain => {
                Some(("or".to_owned(), spelling.to_owned(), 2, 3))
            }
            "and" => Some(("and".to_owned(), spelling.to_owned(), 3, 4)),
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                Some((spelling.to_owned(), spelling.to_owned(), 5, 6))
            }
            "+" | "-" => Some((spelling.to_owned(), spelling.to_owned(), 6, 7)),
            "*" | "/" | "%" => Some((spelling.to_owned(), spelling.to_owned(), 7, 8)),
            _ => None,
        }
    }

    fn starts_struct_fields(&self) -> bool {
        self.peek_symbol("{")
            && matches!(self.peek_n(1).kind, TokenKind::Ident(_))
            && matches!(&self.peek_n(2).kind, TokenKind::Symbol(value) if value == ":")
    }

    fn at_boundary(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
            || self
                .line
                .is_some_and(|line| self.peek().span.start.line != line)
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .unwrap_or_else(|| self.tokens.last().expect("lexer emits EOF"))
    }

    fn peek_n(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.cursor + offset)
            .unwrap_or_else(|| self.tokens.last().expect("lexer emits EOF"))
    }

    fn previous_span(&self) -> Span {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or_else(|| self.peek().span, |token| token.span)
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
    }

    fn peek_ident(&self, expected: &str) -> bool {
        !self.at_boundary()
            && matches!(&self.peek().kind, TokenKind::Ident(value) if value == expected)
    }

    fn peek_symbol(&self, expected: &str) -> bool {
        !self.at_boundary()
            && matches!(&self.peek().kind, TokenKind::Symbol(value) if value == expected)
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

    fn expect_ident(&mut self) -> Result<SyntaxIdent, ParseError> {
        if self.at_boundary() {
            return Err(self.boundary_error("expected identifier"));
        }
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

    fn expect_symbol(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_symbol(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        if self.at_boundary() {
            self.boundary_error(message)
        } else {
            ParseError::new(message, self.peek().span)
        }
    }

    fn boundary_error(&self, message: &str) -> ParseError {
        let end = self.previous_span().end;
        ParseError::new(message, Span { start: end, end })
    }
}

fn join(first: Span, last: Span) -> Span {
    Span {
        start: first.start,
        end: last.end,
    }
}
