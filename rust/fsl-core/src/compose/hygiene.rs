// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Source-owned lexical rewriting before action environments are merged.

use std::collections::{HashMap, HashSet};

use fsl_syntax::{ActionItem, Binder, Expr, LValue, Pattern, Span, Statement};

use super::{ComponentNames, component_rewrite_error, map_normalized_action_items};
use crate::model::ExpressionErrorContext;
use crate::typecheck::{TypecheckError, reject_pattern_binding_name};
use crate::{CoreError, capture_names, fresh_binding_name};

pub(super) struct Arguments<'a> {
    pub values: &'a HashMap<String, Expr>,
    pub globals: &'a ComponentNames,
    pub params: &'a HashSet<String>,
    pub span: Span,
    pub avoid: &'a HashSet<String>,
}

pub(super) struct Source<'a> {
    pub globals: &'a ComponentNames,
    pub params: &'a HashSet<String>,
    pub span: Span,
    pub arguments: Option<Arguments<'a>>,
}

#[derive(Clone)]
struct Scope {
    original: HashSet<String>,
    renamed: HashMap<String, String>,
    exporting: bool,
}

impl Scope {
    fn new(params: &HashSet<String>) -> Self {
        Self {
            original: params.clone(),
            renamed: HashMap::new(),
            exporting: true,
        }
    }
}

struct Renamer<'a> {
    source: Source<'a>,
    avoid: &'a HashSet<String>,
    pattern_avoid: &'a HashSet<String>,
    ambient: &'a HashSet<String>,
    incoming: HashSet<String>,
    used: &'a mut HashSet<String>,
}

pub(super) fn rewrite(
    items: Vec<ActionItem>,
    source: Source<'_>,
    avoid: &HashSet<String>,
    pattern_avoid: &HashSet<String>,
    ambient: &HashSet<String>,
    used: &mut HashSet<String>,
) -> Result<Vec<ActionItem>, CoreError> {
    let mut scope = Scope::new(source.params);
    let span = source.span;
    let mut incoming = HashSet::new();
    if let Some(arguments) = &source.arguments {
        for value in arguments.values.values() {
            incoming.extend(capture_names(value, &HashSet::new()));
        }
    }
    let mut renamer = Renamer {
        source,
        avoid,
        pattern_avoid,
        ambient,
        incoming,
        used,
    };
    map_normalized_action_items(items, &mut scope, |item, scope| {
        Ok(match item {
            ActionItem::Let(name, expr, item_span) => {
                let value = renamer.local_at(expr, scope, span)?;
                let name = renamer.bind(name, scope, Binding::Let)?;
                ActionItem::Let(name, value, item_span)
            }
            ActionItem::Requires(expr, item_span) => {
                let expr = renamer
                    .expression(expr, scope)
                    .map_err(|error| error.with_span(span))?;
                ActionItem::Requires(expr, item_span)
            }
            ActionItem::Ensures(expr, item_span) => {
                ActionItem::Ensures(renamer.local_at(expr, scope, span)?, item_span)
            }
            ActionItem::Statement(statement) => {
                ActionItem::Statement(renamer.statement(statement, scope)?)
            }
        })
    })
    .map_err(|error| component_rewrite_error(error, ExpressionErrorContext::Model))
}

#[derive(Clone, Copy)]
enum Binding {
    Let,
    Pattern,
    Closed,
}

