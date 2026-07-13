// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Intentional-undecided metadata extraction (issue #189).

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{ActionDef, KernelBinder, KernelExpr, KernelLValue, KernelModel, KernelStatement};
use fsl_syntax::MetaTag;
use serde_json::{Value, json};

fn is_undecided(meta: Option<&MetaTag>) -> bool {
    meta.is_some_and(|meta| meta.id.eq_ignore_ascii_case("undecided"))
}

fn reason(meta: &MetaTag) -> String {
    meta.text.clone().unwrap_or_default()
}

fn expression_roots(model: &KernelModel, expr: &KernelExpr) -> BTreeSet<String> {
    fn collect(expr: &KernelExpr, roots: &mut BTreeSet<String>) {
        match expr {
            KernelExpr::Var(name) => {
                roots.insert(name.clone());
            }
            KernelExpr::Some(value)
            | KernelExpr::Neg(value)
            | KernelExpr::Not(value)
            | KernelExpr::Field(value, _)
            | KernelExpr::UnaryNamed { expr: value, .. } => collect(value, roots),
            KernelExpr::Index(left, right)
            | KernelExpr::Binary { left, right, .. }
            | KernelExpr::BinaryNamed { left, right, .. } => {
                collect(left, roots);
                collect(right, roots);
            }
            KernelExpr::IfThenElse {
                condition,
                then_expr,
                else_expr,
            }
            | KernelExpr::TernaryNamed {
                first: condition,
                second: then_expr,
                third: else_expr,
                ..
            } => {
                collect(condition, roots);
                collect(then_expr, roots);
                collect(else_expr, roots);
            }
            KernelExpr::Set(items) | KernelExpr::Seq(items) => {
                for item in items {
                    collect(item, roots);
                }
            }
            KernelExpr::Struct { fields, .. } => {
                for (_, value) in fields {
                    collect(value, roots);
                }
            }
            KernelExpr::Call { args, .. } | KernelExpr::Method { args, .. } => {
                if let KernelExpr::Method { receiver, .. } = expr {
                    collect(receiver, roots);
                }
                for arg in args {
                    collect(arg, roots);
                }
            }
            KernelExpr::Is { expr, .. } => collect(expr, roots),
            KernelExpr::Quantified { binder, body, .. } => {
                collect_binder(binder, roots);
                collect(body, roots);
            }
            KernelExpr::Count { condition, .. } => collect(condition, roots),
            KernelExpr::Sum {
                body, condition, ..
            } => {
                collect(body, roots);
                if let Some(condition) = condition {
                    collect(condition, roots);
                }
            }
            KernelExpr::BinderNamed { binder, .. } => collect_binder(binder, roots),
            KernelExpr::Num(_) | KernelExpr::Bool(_) | KernelExpr::None => {}
        }
    }

    fn collect_binder(binder: &KernelBinder, roots: &mut BTreeSet<String>) {
        match binder {
            KernelBinder::Typed { where_expr, .. } => {
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
            KernelBinder::Range { lo, hi, .. } => {
                collect(lo, roots);
                collect(hi, roots);
            }
            KernelBinder::Collection {
                collection,
                where_expr,
                ..
            } => {
                collect(collection, roots);
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
        }
    }

    let state = model
        .state
        .iter()
        .map(|(name, _)| name)
        .collect::<BTreeSet<_>>();
    let mut roots = BTreeSet::new();
    collect(expr, &mut roots);
    roots.retain(|name| state.contains(name));
    roots
}

fn lvalue_roots(model: &KernelModel, target: &KernelLValue) -> BTreeSet<String> {
    match target {
        KernelLValue::Var(name) => BTreeSet::from([name.clone()]),
        KernelLValue::Index(name, index) => {
            let mut roots = BTreeSet::from([name.clone()]);
            roots.extend(expression_roots(model, index));
            roots
        }
        KernelLValue::Field(base, _) => lvalue_roots(model, base),
    }
}

fn statement_roots(model: &KernelModel, statements: &[KernelStatement]) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    for statement in statements {
        match statement {
            KernelStatement::Assign { target, value, .. } => {
                roots.extend(lvalue_roots(model, target));
                roots.extend(expression_roots(model, value));
            }
            KernelStatement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                roots.extend(expression_roots(model, condition));
                roots.extend(statement_roots(model, then_statements));
                roots.extend(statement_roots(model, else_statements));
            }
            KernelStatement::ForAll {
                binder, statements, ..
            } => {
                let binder_expr = KernelExpr::BinderNamed {
                    name: "forall".to_owned(),
                    binder: binder.clone(),
                };
                roots.extend(expression_roots(model, &binder_expr));
                roots.extend(statement_roots(model, statements));
            }
        }
    }
    roots
}

