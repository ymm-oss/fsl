// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Source-coupled row registries for the expression and type axes.
//!
//! Each macro invocation emits both the exhaustive match over the real enum
//! and the witnesses used to enumerate its rows. Adding an enum variant
//! therefore fails compilation until the same declaration supplies its row
//! and witness; the axis and C6 coverage checks consume these generated rows
//! rather than maintaining independent constructor lists or count floors.

use fsl_core::{TypeDef, TypeRef};
use fsl_syntax::{
    AggregateKind, Binder, ConditionalSpans, Expr, Pattern, QualifiedName, SourcePos, Span,
};

macro_rules! define_variant_rows {
    (
        $label_fn:ident,
        $rows_fn:ident,
        $enum_ty:ty,
        {
            $(
                $pattern:pat => ($label:literal, $witness:expr)
            ),+ $(,)?
        }
    ) => {
        #[must_use]
        pub fn $label_fn(value: &$enum_ty) -> &'static str {
            match value {
                $($pattern => $label),+
            }
        }

        #[must_use]
        pub fn $rows_fn() -> Vec<&'static str> {
            vec![
                $(
                    {
                        let witness: $enum_ty = $witness;
                        $label_fn(&witness)
                    }
                ),+
            ]
        }
    };
}

fn span() -> Span {
    Span {
        start: SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        },
        end: SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        },
    }
}

fn binder() -> Binder {
    Binder::Typed {
        name: "item".to_owned(),
        type_name: QualifiedName {
            namespace: None,
            name: "Item".to_owned(),
        },
        where_expr: None,
    }
}

define_variant_rows!(
    expr_variant_row,
    expr_variant_rows,
    Expr,
    {
        Expr::Num(_) => ("Expr::Num", Expr::Num(0)),
        Expr::Bool(_) => ("Expr::Bool", Expr::Bool(false)),
        Expr::None => ("Expr::None", Expr::None),
        Expr::Some(_) => ("Expr::Some", Expr::Some(Box::new(Expr::Num(0)))),
        Expr::Set(_) => ("Expr::Set", Expr::Set(Vec::new())),
        Expr::Seq(_) => ("Expr::Seq", Expr::Seq(Vec::new())),
        Expr::Struct { .. } => (
            "Expr::Struct",
            Expr::Struct {
                name: "Payload".to_owned(),
                fields: Vec::new(),
            }
        ),
        Expr::Var(_) => ("Expr::Var", Expr::Var("value".to_owned())),
        Expr::EnumMember { .. } => (
            "Expr::EnumMember",
            Expr::EnumMember {
                type_name: "Status".to_owned(),
                member: "Pending".to_owned(),
            }
        ),
        Expr::Call { .. } => (
            "Expr::Call",
            Expr::Call {
                name: "predicate".to_owned(),
                args: Vec::new(),
                span: span(),
            }
        ),
        Expr::Index(_, _) => (
            "Expr::Index",
            Expr::Index(
                Box::new(Expr::Var("map".to_owned())),
                Box::new(Expr::Num(0)),
            )
        ),
        Expr::Field(_, _) => (
            "Expr::Field",
            Expr::Field(
                Box::new(Expr::Var("payload".to_owned())),
                "value".to_owned(),
            )
        ),
        Expr::Method { .. } => (
            "Expr::Method",
            Expr::Method {
                receiver: Box::new(Expr::Var("set".to_owned())),
                name: "size".to_owned(),
                args: Vec::new(),
            }
        ),
        Expr::Binary { .. } => (
            "Expr::Binary",
            Expr::Binary {
                op: "==".to_owned(),
                left: Box::new(Expr::Num(0)),
                right: Box::new(Expr::Num(0)),
            }
        ),
        Expr::Neg(_) => ("Expr::Neg", Expr::Neg(Box::new(Expr::Num(0)))),
        Expr::Not(_) => ("Expr::Not", Expr::Not(Box::new(Expr::Bool(false)))),
        Expr::Conditional { .. } => (
            "Expr::Conditional",
            Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(Expr::Bool(true)),
                else_expr: Box::new(Expr::Bool(false)),
                spans: Box::new(ConditionalSpans {
                    condition: span(),
                    then_expr: span(),
                    else_expr: span(),
                }),
            }
        ),
        Expr::Is { .. } => (
            "Expr::Is",
            Expr::Is {
                expr: Box::new(Expr::None),
                pattern: Pattern::None,
            }
        ),
        Expr::Quantified { .. } => (
            "Expr::Quantified",
            Expr::Quantified {
                quantifier: "forall".to_owned(),
                binder: binder(),
                body: Box::new(Expr::Bool(true)),
            }
        ),
        Expr::Aggregate { .. } => (
            "Expr::Aggregate",
            Expr::Aggregate {
                kind: AggregateKind::Count,
                binder: binder(),
                value: None,
            }
        ),
        Expr::Stage { .. } => (
            "Expr::Stage",
            Expr::Stage {
                process: None,
                entity: Box::new(Expr::Var("item".to_owned())),
                entity_span: span(),
                span: span(),
            }
        ),
        Expr::UnaryNamed { .. } => (
            "Expr::UnaryNamed",
            Expr::UnaryNamed {
                name: "abs".to_owned(),
                expr: Box::new(Expr::Num(0)),
                span: span(),
            }
        ),
        Expr::BinaryNamed { .. } => (
            "Expr::BinaryNamed",
            Expr::BinaryNamed {
                name: "min".to_owned(),
                left: Box::new(Expr::Num(0)),
                right: Box::new(Expr::Num(1)),
            }
        ),
        Expr::TernaryNamed { .. } => (
            "Expr::TernaryNamed",
            Expr::TernaryNamed {
                name: "rel_reachable".to_owned(),
                first: Box::new(Expr::Var("relation".to_owned())),
                second: Box::new(Expr::Num(0)),
                third: Box::new(Expr::Num(1)),
            }
        ),
    }
);

