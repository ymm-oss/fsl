// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Name resolution through a whole expression, shared by the rewrites that
//! spell an entity's fields `Entity.field` in concept conditions, decision
//! tables and event expressions (#1798).

use std::collections::BTreeSet;

use crate::{Binder, Expr, ParseError};

/// How a name-resolving rewrite treats one name reference, given the binder
/// names in scope there: `Some` replaces it, `None` keeps a `Var` or
/// `EnumMember` and looks inside a field read. A replacement must not name a
/// binder in scope, which would capture it (#1798): `resolve` refuses instead.
pub(in crate::parser) type ResolveName<'a> =
    dyn Fn(&Expr, &BTreeSet<String>) -> Result<Option<Expr>, ParseError> + 'a;

/// `expression` rebuilt through every [`Expr`] form, with `resolve` applied
/// to each name reference -- a `Var`, an `EnumMember`, or a field read of a
/// `Var` -- whose head name no quantifier or aggregate binder in scope binds.
/// `bound` holds the binders of the enclosing scope. A binder's domain (a
/// range's bounds, a collection) lies outside its scope; its `where` filter
/// and its body or value lie inside. An `is some(v)` pattern does not shadow
/// (#1552); the lowering refuses a pattern named like a field (#1561).
///
/// The match names every form and has no catch-all, so a form added to
/// [`Expr`] cannot silently skip resolution and leave a field name to bind to
/// an event input of the same name (#1798).
#[allow(clippy::too_many_lines)]
pub(in crate::parser) fn resolve_names(
    expression: Expr,
    bound: &BTreeSet<String>,
    resolve: &ResolveName<'_>,
) -> Result<Expr, ParseError> {
    crate::recursion::guard(|| {
        let recurse = |expression: Expr| resolve_names(expression, bound, resolve);
        let boxed = |expression: Box<Expr>| recurse(*expression).map(Box::new);
        let list = |values: Vec<Expr>| {
            values
                .into_iter()
                .map(recurse)
                .collect::<Result<Vec<_>, _>>()
        };
        let free = |name: &String| !bound.contains(name);
        let reference = match &expression {
            Expr::Var(name)
            | Expr::EnumMember {
                type_name: name, ..
            } => free(name),
            Expr::Field(receiver, _) => matches!(receiver.as_ref(), Expr::Var(name) if free(name)),
            _ => false,
        };
        if reference && let Some(resolved) = resolve(&expression, bound)? {
            return Ok(resolved);
        }
        Ok(match expression {
            leaf @ (Expr::Num(_)
            | Expr::Bool(_)
            | Expr::None
            | Expr::Var(_)
            | Expr::EnumMember { .. }) => leaf,
            Expr::Some(value) => Expr::Some(boxed(value)?),
            Expr::Set(values) => Expr::Set(list(values)?),
            Expr::Seq(values) => Expr::Seq(list(values)?),
            Expr::Struct { name, fields } => Expr::Struct {
                name,
                fields: fields
                    .into_iter()
                    .map(|(field, value)| Ok((field, recurse(value)?)))
                    .collect::<Result<_, ParseError>>()?,
            },
            Expr::Call { name, args, span } => Expr::Call {
                name,
                args: list(args)?,
                span,
            },
            Expr::Index(map, key) => Expr::Index(boxed(map)?, boxed(key)?),
            Expr::Field(receiver, field) => Expr::Field(boxed(receiver)?, field),
            Expr::Method {
                receiver,
                name,
                args,
            } => Expr::Method {
                receiver: boxed(receiver)?,
                name,
                args: list(args)?,
            },
            Expr::Binary { op, left, right } => Expr::Binary {
                op,
                left: boxed(left)?,
                right: boxed(right)?,
            },
            Expr::Neg(value) => Expr::Neg(boxed(value)?),
            Expr::Not(value) => Expr::Not(boxed(value)?),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                spans,
            } => Expr::Conditional {
                condition: boxed(condition)?,
                then_expr: boxed(then_expr)?,
                else_expr: boxed(else_expr)?,
                spans,
            },
            Expr::Is { expr, pattern } => Expr::Is {
                expr: boxed(expr)?,
                pattern,
            },
            Expr::Quantified {
                quantifier,
                binder,
                body,
            } => {
                let (binder, scope) = resolve_binder(binder, bound, resolve)?;
                Expr::Quantified {
                    quantifier,
                    binder,
                    body: Box::new(resolve_names(*body, &scope, resolve)?),
                }
            }
            Expr::Aggregate {
                kind,
                binder,
                value,
            } => {
                let (binder, scope) = resolve_binder(binder, bound, resolve)?;
                Expr::Aggregate {
                    kind,
                    binder,
                    value: value
                        .map(|value| resolve_names(*value, &scope, resolve).map(Box::new))
                        .transpose()?,
                }
            }
            Expr::Stage {
                process,
                entity,
                entity_span,
                span,
            } => Expr::Stage {
                process,
                entity: boxed(entity)?,
                entity_span,
                span,
            },
            Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
                name,
                expr: boxed(expr)?,
                span,
            },
            Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
                name,
                left: boxed(left)?,
                right: boxed(right)?,
            },
            Expr::TernaryNamed {
                name,
                first,
                second,
                third,
            } => Expr::TernaryNamed {
                name,
                first: boxed(first)?,
                second: boxed(second)?,
                third: boxed(third)?,
            },
        })
    })
}

