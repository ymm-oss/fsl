// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Intentional-undecided metadata extraction (issue #189).

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    ActionDef, Annotations, KernelBinder, KernelExpr, KernelLValue, KernelModel, KernelStatement,
};
use fsl_syntax::Span;
use serde_json::{Value, json};

#[allow(clippy::too_many_lines)]
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
            | KernelExpr::Stage { entity: value, .. }
            | KernelExpr::UnaryNamed { expr: value, .. } => collect(value, roots),
            KernelExpr::Index(left, right)
            | KernelExpr::Binary { left, right, .. }
            | KernelExpr::BinaryNamed { left, right, .. } => {
                collect(left, roots);
                collect(right, roots);
            }
            KernelExpr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
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
            KernelExpr::Aggregate { binder, value, .. } => {
                collect_binder(binder, roots);
                if let Some(value) = value {
                    collect(value, roots);
                }
            }
            KernelExpr::Num(_)
            | KernelExpr::Bool(_)
            | KernelExpr::None
            | KernelExpr::EnumMember { .. } => {}
        }
    }

    fn collect_binder(binder: &KernelBinder, roots: &mut BTreeSet<String>) {
        match binder {
            KernelBinder::Typed { where_expr, .. } => {
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
            KernelBinder::Range {
                lo, hi, where_expr, ..
            } => {
                collect(lo, roots);
                collect(hi, roots);
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
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
                let binder_expr = KernelExpr::Quantified {
                    quantifier: "forall".to_owned(),
                    binder: binder.clone(),
                    body: Box::new(KernelExpr::Bool(true)),
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
    let mut add = |annotations: &Annotations, roots: BTreeSet<String>| {
        for requirement in annotations
            .requirements()
            .expect("checked model annotations are valid")
        {
            output
                .entry(requirement.id)
                .or_default()
                .extend(roots.clone());
        }
    };
    for action in &model.actions {
        add(&action.annotations, action_roots(model, action));
    }
    for property in model
        .invariants
        .iter()
        .chain(&model.transitions)
        .chain(&model.reachables)
    {
        add(
            &property.annotations,
            expression_roots(model, &property.expr),
        );
    }
    for property in &model.leadstos {
        let mut roots = expression_roots(model, &property.before);
        roots.extend(expression_roots(model, &property.after));
        if let Some(decreases) = &property.decreases {
            roots.extend(expression_roots(model, decreases));
        }
        add(&property.annotations, roots);
    }
    output
}

/// One `undecided:`-tagged declaration, typed and carrying the annotation's
/// source span for consumers that need per-item provenance (issue #325's
/// Requirement Claim IR) that the JSON projection below does not expose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndecidedRecord {
    pub declaration: String,
    pub node: String,
    pub reason: String,
    pub requirement_ids: Vec<String>,
    pub span: Span,
}

fn record(
    declaration: &str,
    node: &str,
    reason: &str,
    span: Span,
    roots: &BTreeSet<String>,
    requirements: &BTreeMap<String, BTreeSet<String>>,
) -> UndecidedRecord {
    let requirement_ids = requirements
        .iter()
        .filter(|(_, requirement_roots)| !roots.is_disjoint(requirement_roots))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    UndecidedRecord {
        declaration: declaration.to_owned(),
        node: node.to_owned(),
        reason: reason.to_owned(),
        requirement_ids,
        span,
    }
}

/// Return every declaration tagged with the reserved `undecided:` marker.
#[must_use]
pub fn undecided_records(model: &KernelModel) -> Vec<UndecidedRecord> {
    let requirements = requirement_roots(model);
    let mut output = Vec::new();
    for (reason, span) in model.init_annotations.undecided() {
        let roots = statement_roots(model, &model.init);
        output.push(record("init", "init", reason, span, &roots, &requirements));
    }
    for action in &model.actions {
        for (reason, span) in action.annotations.undecided() {
            output.push(record(
                &format!("action {}", action.name),
                &format!("action:{}", action.name),
                reason,
                span,
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
            for (reason, span) in property.annotations.undecided() {
                output.push(record(
                    &format!("{kind} {}", property.name),
                    &format!("{kind}:{}", property.name),
                    reason,
                    span,
                    &expression_roots(model, &property.expr),
                    &requirements,
                ));
            }
        }
    }
    for property in &model.leadstos {
        for (reason, span) in property.annotations.undecided() {
            let mut roots = expression_roots(model, &property.before);
            roots.extend(expression_roots(model, &property.after));
            output.push(record(
                &format!("leadsTo {}", property.name),
                &format!("leadsTo:{}", property.name),
                reason,
                span,
                &roots,
                &requirements,
            ));
        }
    }
    output
}

/// Return every declaration tagged with the reserved `undecided:` marker.
#[must_use]
pub fn undecided_declarations(model: &KernelModel) -> Vec<Value> {
    undecided_records(model)
        .into_iter()
        .map(|record| {
            json!({
                "declaration": record.declaration,
                "node": record.node,
                "reason": record.reason,
                "requirement_ids": record.requirement_ids,
            })
        })
        .collect()
}
