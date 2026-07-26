// SPDX-License-Identifier: Apache-2.0

//! Render a checked expression back into readable FSL-like source text.
//!
//! Moved here from `fslc` (the native CLI crate) so `fsl-tools` can reuse it
//! too (issue #326's controlled-language renderer needs the same canonical
//! fallback text `explain --readable` already produces) without creating a
//! `fsl-tools -> fslc` dependency edge in the wrong direction. `fslc` re-exports
//! `expr_text`/`source_expr_text` from here; call sites and output are
//! unchanged.

use std::fmt::Write as _;

use fsl_syntax::{AggregateKind, Binder, Expr, Pattern};

use crate::{KernelModel, display_name};

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn expr_text(expr: &Expr) -> String {
    expr_text_with_origins(None, expr)
}

/// Render an expression with source-level names recovered from lowering origins.
#[must_use]
pub fn source_expr_text(model: &KernelModel, expr: &Expr) -> String {
    expr_text_with_origins(Some(model), expr)
}

/// Render a binder (`c: Sub`, `i in 0..3`, `x in collection`, each with an
/// optional ` where ...` filter) the same way [`expr_text`] renders the
/// binder inside a `Quantified`/`Aggregate` expression. Exposed for the
/// controlled-language renderer (issue #326), which needs the identical
/// canonical binder text outside of a full quantifier expression.
#[must_use]
pub fn binder_text(binder: &Binder) -> String {
    binder_text_with_origins(None, binder)
}

/// [`binder_text`] with source-level names recovered from lowering origins.
#[must_use]
pub fn source_binder_text(model: &KernelModel, binder: &Binder) -> String {
    binder_text_with_origins(Some(model), binder)
}

fn precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Conditional { .. } | Expr::Quantified { .. } => 0,
        Expr::Binary { op, .. } => match op.as_str() {
            "=>" => 1,
            "or" => 2,
            "and" => 3,
            "==" | "!=" | "<" | "<=" | ">" | ">=" => 6,
            "+" | "-" => 7,
            "*" | "/" | "%" => 8,
            _ => 10,
        },
        Expr::Not(_) => 4,
        Expr::Is { .. } => 5,
        Expr::Neg(_) => 9,
        _ => 10,
    }
}