define_variant_rows!(
    aggregate_kind_row,
    aggregate_kind_rows,
    AggregateKind,
    {
        AggregateKind::Count => ("AggregateKind::Count", AggregateKind::Count),
        AggregateKind::Sum => ("AggregateKind::Sum", AggregateKind::Sum),
        AggregateKind::Unique => ("AggregateKind::Unique", AggregateKind::Unique),
        AggregateKind::ExactlyOne => (
            "AggregateKind::ExactlyOne",
            AggregateKind::ExactlyOne
        ),
    }
);

define_variant_rows!(
    type_ref_row,
    type_ref_rows,
    TypeRef,
    {
        TypeRef::Int => ("TypeRef::Int", TypeRef::Int),
        TypeRef::Bool => ("TypeRef::Bool", TypeRef::Bool),
        TypeRef::Named(_) => ("TypeRef::Named", TypeRef::Named("Named".to_owned())),
        TypeRef::Range(_, _) => ("TypeRef::Range", TypeRef::Range(0, 1)),
        TypeRef::Map(_, _) => (
            "TypeRef::Map",
            TypeRef::Map(Box::new(TypeRef::Bool), Box::new(TypeRef::Bool))
        ),
        TypeRef::Relation(_, _) => (
            "TypeRef::Relation",
            TypeRef::Relation(Box::new(TypeRef::Bool), Box::new(TypeRef::Bool))
        ),
        TypeRef::Set(_) => ("TypeRef::Set", TypeRef::Set(Box::new(TypeRef::Bool))),
        TypeRef::Seq(_, _) => (
            "TypeRef::Seq",
            TypeRef::Seq(Box::new(TypeRef::Bool), 1)
        ),
        TypeRef::Option(_) => (
            "TypeRef::Option",
            TypeRef::Option(Box::new(TypeRef::Bool))
        ),
    }
);

define_variant_rows!(
    type_def_row,
    type_def_rows,
    TypeDef,
    {
        TypeDef::Domain { .. } => (
            "TypeDef::Domain",
            TypeDef::Domain {
                lo: 0,
                hi: 1,
                symmetric: false,
            }
        ),
        TypeDef::Enum { .. } => (
            "TypeDef::Enum",
            TypeDef::Enum {
                members: vec!["Member".to_owned()],
                symmetric: false,
            }
        ),
        TypeDef::Struct { .. } => (
            "TypeDef::Struct",
            TypeDef::Struct { fields: Vec::new() }
        ),
    }
);

#[must_use]
#[allow(dead_code)] // Used by assurance_matrix; this file is also compiled by typed_agreement.
pub fn expression_rows() -> Vec<&'static str> {
    expr_variant_rows()
        .into_iter()
        .chain(aggregate_kind_rows())
        .collect()
}

#[must_use]
#[allow(dead_code)] // Used by typed_agreement; this file is also compiled by assurance_matrix.
pub fn checked_expr_variant_rows() -> Vec<&'static str> {
    expr_variant_rows()
        .into_iter()
        .filter(|row| !matches!(*row, "Expr::Call" | "Expr::Stage"))
        .collect()
}

#[must_use]
pub fn type_rows() -> Vec<&'static str> {
    type_ref_rows().into_iter().chain(type_def_rows()).collect()
}
