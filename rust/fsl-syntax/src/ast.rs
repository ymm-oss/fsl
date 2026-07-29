// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use crate::SymbolPath;
use crate::recursion;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePos {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: SourcePos,
    pub end: SourcePos,
}

impl Span {
    #[must_use]
    pub fn python_loc(self) -> Value {
        // Python preserves insertion order when serializing this public shape.
        // Keep the same order so byte-oriented reports such as `ledger` remain
        // identical across the Python and Rust implementations.
        json!({"line": self.start.line, "column": self.start.column})
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pattern {
    None,
    Some(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualifiedName {
    pub namespace: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionalSpans {
    pub condition: Span,
    pub then_expr: Span,
    pub else_expr: Span,
}

impl QualifiedName {
    #[must_use]
    pub fn kernel_ast_v1(&self) -> Value {
        self.namespace.as_ref().map_or_else(
            || Value::String(self.name.clone()),
            |namespace| {
                Value::Array(vec![
                    Value::from("qname"),
                    Value::from(namespace.as_str()),
                    Value::from(self.name.as_str()),
                ])
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Binder {
    Typed {
        name: String,
        type_name: QualifiedName,
        where_expr: Option<Box<Expr>>,
    },
    Range {
        name: String,
        lo: Box<Expr>,
        hi: Box<Expr>,
        where_expr: Option<Box<Expr>>,
    },
    Collection {
        name: String,
        collection: Box<Expr>,
        where_expr: Option<Box<Expr>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AggregateKind {
    Count,
    Sum,
    Unique,
    ExactlyOne,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expr {
    Num(i64),
    Bool(bool),
    None,
    Some(Box<Expr>),
    Set(Vec<Expr>),
    Seq(Vec<Expr>),
    Struct {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    Var(String),
    EnumMember {
        type_name: String,
        member: String,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Method {
        receiver: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        spans: Box<ConditionalSpans>,
    },
    Is {
        expr: Box<Expr>,
        pattern: Pattern,
    },
    Quantified {
        quantifier: String,
        binder: Binder,
        body: Box<Expr>,
    },
    Aggregate {
        kind: AggregateKind,
        binder: Binder,
        value: Option<Box<Expr>>,
    },
    Stage {
        process: Option<Box<SymbolPath>>,
        entity: Box<Expr>,
        entity_span: Span,
        span: Span,
    },
    UnaryNamed {
        name: String,
        expr: Box<Expr>,
        span: Span,
    },
    BinaryNamed {
        name: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    TernaryNamed {
        name: String,
        first: Box<Expr>,
        second: Box<Expr>,
        third: Box<Expr>,
    },
}

impl Pattern {
    #[must_use]
    pub fn kernel_ast_v1(&self) -> Value {
        match self {
            Self::None => Value::Array(vec![Value::from("pat_none")]),
            Self::Some(name) => {
                Value::Array(vec![Value::from("pat_some"), Value::from(name.as_str())])
            }
        }
    }
}

impl Binder {
    #[must_use]
    pub fn kernel_ast_v1(&self) -> Value {
        match self {
            Self::Typed {
                name,
                type_name,
                where_expr,
            } => Value::Array(vec![
                Value::from("binder_typed"),
                Value::from(name.as_str()),
                type_name.kernel_ast_v1(),
                optional(where_expr.as_deref().map(Expr::kernel_ast_v1)),
            ]),
            Self::Range {
                name,
                lo,
                hi,
                where_expr,
            } => where_expr.as_deref().map_or_else(
                || {
                    Value::Array(vec![
                        Value::from("binder_range"),
                        Value::from(name.as_str()),
                        lo.kernel_ast_v1(),
                        hi.kernel_ast_v1(),
                    ])
                },
                |where_expr| {
                    Value::Array(vec![
                        Value::from("binder_range"),
                        Value::from(name.as_str()),
                        lo.kernel_ast_v1(),
                        hi.kernel_ast_v1(),
                        where_expr.kernel_ast_v1(),
                    ])
                },
            ),
            Self::Collection {
                name,
                collection,
                where_expr,
            } => Value::Array(vec![
                Value::from("binder_collection"),
                Value::from(name.as_str()),
                collection.kernel_ast_v1(),
                optional(where_expr.as_deref().map(Expr::kernel_ast_v1)),
            ]),
        }
    }
}

impl Expr {
    /// Cycle entry for the JSON AST projection, and where `recursion::guard`
    /// belongs: every arm below recurses back through here, as do the `Binder`
    /// and `TypeExpr` projections above.
    ///
    /// Measured: `analyze` on the N=400 witness, with this function as the
    /// innermost frame. Its consumers are wider than `analyze` alone -- both
    /// spec digests (`fslc::approval::spec_digest`,
    /// `fsl_tools::document_digest::spec_digest_from_kernel`), the requirements
    /// document's claim projection, and testgen's `expect` all project through
    /// it -- so the residual `serde_json` recursion described in
    /// `docs/DESIGN-rust-component-internals.md` 4.4 (#622) is not an
    /// `analyze`-only problem. The witness only reaches it via `analyze`
    /// because the deep expression lives in the mapping file, not in a spec.
    #[must_use]
    pub fn kernel_ast_v1(&self) -> Value {
        recursion::guard(|| self.kernel_ast_v1_inner())
    }

    #[allow(clippy::too_many_lines)]
    fn kernel_ast_v1_inner(&self) -> Value {
        match self {
            Self::Num(value) => Value::Array(vec![Value::from("num"), Value::from(*value)]),
            Self::Bool(value) => Value::Array(vec![Value::from("bool"), Value::from(*value)]),
            Self::None => Value::Array(vec![Value::from("none")]),
            Self::Some(expr) => Value::Array(vec![Value::from("some"), expr.kernel_ast_v1()]),
            Self::Set(items) => {
                Value::Array(vec![Value::from("set_lit"), Value::Array(ast_list(items))])
            }
            Self::Seq(items) => {
                Value::Array(vec![Value::from("seq_lit"), Value::Array(ast_list(items))])
            }
            Self::Struct { name, fields } => {
                let object = fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.kernel_ast_v1()))
                    .collect::<serde_json::Map<_, _>>();
                Value::Array(vec![
                    Value::from("struct_lit"),
                    Value::from(name.as_str()),
                    Value::Object(object),
                ])
            }
            Self::Var(name) => Value::Array(vec![Value::from("var"), Value::from(name.as_str())]),
            Self::EnumMember { type_name, member } => Value::Array(vec![
                Value::from("enum_member"),
                Value::from(type_name.as_str()),
                Value::from(member.as_str()),
            ]),
            Self::Call { name, args, span } => Value::Array(vec![
                Value::from("call"),
                Value::from(name.as_str()),
                Value::Array(ast_list(args)),
                span.python_loc(),
            ]),
            Self::Index(base, index) => Value::Array(vec![
                Value::from("index"),
                base.kernel_ast_v1(),
                index.kernel_ast_v1(),
            ]),
            Self::Field(base, name) => Value::Array(vec![
                Value::from("field"),
                base.kernel_ast_v1(),
                Value::from(name.as_str()),
            ]),
            Self::Method {
                receiver,
                name,
                args,
            } => Value::Array(vec![
                Value::from("method"),
                receiver.kernel_ast_v1(),
                Value::from(name.as_str()),
                Value::Array(ast_list(args)),
            ]),
            Self::Binary { op, left, right } => Value::Array(vec![
                Value::from("bin"),
                Value::from(op.as_str()),
                left.kernel_ast_v1(),
                right.kernel_ast_v1(),
            ]),
            Self::Neg(expr) => Value::Array(vec![Value::from("neg"), expr.kernel_ast_v1()]),
            Self::Not(expr) => Value::Array(vec![Value::from("not"), expr.kernel_ast_v1()]),
            Self::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => Value::Array(vec![
                Value::from("ite"),
                condition.kernel_ast_v1(),
                then_expr.kernel_ast_v1(),
                else_expr.kernel_ast_v1(),
            ]),
            Self::Is { expr, pattern } => Value::Array(vec![
                Value::from("is"),
                expr.kernel_ast_v1(),
                pattern.kernel_ast_v1(),
            ]),
            Self::Quantified {
                quantifier,
                binder,
                body,
            } => Value::Array(vec![
                Value::from(quantifier.as_str()),
                binder.kernel_ast_v1(),
                body.kernel_ast_v1(),
            ]),
            Self::Aggregate {
                kind,
                binder,
                value,
            } => match (kind, binder, value.as_deref()) {
                (
                    AggregateKind::Count,
                    Binder::Typed {
                        name,
                        type_name,
                        where_expr: Some(condition),
                    },
                    None,
                ) => Value::Array(vec![
                    Value::from("count"),
                    Value::from(name.as_str()),
                    type_name.kernel_ast_v1(),
                    condition.kernel_ast_v1(),
                ]),
                (
                    AggregateKind::Sum,
                    Binder::Typed {
                        name,
                        type_name,
                        where_expr,
                    },
                    Some(body),
                ) => Value::Array(vec![
                    Value::from("sum"),
                    Value::from(name.as_str()),
                    type_name.kernel_ast_v1(),
                    body.kernel_ast_v1(),
                    optional(where_expr.as_deref().map(Expr::kernel_ast_v1)),
                ]),
                (AggregateKind::Unique, binder, None) => {
                    Value::Array(vec![Value::from("unique"), binder.kernel_ast_v1()])
                }
                (AggregateKind::ExactlyOne, binder, None) => {
                    Value::Array(vec![Value::from("exactly_one"), binder.kernel_ast_v1()])
                }
                _ => Value::Array(vec![
                    Value::from("aggregate"),
                    Value::from(match kind {
                        AggregateKind::Count => "count",
                        AggregateKind::Sum => "sum",
                        AggregateKind::Unique => "unique",
                        AggregateKind::ExactlyOne => "exactly_one",
                    }),
                    binder.kernel_ast_v1(),
                    optional(value.as_deref().map(Expr::kernel_ast_v1)),
                ]),
            },
            Self::Stage {
                process,
                entity,
                span,
                ..
            } => process.as_ref().map_or_else(
                || {
                    Value::Array(vec![
                        Value::from("stage"),
                        entity.kernel_ast_v1(),
                        span.python_loc(),
                    ])
                },
                |process| {
                    Value::Array(vec![
                        Value::from("qualified_stage"),
                        Value::from(process.to_string()),
                        entity.kernel_ast_v1(),
                        span.python_loc(),
                    ])
                },
            ),
            Self::UnaryNamed {
                name,
                expr,
                span: _,
            } => match name.as_str() {
                // Two families -- the value/temporal `old`/`abs` and the
                // relation predicates -- project identically, as `[name, arg]`.
                // They were separate arms with the same body until the longer
                // method name made rustfmt render both as blocks and
                // `clippy::match_same_arms` saw the duplication it had been
                // hiding. The list stays exhaustive so an unvalidated name
                // still reaches `unreachable!` rather than a silent projection.
                "old" | "abs" | "rel_acyclic" | "rel_functional" | "rel_injective"
                | "rel_domain" | "rel_range" => {
                    Value::Array(vec![Value::from(name.as_str()), expr.kernel_ast_v1()])
                }
                _ => unreachable!("validated named unary expression"),
            },
            Self::BinaryNamed { name, left, right } => Value::Array(vec![
                Value::from(name.as_str()),
                left.kernel_ast_v1(),
                right.kernel_ast_v1(),
            ]),
            Self::TernaryNamed {
                name,
                first,
                second,
                third,
            } => Value::Array(vec![
                Value::from(name.as_str()),
                first.kernel_ast_v1(),
                second.kernel_ast_v1(),
                third.kernel_ast_v1(),
            ]),
        }
    }
}

fn ast_list(items: &[Expr]) -> Vec<Value> {
    items.iter().map(Expr::kernel_ast_v1).collect()
}

/// A missing optional child projects as JSON `null`.
///
/// `json!` produced this from an interpolated `Option<Value>`; direct
/// construction has to say it, and saying it in one place keeps the four
/// optional-child sites from drifting apart.
fn optional(child: Option<Value>) -> Value {
    child.unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::{SourcePos, Span};

    #[test]
    fn python_location_uses_python_key_order() {
        let position = SourcePos {
            offset: 0,
            line: 34,
            column: 3,
        };
        let location = Span {
            start: position,
            end: position,
        }
        .python_loc();

        assert_eq!(location.to_string(), r#"{"line":34,"column":3}"#);
    }
}