impl Renamer<'_> {
    /// One declaration operation for lets, patterns and every binder form.
    fn bind(
        &mut self,
        name: String,
        scope: &mut Scope,
        binding: Binding,
    ) -> Result<String, TypecheckError> {
        if matches!(binding, Binding::Pattern) {
            reject_pattern_binding_name(
                &name,
                self.source.globals.state.iter().map(String::as_str),
                self.source.globals.consts.iter().map(String::as_str),
                self.source.globals.enum_members.iter().map(String::as_str),
                scope.original.iter().map(String::as_str),
            )?;
        }
        let collides = match binding {
            Binding::Let => self.avoid.contains(&name),
            Binding::Closed => self.incoming.contains(&name),
            Binding::Pattern => {
                self.ambient.contains(&name)
                    || self.incoming.contains(&name)
                    || if scope.exporting {
                        self.avoid.contains(&name)
                    } else {
                        self.pattern_avoid.contains(&name)
                    }
            }
        };
        let fresh = if collides {
            fresh_binding_name(&name, self.used)
        } else {
            name.clone()
        };
        self.used.insert(fresh.clone());
        scope.original.insert(name.clone());
        scope.renamed.insert(name, fresh.clone());
        Ok(fresh)
    }

    fn local(&mut self, expr: Expr, scope: &Scope) -> Result<Expr, TypecheckError> {
        let mut nested = scope.clone();
        nested.exporting = false;
        self.expression(expr, &mut nested)
    }

    fn local_at(&mut self, expr: Expr, scope: &Scope, span: Span) -> Result<Expr, TypecheckError> {
        self.local(expr, scope)
            .map_err(|error| error.with_span(span))
    }

    fn expression(&mut self, expr: Expr, scope: &mut Scope) -> Result<Expr, TypecheckError> {
        crate::recursion::guard(|| self.expression_inner(expr, scope))
    }

    fn expression_inner(&mut self, expr: Expr, scope: &mut Scope) -> Result<Expr, TypecheckError> {
        Ok(match expr {
            Expr::Var(name) => {
                if let Some(renamed) = scope.renamed.get(&name) {
                    Expr::Var(renamed.clone())
                } else if let Some(arguments) = &self.source.arguments {
                    if let Some(value) = arguments.values.get(&name) {
                        // An actual is a separate lexical source. Rewrite each
                        // inserted copy in header scope, never receiver scope.
                        let mut argument_scope = Scope::new(arguments.params);
                        argument_scope.exporting = false;
                        let source = Source {
                            globals: arguments.globals,
                            params: arguments.params,
                            span: arguments.span,
                            arguments: None,
                        };
                        Renamer {
                            source,
                            avoid: arguments.avoid,
                            pattern_avoid: arguments.avoid,
                            ambient: self.ambient,
                            incoming: HashSet::new(),
                            used: &mut *self.used,
                        }
                        .expression(value.clone(), &mut argument_scope)
                        .map_err(|error| error.with_span(arguments.span))?
                    } else {
                        Expr::Var(name)
                    }
                } else {
                    Expr::Var(name)
                }
            }
            Expr::Is {
                expr,
                pattern: Pattern::Some(name),
            } => {
                let mut nested = scope.clone();
                let name = self.bind(name, &mut nested, Binding::Pattern)?;
                let expr = self.local(*expr, scope)?;
                *scope = nested;
                Expr::Is {
                    expr: Box::new(expr),
                    pattern: Pattern::Some(name),
                }
            }
            Expr::Is {
                expr,
                pattern: Pattern::None,
            } => Expr::Is {
                expr: Box::new(self.local(*expr, scope)?),
                pattern: Pattern::None,
            },
            Expr::Binary { op, left, right } => Expr::Binary {
                op,
                left: Box::new(self.expression(*left, scope)?),
                right: Box::new(self.expression(*right, scope)?),
            },
            Expr::Quantified {
                quantifier,
                binder,
                body,
            } => {
                let (binder, nested) = self.binder(binder, scope)?;
                Expr::Quantified {
                    quantifier,
                    binder,
                    body: Box::new(self.local(*body, &nested)?),
                }
            }
            Expr::Aggregate {
                kind,
                binder,
                value,
            } => {
                let (binder, nested) = self.binder(binder, scope)?;
                Expr::Aggregate {
                    kind,
                    binder,
                    value: value
                        .map(|value| self.local(*value, &nested).map(Box::new))
                        .transpose()?,
                }
            }
            expr => map_children(expr, |child, span| match span {
                Some(span) => self.local_at(child, scope, span),
                None => self.local(child, scope),
            })?,
        })
    }

    fn binder(&mut self, binder: Binder, scope: &Scope) -> Result<(Binder, Scope), TypecheckError> {
        let mut nested = scope.clone();
        nested.exporting = false;
        let name = self.bind(
            super::binder_name(&binder).to_owned(),
            &mut nested,
            Binding::Closed,
        )?;
        let binder = match binder {
            Binder::Typed {
                type_name,
                where_expr,
                ..
            } => Binder::Typed {
                name,
                type_name,
                where_expr: where_expr
                    .map(|expr| self.local(*expr, &nested).map(Box::new))
                    .transpose()?,
            },
            Binder::Range {
                lo, hi, where_expr, ..
            } => Binder::Range {
                name,
                lo: Box::new(self.local(*lo, scope)?),
                hi: Box::new(self.local(*hi, scope)?),
                where_expr: where_expr
                    .map(|expr| self.local(*expr, &nested).map(Box::new))
                    .transpose()?,
            },
            Binder::Collection {
                collection,
                where_expr,
                ..
            } => Binder::Collection {
                name,
                collection: Box::new(self.local(*collection, scope)?),
                where_expr: where_expr
                    .map(|expr| self.local(*expr, &nested).map(Box::new))
                    .transpose()?,
            },
        };
        Ok((binder, nested))
    }

    fn statement(
        &mut self,
        statement: Statement,
        scope: &Scope,
    ) -> Result<Statement, TypecheckError> {
        crate::recursion::guard(|| self.statement_inner(statement, scope))
    }

    fn statement_inner(
        &mut self,
        statement: Statement,
        scope: &Scope,
    ) -> Result<Statement, TypecheckError> {
        Ok(match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => Statement::Assign {
                target: self
                    .lvalue(target, scope)
                    .map_err(|error| error.with_span(span))?,
                value: self.local_at(value, scope, span)?,
                span,
            },
            Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => Statement::If {
                condition: self.local_at(condition, scope, span)?,
                then_statements: self.statements(then_statements, scope)?,
                else_statements: self.statements(else_statements, scope)?,
                span,
            },
            Statement::ForAll {
                binder,
                statements,
                span,
            } => {
                let (binder, nested) = self
                    .binder(binder, scope)
                    .map_err(|error| error.with_span(span))?;
                Statement::ForAll {
                    binder,
                    statements: self.statements(statements, &nested)?,
                    span,
                }
            }
        })
    }

    fn statements(
        &mut self,
        statements: Vec<Statement>,
        scope: &Scope,
    ) -> Result<Vec<Statement>, TypecheckError> {
        statements
            .into_iter()
            .map(|statement| self.statement(statement, scope))
            .collect()
    }

    fn lvalue(&mut self, value: LValue, scope: &Scope) -> Result<LValue, TypecheckError> {
        Ok(match value {
            LValue::Var(name) => LValue::Var(name),
            LValue::Index(name, index) => LValue::Index(name, self.local(index, scope)?),
            LValue::Field(base, field) => {
                LValue::Field(Box::new(self.lvalue(*base, scope)?), field)
            }
        })
    }
}

