// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SourcePos {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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

impl QualifiedName {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        self.namespace.as_ref().map_or_else(
            || Value::String(self.name.clone()),
            |namespace| json!(["qname", namespace, self.name]),
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
    },
    Collection {
        name: String,
        collection: Box<Expr>,
        where_expr: Option<Box<Expr>>,
    },
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
    IfThenElse {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
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
    Count {
        name: String,
        type_name: QualifiedName,
        condition: Box<Expr>,
    },
    Sum {
        name: String,
        type_name: QualifiedName,
        body: Box<Expr>,
        condition: Option<Box<Expr>>,
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
    BinderNamed {
        name: String,
        binder: Binder,
    },
}

impl Pattern {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::None => json!(["pat_none"]),
            Self::Some(name) => json!(["pat_some", name]),
        }
    }
}

impl Binder {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Typed {
                name,
                type_name,
                where_expr,
            } => json!([
                "binder_typed",
                name,
                type_name.python_ast(),
                where_expr.as_deref().map(Expr::python_ast)
            ]),
            Self::Range { name, lo, hi } => {
                json!(["binder_range", name, lo.python_ast(), hi.python_ast()])
            }
            Self::Collection {
                name,
                collection,
                where_expr,
            } => json!([
                "binder_collection",
                name,
                collection.python_ast(),
                where_expr.as_deref().map(Expr::python_ast)
            ]),
        }
    }
}

impl Expr {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Num(value) => json!(["num", value]),
            Self::Bool(value) => json!(["bool", value]),
            Self::None => json!(["none"]),
            Self::Some(expr) => json!(["some", expr.python_ast()]),
            Self::Set(items) => json!(["set_lit", ast_list(items)]),
            Self::Seq(items) => json!(["seq_lit", ast_list(items)]),
            Self::Struct { name, fields } => {
                let object = fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.python_ast()))
                    .collect::<serde_json::Map<_, _>>();
                json!(["struct_lit", name, object])
            }
            Self::Var(name) => json!(["var", name]),
            Self::Call { name, args, span } => {
                json!(["call", name, ast_list(args), span.python_loc()])
            }
            Self::Index(base, index) => json!(["index", base.python_ast(), index.python_ast()]),
            Self::Field(base, name) => json!(["field", base.python_ast(), name]),
            Self::Method {
                receiver,
                name,
                args,
            } => json!(["method", receiver.python_ast(), name, ast_list(args)]),
            Self::Binary { op, left, right } => {
                json!(["bin", op, left.python_ast(), right.python_ast()])
            }
            Self::Neg(expr) => json!(["neg", expr.python_ast()]),
            Self::Not(expr) => json!(["not", expr.python_ast()]),
            Self::IfThenElse {
                condition,
                then_expr,
                else_expr,
            } => json!([
                "ite",
                condition.python_ast(),
                then_expr.python_ast(),
                else_expr.python_ast()
            ]),
            Self::Is { expr, pattern } => {
                json!(["is", expr.python_ast(), pattern.python_ast()])
            }
            Self::Quantified {
                quantifier,
                binder,
                body,
            } => json!([quantifier, binder.python_ast(), body.python_ast()]),
            Self::Count {
                name,
                type_name,
                condition,
            } => json!([
                "count",
                name,
                type_name.python_ast(),
                condition.python_ast()
            ]),
            Self::Sum {
                name,
                type_name,
                body,
                condition,
            } => json!([
                "sum",
                name,
                type_name.python_ast(),
                body.python_ast(),
                condition.as_deref().map(Expr::python_ast)
            ]),
            Self::UnaryNamed { name, expr, span } => match name.as_str() {
                "stage" => json!(["stage", expr.python_ast(), span.python_loc()]),
                "old" | "abs" => json!([name, expr.python_ast()]),
                "rel_acyclic" | "rel_functional" | "rel_injective" | "rel_domain" | "rel_range" => {
                    json!([name, expr.python_ast()])
                }
                _ => unreachable!("validated named unary expression"),
            },
            Self::BinaryNamed { name, left, right } => {
                json!([name, left.python_ast(), right.python_ast()])
            }
            Self::TernaryNamed {
                name,
                first,
                second,
                third,
            } => json!([
                name,
                first.python_ast(),
                second.python_ast(),
                third.python_ast()
            ]),
            Self::BinderNamed { name, binder } => json!([name, binder.python_ast()]),
        }
    }
}

fn ast_list(items: &[Expr]) -> Vec<Value> {
    items.iter().map(Expr::python_ast).collect()
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
