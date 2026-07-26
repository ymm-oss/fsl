// SPDX-License-Identifier: Apache-2.0

//! Reserved words that cannot name a declaration (issue #570).
//!
//! The reserved set is derived from the native expression parser, not copied
//! from the frozen reference. `fsl_syntax::syntax_expr`'s `atom()` intercepts a
//! few identifiers before they can become a name reference. Three of them are
//! matched *unconditionally* and become a literal:
//!
//! - `true` / `false` -> `SyntaxExprKind::Bool`
//! - `none` -> `SyntaxExprKind::None`
//!
//! A declaration with one of those names is therefore unreadable from any
//! expression, and — this is the reason the check exists — the misreading is
//! *silent*. `spec ShadowTrue { state { true: Bool } init { true = false }
//! invariant AlwaysHolds { true } }` returned `proved`: the author wrote "the
//! variable holds" and the verifier proved the literal. `--engine explicit`
//! agreed, so symbolic/concrete/BFS agreement does not catch it — all three
//! share the misreading.
//!
//! The parser's other intercepted identifiers are *conditional*: `some`, `Set`,
//! `Seq`, `unique`, `exactlyOne`, `forall`, and `exists` are matched only when
//! followed by more syntax, so a bare reference to a declaration with one of
//! those names is a loud parse error rather than a silent literal. And
//! `count`/`sum`/`stage`/`in`/`is`/`where`/`old`/`abs`/`and`/`or` parse
//! unambiguously as bare identifiers and were measured to read back correctly.
//! None of those are reserved: reserving a word that works today breaks valid
//! specifications, and for an author whose spec legitimately uses one that is
//! worse than the bug.

use fsl_syntax::{Binder, Expr, Param, Span, SpecItem, SurfaceSpec};

use crate::model::ModelError;

/// Words the expression parser resolves to a literal, so they can never be read
/// back as a name.
pub const RESERVED_NAMES: [&str; 3] = ["true", "false", "none"];

/// Whether `name` is a reserved word that cannot name a declaration.
#[must_use]
pub fn is_reserved(name: &str) -> bool {
    RESERVED_NAMES.contains(&name)
}

/// The one wording every reserved-name diagnostic uses, wherever it is raised.
pub(crate) fn reserved_message(name: &str, position: &str) -> String {
    format!(
        "'{name}' is a reserved FSL keyword and cannot be used as a {position} name; \
         every occurrence in an expression resolves to the literal, so the declaration \
         would be unreadable"
    )
}

fn reject(name: &str, position: &str, span: Option<Span>) -> ModelError {
    ModelError {
        message: reserved_message(name, position),
        origin: None,
        span,
    }
}

fn check(name: &str, position: &str, span: Option<Span>) -> Result<(), ModelError> {
    if is_reserved(name) {
        return Err(reject(name, position, span));
    }
    Ok(())
}

fn check_binder(binder: &Binder, span: Option<Span>) -> Result<(), ModelError> {
    let name = match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    };
    check(name, "binder", span)?;
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(expr) = where_expr {
                check_expr(expr, span)?;
            }
        }
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            check_expr(lo, span)?;
            check_expr(hi, span)?;
            if let Some(expr) = where_expr {
                check_expr(expr, span)?;
            }
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            check_expr(collection, span)?;
            if let Some(expr) = where_expr {
                check_expr(expr, span)?;
            }
        }
    }
    Ok(())
}

/// Walk an expression for the name-introducing positions it can contain:
/// quantifier and aggregate binders, and an `is some(x)` pattern binding.
fn check_expr(expr: &Expr, span: Option<Span>) -> Result<(), ModelError> {
    match expr {
        Expr::Quantified { binder, body, .. } => {
            check_binder(binder, span)?;
            check_expr(body, span)?;
        }
        Expr::Aggregate { binder, value, .. } => {
            check_binder(binder, span)?;
            if let Some(value) = value {
                check_expr(value, span)?;
            }
        }
        Expr::Is { expr, pattern } => {
            if let fsl_syntax::Pattern::Some(name) = pattern {
                check(name, "pattern binding", span)?;
            }
            check_expr(expr, span)?;
        }
        _ => {
            for child in children(expr) {
                check_expr(child, span)?;
            }
        }
    }
    Ok(())
}