fn action_roots(model: &KernelModel, action: &ActionDef) -> BTreeSet<String> {
    let mut roots = statement_roots(model, &action.statements);
    for expr in action
        .requires
        .iter()
        .chain(action.lets.iter().map(|(_, expr)| expr))
        .chain(action.ensures.iter())
    {
        roots.extend(expression_roots(model, expr));
    }
    roots
}

fn requirement_roots(model: &KernelModel) -> BTreeMap<String, BTreeSet<String>> {
    let mut output = BTreeMap::<String, BTreeSet<String>>::new();
    let mut add = |meta: Option<&MetaTag>, roots: BTreeSet<String>| {
        if let Some(meta) = meta.filter(|meta| !is_undecided(Some(meta))) {
            output.entry(meta.id.clone()).or_default().extend(roots);
        }
    };
    for action in &model.actions {
        add(action.meta.as_ref(), action_roots(model, action));
    }
    for property in model
        .invariants
        .iter()
        .chain(&model.transitions)
        .chain(&model.reachables)
    {
        add(
            property.meta.as_ref(),
            expression_roots(model, &property.expr),
        );
    }
    for property in &model.leadstos {
        let mut roots = expression_roots(model, &property.before);
        roots.extend(expression_roots(model, &property.after));
        if let Some(decreases) = &property.decreases {
            roots.extend(expression_roots(model, decreases));
        }
        add(property.meta.as_ref(), roots);
    }
    output
}

fn record(
    declaration: &str,
    node: &str,
    meta: &MetaTag,
    roots: &BTreeSet<String>,
    requirements: &BTreeMap<String, BTreeSet<String>>,
) -> Value {
    let requirement_ids = requirements
        .iter()
        .filter(|(_, requirement_roots)| !roots.is_disjoint(requirement_roots))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    json!({
        "declaration": declaration,
        "node": node,
        "reason": reason(meta),
        "requirement_ids": requirement_ids,
    })
}

/// Return every declaration tagged with the reserved `undecided:` marker.
#[must_use]
pub fn undecided_declarations(model: &KernelModel) -> Vec<Value> {
    let requirements = requirement_roots(model);
    let mut output = Vec::new();
    if let Some(meta) = model
        .init_meta
        .as_ref()
        .filter(|meta| is_undecided(Some(meta)))
    {
        let roots = statement_roots(model, &model.init);
        output.push(record("init", "init", meta, &roots, &requirements));
    }
    for action in &model.actions {
        if let Some(meta) = action.meta.as_ref().filter(|meta| is_undecided(Some(meta))) {
            output.push(record(
                &format!("action {}", action.name),
                &format!("action:{}", action.name),
                meta,
                &action_roots(model, action),
                &requirements,
            ));
        }
    }
    for (kind, properties) in [
        ("invariant", &model.invariants),
        ("trans", &model.transitions),
        ("reachable", &model.reachables),
    ] {
        for property in properties {
            if let Some(meta) = property
                .meta
                .as_ref()
                .filter(|meta| is_undecided(Some(meta)))
            {
                output.push(record(
                    &format!("{kind} {}", property.name),
                    &format!("{kind}:{}", property.name),
                    meta,
                    &expression_roots(model, &property.expr),
                    &requirements,
                ));
            }
        }
    }
    for property in &model.leadstos {
        if let Some(meta) = property
            .meta
            .as_ref()
            .filter(|meta| is_undecided(Some(meta)))
        {
            let mut roots = expression_roots(model, &property.before);
            roots.extend(expression_roots(model, &property.after));
            output.push(record(
                &format!("leadsTo {}", property.name),
                &format!("leadsTo:{}", property.name),
                meta,
                &roots,
                &requirements,
            ));
        }
    }
    output
}