fn operand(model: Option<&KernelModel>, expr: &Expr, minimum: u8) -> String {
    let rendered = expr_text_with_origins(model, expr);
    if precedence(expr) < minimum {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn binder_text_with_origins(model: Option<&KernelModel>, binder: &Binder) -> String {
    let mut text = binder_base_text(model, binder);
    if let Some(condition) = binder_filter(binder) {
        let _ = write!(text, " where {}", expr_text_with_origins(model, condition));
    }
    text
}

fn binder_base_text(model: Option<&KernelModel>, binder: &Binder) -> String {
    match binder {
        Binder::Typed {
            name, type_name, ..
        } => format!("{name}: {}", display_name(&type_name.name)),
        Binder::Range { name, lo, hi, .. } => {
            format!(
                "{name} in {}..{}",
                expr_text_with_origins(model, lo),
                expr_text_with_origins(model, hi)
            )
        }
        Binder::Collection {
            name, collection, ..
        } => format!("{name} in {}", expr_text_with_origins(model, collection)),
    }
}

fn binder_filter(binder: &Binder) -> Option<&Expr> {
    match binder {
        Binder::Typed { where_expr, .. }
        | Binder::Range { where_expr, .. }
        | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
    }
}

#[allow(clippy::too_many_lines)]
fn expr_text_with_origins(model: Option<&KernelModel>, expr: &Expr) -> String {
    match expr {
        Expr::Num(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::None => "none".to_owned(),
        Expr::Some(value) => format!("some({})", expr_text_with_origins(model, value)),
        Expr::Set(values) => format!(
            "Set {{{}}}",
            values
                .iter()
                .map(|value| expr_text_with_origins(model, value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Seq(values) => format!(
            "Seq {{{}}}",
            values
                .iter()
                .map(|value| expr_text_with_origins(model, value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Struct { name, fields } => format!(
            "{} {{ {} }}",
            display_name(name),
            {
                let mut fields = fields.iter().collect::<Vec<_>>();
                fields.sort_by_key(|(name, _)| name.as_str());
                fields
            }
            .into_iter()
            .map(|(name, value)| { format!("{name}: {}", expr_text_with_origins(model, value)) })
            .collect::<Vec<_>>()
            .join(", ")
        ),
        Expr::Var(name) => display_name(name),
        Expr::EnumMember { type_name, member } => {
            format!("{}.{}", display_name(type_name), display_name(member))
        }
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            display_name(name),
            args.iter()
                .map(|argument| expr_text_with_origins(model, argument))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Index(base, index) => {
            let stage_qualifier = model.and_then(|model| {
                let Expr::Var(name) = base.as_ref() else {
                    return None;
                };
                let origin = model.state_origin(name)?;
                origin
                    .lowering_steps
                    .iter()
                    .any(|step| step.kind == "synthesize_stage_map")
                    .then(|| {
                        origin
                            .lowering_steps
                            .iter()
                            .find(|step| step.kind == "qualified_process_path")
                            .and_then(|step| step.detail.as_deref())
                    })
            });
            if let Some(qualifier) = stage_qualifier {
                format!(
                    "{}stage({})",
                    qualifier.map_or_else(String::new, |path| format!("{path}.")),
                    expr_text_with_origins(model, index)
                )
            } else {
                format!(
                    "{}[{}]",
                    operand(model, base, 10),
                    expr_text_with_origins(model, index)
                )
            }
        }
        Expr::Field(base, field) => format!("{}.{field}", operand(model, base, 10)),
        Expr::Method {
            receiver,
            name,
            args,
        } => format!(
            "{}.{}({})",
            operand(model, receiver, 10),
            name,
            args.iter()
                .map(|argument| expr_text_with_origins(model, argument))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Binary { op, left, right } => {
            let (left_minimum, right_minimum) = match op.as_str() {
                "=>" => (2, 1),
                "or" => (2, 3),
                "and" => (3, 4),
                "==" | "!=" | "<" | "<=" | ">" | ">=" => (7, 7),
                "+" | "-" => (7, 8),
                "*" | "/" | "%" => (8, 9),
                _ => (0, 0),
            };
            format!(
                "{} {op} {}",
                operand(model, left, left_minimum),
                operand(model, right, right_minimum)
            )
        }
        Expr::Neg(value) => format!("-{}", operand(model, value, 9)),
        Expr::Not(value) => format!("not {}", operand(model, value, 4)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => format!(
            "if {} then {} else {}",
            expr_text_with_origins(model, condition),
            expr_text_with_origins(model, then_expr),
            expr_text_with_origins(model, else_expr)
        ),
        Expr::Is { expr, pattern } => match pattern {
            Pattern::None => format!("{} is none", operand(model, expr, 6)),
            Pattern::Some(name) => format!("{} is some({name})", operand(model, expr, 6)),
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => format!(
            "{quantifier} {} {{ {} }}",
            binder_text_with_origins(model, binder),
            expr_text_with_origins(model, body)
        ),
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => match kind {
            AggregateKind::Count => format!("count({})", binder_text_with_origins(model, binder)),
            AggregateKind::Sum => format!(
                "sum({} of {}{})",
                binder_base_text(model, binder),
                expr_text_with_origins(model, value.as_deref().expect("sum has a value")),
                binder_filter(binder).map_or_else(String::new, |filter| {
                    format!(" where {}", expr_text_with_origins(model, filter))
                })
            ),
            AggregateKind::Unique => format!("unique({})", binder_text_with_origins(model, binder)),
            AggregateKind::ExactlyOne => {
                format!("exactlyOne({})", binder_text_with_origins(model, binder))
            }
        },
        Expr::Stage {
            process, entity, ..
        } => process.as_ref().map_or_else(
            || format!("stage({})", expr_text_with_origins(model, entity)),
            |process| format!("{process}.stage({})", expr_text_with_origins(model, entity)),
        ),
        Expr::UnaryNamed { name, expr, .. } => {
            let public = match name.as_str() {
                "rel_acyclic" => "acyclic",
                "rel_functional" => "functional",
                "rel_injective" => "injective",
                "rel_domain" => "domain",
                "rel_range" => "range",
                other => other,
            };
            format!("{public}({})", expr_text_with_origins(model, expr))
        }
        Expr::BinaryNamed { name, left, right } => {
            format!(
                "{}({}, {})",
                name,
                expr_text_with_origins(model, left),
                expr_text_with_origins(model, right)
            )
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => format!(
            "{}({}, {}, {})",
            name,
            expr_text_with_origins(model, first),
            expr_text_with_origins(model, second),
            expr_text_with_origins(model, third)
        ),
    }
}