/// Ordinary children never export a pattern declaration into their parent.
pub(super) fn map_children<E>(
    expr: Expr,
    mut map: impl FnMut(Expr, Option<Span>) -> Result<Expr, E>,
) -> Result<Expr, E> {
    let mut child = |expr| map(expr, None);
    Ok(match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::EnumMember { .. } => expr,
        Expr::Some(value) => Expr::Some(Box::new(child(*value)?)),
        Expr::Set(values) => Expr::Set(
            values
                .into_iter()
                .map(&mut child)
                .collect::<Result<_, _>>()?,
        ),
        Expr::Seq(values) => Expr::Seq(
            values
                .into_iter()
                .map(&mut child)
                .collect::<Result<_, _>>()?,
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, value)| Ok((name, child(value)?)))
                .collect::<Result<_, E>>()?,
        },
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args.into_iter().map(&mut child).collect::<Result<_, _>>()?,
            span,
        },
        Expr::Index(base, index) => Expr::Index(Box::new(child(*base)?), Box::new(child(*index)?)),
        Expr::Field(base, field) => Expr::Field(Box::new(child(*base)?), field),
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(child(*receiver)?),
            name,
            args: args.into_iter().map(&mut child).collect::<Result<_, _>>()?,
        },
        Expr::Neg(value) => Expr::Neg(Box::new(child(*value)?)),
        Expr::Not(value) => Expr::Not(Box::new(child(*value)?)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            condition: Box::new(map(*condition, Some(spans.condition))?),
            then_expr: Box::new(map(*then_expr, Some(spans.then_expr))?),
            else_expr: Box::new(map(*else_expr, Some(spans.else_expr))?),
            spans,
        },
        Expr::Stage {
            process,
            entity,
            entity_span,
            span,
        } => Expr::Stage {
            process,
            entity: Box::new(child(*entity)?),
            entity_span,
            span,
        },
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(child(*expr)?),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(child(*left)?),
            right: Box::new(child(*right)?),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(child(*first)?),
            second: Box::new(child(*second)?),
            third: Box::new(child(*third)?),
        },
        Expr::Var(_)
        | Expr::Is { .. }
        | Expr::Binary { .. }
        | Expr::Quantified { .. }
        | Expr::Aggregate { .. } => {
            unreachable!("binding expressions are handled by the source rewriter")
        }
    })
}