/// The sub-expressions of a non-binding node, so the walk reaches a binder
/// nested anywhere inside an expression.
///
/// Deliberately exhaustive with no catch-all arm: a new `Expr` variant must
/// fail to compile here rather than silently become a hole this check does not
/// look through. `crate::visit_expr_children` covers the same shape but its
/// visitor returns `CoreError`, which mandates a concrete line/column, and
/// issue 555 forbids inventing one for a construct that carries no span.
fn children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Some(inner) | Expr::Neg(inner) | Expr::Not(inner) | Expr::Field(inner, _) => {
            vec![inner]
        }
        Expr::Set(items) | Expr::Seq(items) => items.iter().collect(),
        Expr::Struct { fields, .. } => fields.iter().map(|(_, value)| value).collect(),
        Expr::Call { args, .. } => args.iter().collect(),
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => vec![left, right],
        Expr::Method { receiver, args, .. } => {
            let mut out = vec![receiver.as_ref()];
            out.extend(args.iter());
            out
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => vec![condition, then_expr, else_expr],
        Expr::Stage { entity: inner, .. } | Expr::UnaryNamed { expr: inner, .. } => vec![inner],
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => vec![first, second, third],
        // Handled by `check_expr` before it delegates here.
        Expr::Quantified { .. } | Expr::Aggregate { .. } | Expr::Is { .. } => Vec::new(),
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {
            Vec::new()
        }
    }
}

fn check_param(param: &Param, span: Option<Span>) -> Result<(), ModelError> {
    match param {
        Param::Typed(name, _) => check(name, "parameter", span),
        Param::Range(name, lo, hi) => {
            check(name, "parameter", span)?;
            check_expr(lo, span)?;
            check_expr(hi, span)
        }
    }
}

/// Reject a reserved word in every position of `spec` that introduces a name.
///
/// Runs on the *lowered* surface spec, so a name a dialect generates is checked
/// on the same footing as one an author wrote.
///
/// # Errors
///
/// Returns [`ModelError`] naming the reserved word and the position that used
/// it.
pub(crate) fn check_reserved_names(spec: &SurfaceSpec) -> Result<(), ModelError> {
    check(&spec.name, "specification", None)?;
    for item in &spec.items {
        match item {
            SpecItem::Const { name, value } => {
                check(name, "const", None)?;
                check_expr(value, None)?;
            }
            SpecItem::Def {
                name,
                params,
                value,
                span,
            } => {
                check(name, "def", Some(*span))?;
                for (param, _) in params {
                    check(param, "def parameter", Some(*span))?;
                }
                check_expr(value, Some(*span))?;
            }
            SpecItem::Type { name, .. } => check(name, "type", None)?,
            SpecItem::Enum { name, members, .. } => {
                check(name, "enum", None)?;
                for member in members {
                    check(member, "enum member", None)?;
                }
            }
            SpecItem::Struct { name, fields, span } => {
                check(name, "struct", Some(*span))?;
                for (field, _) in fields {
                    check(field, "struct field", Some(*span))?;
                }
            }
            SpecItem::Entity(name, span) => check(name, "entity", Some(*span))?,
            SpecItem::Number(name, span) => check(name, "number", Some(*span))?,
            SpecItem::State(fields) => {
                for field in fields {
                    check(&field.name, "state variable", Some(field.span))?;
                    if let Some(initializer) = &field.initializer {
                        check_expr(initializer, Some(field.span))?;
                    }
                }
            }
            SpecItem::Action {
                name, params, span, ..
            } => {
                check(name, "action", Some(*span))?;
                for param in params {
                    check_param(param, Some(*span))?;
                }
            }
            SpecItem::Invariant {
                name, expr, span, ..
            }
            | SpecItem::Trans {
                name, expr, span, ..
            }
            | SpecItem::Reachable {
                name, expr, span, ..
            } => {
                check(name, "property", Some(*span))?;
                check_expr(expr, Some(*span))?;
            }
            SpecItem::Until {
                name,
                before,
                after,
                span,
                ..
            }
            | SpecItem::Unless {
                name,
                before,
                after,
                span,
                ..
            } => {
                check(name, "property", Some(*span))?;
                check_expr(before, Some(*span))?;
                check_expr(after, Some(*span))?;
            }
            SpecItem::LeadsTo {
                name,
                binders,
                before,
                after,
                span,
                ..
            } => {
                check(name, "property", Some(*span))?;
                for binder in binders {
                    check_binder(binder, Some(*span))?;
                }
                check_expr(before, Some(*span))?;
                check_expr(after, Some(*span))?;
            }
            SpecItem::Terminal { expr, span } => check_expr(expr, Some(*span))?,
            SpecItem::Init { .. } | SpecItem::VerifyBounds { .. } => {}
        }
    }
    Ok(())
}