/// The bare names `expression` reads that no binder inside it binds.
pub(in crate::parser) fn free_names(expression: &Expr) -> BTreeSet<String> {
    let names = std::cell::RefCell::new(BTreeSet::new());
    resolve_names(expression.clone(), &BTreeSet::new(), &|reference, _| {
        if let Expr::Var(name) = reference {
            names.borrow_mut().insert(name.clone());
        }
        Ok(None)
    })
    .expect("collecting names refuses none");
    names.into_inner()
}

/// `binder` with its domain resolved in the enclosing scope `bound` and its
/// filter in its own scope, and that scope: `bound` with the binder's name.
fn resolve_binder(
    binder: Binder,
    bound: &BTreeSet<String>,
    resolve: &ResolveName<'_>,
) -> Result<(Binder, BTreeSet<String>), ParseError> {
    let outer = |expression: Box<Expr>| resolve_names(*expression, bound, resolve).map(Box::new);
    let inner = |expression: Option<Box<Expr>>, scope: &BTreeSet<String>| {
        expression
            .map(|expression| resolve_names(*expression, scope, resolve).map(Box::new))
            .transpose()
    };
    let mut scope = bound.clone();
    let binder = match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => {
            scope.insert(name.clone());
            Binder::Typed {
                name,
                type_name,
                where_expr: inner(where_expr, &scope)?,
            }
        }
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => {
            let (lo, hi) = (outer(lo)?, outer(hi)?);
            scope.insert(name.clone());
            Binder::Range {
                name,
                lo,
                hi,
                where_expr: inner(where_expr, &scope)?,
            }
        }
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            let collection = outer(collection)?;
            scope.insert(name.clone());
            Binder::Collection {
                name,
                collection,
                where_expr: inner(where_expr, &scope)?,
            }
        }
    };
    Ok((binder, scope))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::resolve_names;
    use crate::{
        AggregateKind, Binder, ConditionalSpans, Expr, Pattern, QualifiedName, SourcePos, Span,
    };

    fn span() -> Span {
        let at = SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        };
        Span { start: at, end: at }
    }

    #[allow(clippy::unnecessary_box_returns)] // every use is a boxed operand
    fn var(name: &str) -> Box<Expr> {
        Box::new(Expr::Var(name.to_owned()))
    }

    /// `amount` (free) becomes `Claim.amount`; everything else stays.
    fn spell(expression: Expr) -> Expr {
        resolve_names(expression, &BTreeSet::new(), &|reference, _| {
            Ok(match reference {
                Expr::Var(name) if name == "amount" => Some(Expr::Var("Claim.amount".into())),
                _ => None,
            })
        })
        .expect("infallible")
    }

    /// One expression of every form, each holding a free `amount`, with that
    /// `amount` spelled `Claim.amount` when `spelled`. Adding an [`Expr`]
    /// variant fails to compile in `resolve_names` until it is resolved there.
    #[allow(clippy::too_many_lines)]
    fn every_form(spelled: bool) -> Vec<Expr> {
        let amount = || {
            if spelled {
                var("Claim.amount")
            } else {
                var("amount")
            }
        };
        let range = |name: &str, filter: Option<Box<Expr>>| Binder::Range {
            name: name.to_owned(),
            lo: amount(),
            hi: amount(),
            where_expr: filter,
        };
        vec![
            Expr::Some(amount()),
            Expr::Set(vec![*amount()]),
            Expr::Seq(vec![*amount()]),
            Expr::Struct {
                name: "Pair".into(),
                fields: vec![("first".into(), *amount())],
            },
            Expr::Call {
                name: "helper".into(),
                args: vec![*amount()],
                span: span(),
            },
            Expr::Index(amount(), amount()),
            Expr::Field(amount(), "part".into()),
            Expr::Method {
                receiver: amount(),
                name: "contains".into(),
                args: vec![*amount()],
            },
            Expr::Binary {
                op: "+".into(),
                left: amount(),
                right: amount(),
            },
            Expr::Neg(amount()),
            Expr::Not(amount()),
            Expr::Conditional {
                condition: amount(),
                then_expr: amount(),
                else_expr: amount(),
                spans: Box::new(ConditionalSpans {
                    condition: span(),
                    then_expr: span(),
                    else_expr: span(),
                }),
            },
            Expr::Is {
                expr: amount(),
                pattern: Pattern::Some("v".into()),
            },
            Expr::Quantified {
                quantifier: "exists".into(),
                binder: range("x", Some(amount())),
                body: amount(),
            },
            Expr::Quantified {
                quantifier: "all".into(),
                binder: Binder::Typed {
                    name: "x".into(),
                    type_name: QualifiedName {
                        namespace: None,
                        name: "Amount".into(),
                    },
                    where_expr: Some(amount()),
                },
                body: amount(),
            },
            Expr::Aggregate {
                kind: AggregateKind::Sum,
                binder: Binder::Collection {
                    name: "x".into(),
                    collection: amount(),
                    where_expr: Some(amount()),
                },
                value: Some(amount()),
            },
            Expr::Stage {
                process: None,
                entity: amount(),
                entity_span: span(),
                span: span(),
            },
            Expr::UnaryNamed {
                name: "abs".into(),
                expr: amount(),
                span: span(),
            },
            Expr::BinaryNamed {
                name: "max".into(),
                left: amount(),
                right: amount(),
            },
            Expr::TernaryNamed {
                name: "rel_reachable".into(),
                first: amount(),
                second: amount(),
                third: amount(),
            },
        ]
    }

    #[test]
    fn a_free_name_is_resolved_inside_every_form() {
        for (before, after) in every_form(false).into_iter().zip(every_form(true)) {
            assert_eq!(spell(before.clone()), after, "{before:?}");
        }
    }

    #[test]
    fn a_binder_shadows_in_its_filter_and_body_but_not_in_its_domain() {
        let quantified = |lo: &str, filter: &str, body: &str| Expr::Quantified {
            quantifier: "exists".into(),
            binder: Binder::Range {
                name: "amount".into(),
                lo: var(lo),
                hi: var(lo),
                where_expr: Some(var(filter)),
            },
            body: var(body),
        };
        assert_eq!(
            spell(quantified("amount", "amount", "amount")),
            quantified("Claim.amount", "amount", "amount"),
        );
        let aggregate = |collection: &str, value: &str| Expr::Aggregate {
            kind: AggregateKind::Count,
            binder: Binder::Collection {
                name: "amount".into(),
                collection: var(collection),
                where_expr: None,
            },
            value: Some(var(value)),
        };
        assert_eq!(
            spell(aggregate("amount", "amount")),
            aggregate("Claim.amount", "amount"),
        );
        // The scope ends with the binder: a sibling operand is resolved.
        assert_eq!(
            spell(Expr::Binary {
                op: "and".into(),
                left: Box::new(quantified("amount", "amount", "amount")),
                right: var("amount"),
            }),
            Expr::Binary {
                op: "and".into(),
                left: Box::new(quantified("Claim.amount", "amount", "amount")),
                right: var("Claim.amount"),
            },
        );
    }

    #[test]
    fn a_bound_receiver_is_not_handed_to_resolve() {
        let seen = std::cell::RefCell::new(Vec::new());
        let expression = Expr::Quantified {
            quantifier: "all".into(),
            binder: Binder::Typed {
                name: "Claim".into(),
                type_name: QualifiedName {
                    namespace: None,
                    name: "Claim".into(),
                },
                where_expr: None,
            },
            body: Box::new(Expr::Field(var("Claim"), "amount".into())),
        };
        let resolved = resolve_names(expression.clone(), &BTreeSet::new(), &|reference, _| {
            seen.borrow_mut().push(reference.clone());
            Ok(None)
        })
        .expect("infallible");
        assert_eq!(resolved, expression);
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
    }

    /// `resolve` sees the binders around each reference: the filter and the
    /// body see the binder, the domain and a sibling operand do not.
    #[test]
    fn resolve_sees_the_binders_in_scope_at_each_reference() {
        let seen = std::cell::RefCell::new(Vec::new());
        let expression = Expr::Binary {
            op: "and".into(),
            left: Box::new(Expr::Quantified {
                quantifier: "exists".into(),
                binder: Binder::Range {
                    name: "x".into(),
                    lo: var("lo"),
                    hi: var("hi"),
                    where_expr: Some(var("filter")),
                },
                body: Box::new(Expr::Aggregate {
                    kind: AggregateKind::Sum,
                    binder: Binder::Collection {
                        name: "y".into(),
                        collection: var("items"),
                        where_expr: None,
                    },
                    value: Some(var("value")),
                }),
            }),
            right: var("after"),
        };
        resolve_names(expression, &BTreeSet::new(), &|reference, bound| {
            if let Expr::Var(name) = reference {
                seen.borrow_mut()
                    .push((name.clone(), bound.iter().cloned().collect::<Vec<_>>()));
            }
            Ok(None)
        })
        .expect("infallible");
        let scope = |names: &[&str]| names.iter().map(|&name| name.to_owned()).collect();
        assert_eq!(
            seen.into_inner(),
            vec![
                ("lo".to_owned(), scope(&[])),
                ("hi".to_owned(), scope(&[])),
                ("filter".to_owned(), scope(&["x"])),
                ("items".to_owned(), scope(&["x"])),
                ("value".to_owned(), scope(&["x", "y"])),
                ("after".to_owned(), scope(&[])),
            ]
        );
    }

    #[test]
    fn an_error_from_resolve_propagates_from_a_nested_form() {
        let error = resolve_names(
            Expr::UnaryNamed {
                name: "abs".into(),
                expr: var("amount"),
                span: span(),
            },
            &BTreeSet::new(),
            &|_, _| Err(crate::ParseError::new("refused", span())),
        )
        .expect_err("refused");
        assert!(error.to_string().contains("refused"), "{error}");
    }
}
