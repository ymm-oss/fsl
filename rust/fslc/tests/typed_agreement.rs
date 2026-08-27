// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! C6 typed generative / metamorphic cross-engine agreement suite (#537 C6
//! slice 1, issue #648).
//!
//! Generates checked `KernelModel`s (never string fuzz) from a deterministic
//! structural axis enumeration (`generator.rs`), compares Monitor BFS /
//! explicit / BMC bounded verdicts and successors (`engines.rs`), and checks
//! seven metamorphic relations with a negative control each (`relations.rs`).
//! `sweep_summary.rs` aggregates what each sweep actually exercised. Slice 2
//! adds an expression-variant family that is also the exercising evidence for
//! the C3 `expr` and `types` axes.
//!
//! See `docs/DESIGN-conformance-harness.md`'s "Typed generative /
//! metamorphic agreement (#537 C6)" section for the accepted design this
//! implements, including why Z3js/Worker parity is out of scope here.

#[path = "typed_agreement/engines.rs"]
mod engines;
#[path = "assurance/enum_rows.rs"]
mod enum_rows;
#[path = "typed_agreement/generator.rs"]
mod generator;
#[path = "typed_agreement/inventory.rs"]
mod inventory;
#[path = "typed_agreement/logic_test.rs"]
mod logic_test;
#[path = "typed_agreement/regression_corpus.rs"]
mod regression_corpus;
#[path = "typed_agreement/relations.rs"]
mod relations;
#[path = "typed_agreement/shrink.rs"]
mod shrink;
#[path = "typed_agreement/sweep_summary.rs"]
mod sweep_summary;

use std::collections::BTreeSet;

use enum_rows::{
    aggregate_kind_row, aggregate_kind_rows, checked_expr_variant_rows, expr_variant_row,
    type_def_row, type_ref_row, type_rows,
};
use fsl_core::{KernelModel, TypeDef, TypeRef};
use fsl_syntax::{Binder, Expr};
use generator::{PropertyKind, domain_sweep, expression_sweep, operation_sweep};
use sweep_summary::SweepSummary;

include!("typed_agreement/nested_options.rs");

/// Generator floor, asserted per the brief and design's "assert model
/// count is at least N" requirement: `domain_axis` has 15 `(kind, size)`
/// pairs (S2's four scalar domain kinds), so anything below that means the
/// axis enumeration itself regressed.
const DOMAIN_SWEEP_FLOOR: usize = 15;
/// `divide`/`remainder` guarded-action-context plus property-context
/// entries. `head`/`pop`/`at`/index and the unguarded divide/remainder
/// action-context boundary are exercised as dedicated `relations.rs` R6
/// tests instead of this sweep; see `generator.rs::operation_sweep`'s doc.
const OPERATION_SWEEP_FLOOR: usize = 4;
/// 21 non-aggregate executable variants plus four separate aggregate-kind
/// models. `Call` and `Stage` are fail-closed before evaluator entry.
const EXPRESSION_SWEEP_FLOOR: usize = 25;

#[test]
fn domain_sweep_meets_its_generator_floor_and_covers_every_property_kind() {
    let models = domain_sweep();
    assert!(
        models.len() >= DOMAIN_SWEEP_FLOOR,
        "domain sweep floor: expected >= {DOMAIN_SWEEP_FLOOR}, got {}",
        models.len()
    );
    for kind in [
        PropertyKind::Invariant,
        PropertyKind::Reachable,
        PropertyKind::LeadsTo,
        PropertyKind::Trans,
        PropertyKind::Terminal,
    ] {
        assert!(
            models.iter().any(|model| model.property_kind == kind),
            "domain sweep must exercise property kind '{}' at least once",
            kind.label()
        );
    }
}

#[test]
fn operation_sweep_meets_its_generator_floor() {
    let models = operation_sweep();
    assert!(
        models.len() >= OPERATION_SWEEP_FLOOR,
        "operation sweep floor: expected >= {OPERATION_SWEEP_FLOOR}, got {}",
        models.len()
    );
}

fn visit_binder(
    binder: &Binder,
    expr_rows: &mut BTreeSet<&'static str>,
    aggregate_rows: &mut BTreeSet<&'static str>,
) {
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            visit_expr(lo, expr_rows, aggregate_rows);
            visit_expr(hi, expr_rows, aggregate_rows);
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            visit_expr(collection, expr_rows, aggregate_rows);
            if let Some(expression) = where_expr {
                visit_expr(expression, expr_rows, aggregate_rows);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn visit_expr(
    expr: &Expr,
    expr_rows: &mut BTreeSet<&'static str>,
    aggregate_rows: &mut BTreeSet<&'static str>,
) {
    expr_rows.insert(expr_variant_row(expr));
    match expr {
        Expr::Some(value)
        | Expr::Neg(value)
        | Expr::Not(value)
        | Expr::Field(value, _)
        | Expr::Stage { entity: value, .. }
        | Expr::UnaryNamed { expr: value, .. }
        | Expr::Is { expr: value, .. } => visit_expr(value, expr_rows, aggregate_rows),
        Expr::Set(values) | Expr::Seq(values) => {
            for value in values {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::Call { args, .. } => {
            for argument in args {
                visit_expr(argument, expr_rows, aggregate_rows);
            }
        }
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            visit_expr(left, expr_rows, aggregate_rows);
            visit_expr(right, expr_rows, aggregate_rows);
        }
        Expr::Method { receiver, args, .. } => {
            visit_expr(receiver, expr_rows, aggregate_rows);
            for argument in args {
                visit_expr(argument, expr_rows, aggregate_rows);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            visit_expr(condition, expr_rows, aggregate_rows);
            visit_expr(then_expr, expr_rows, aggregate_rows);
            visit_expr(else_expr, expr_rows, aggregate_rows);
        }
        Expr::Quantified { binder, body, .. } => {
            visit_binder(binder, expr_rows, aggregate_rows);
            visit_expr(body, expr_rows, aggregate_rows);
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            aggregate_rows.insert(aggregate_kind_row(kind));
            visit_binder(binder, expr_rows, aggregate_rows);
            if let Some(value) = value {
                visit_expr(value, expr_rows, aggregate_rows);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            visit_expr(first, expr_rows, aggregate_rows);
            visit_expr(second, expr_rows, aggregate_rows);
            visit_expr(third, expr_rows, aggregate_rows);
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {}
    }
}

fn visit_type_ref(ty: &TypeRef, rows: &mut BTreeSet<&'static str>) {
    rows.insert(type_ref_row(ty));
    match ty {
        TypeRef::Map(key, value) | TypeRef::Relation(key, value) => {
            visit_type_ref(key, rows);
            visit_type_ref(value, rows);
        }
        TypeRef::Set(item) | TypeRef::Seq(item, _) | TypeRef::Option(item) => {
            visit_type_ref(item, rows);
        }
        TypeRef::Int | TypeRef::Bool | TypeRef::Named(_) | TypeRef::Range(_, _) => {}
    }
}

fn model_type_rows(model: &KernelModel) -> BTreeSet<&'static str> {
    let mut rows = BTreeSet::new();
    for (_, ty) in &model.state {
        visit_type_ref(ty, &mut rows);
    }
    for definition in model.types.values() {
        rows.insert(type_def_row(definition));
        if let TypeDef::Struct { fields } = definition {
            for (_, ty) in fields {
                visit_type_ref(ty, &mut rows);
            }
        }
    }
    rows
}

/// Slice-2 expression/type family: every evaluator-reachable `Expr` variant
/// is present in a checked model and passes Monitor BFS / explicit / BMC
/// verdict, replay, and successor agreement. Four aggregate models make the
/// `AggregateKind` inventory explicit. The same model schema contains all 9
/// `TypeRef` and all 3 `TypeDef` variants, so this is also the C3 `types`
/// axis's concrete/symbolic value-generation evidence.
#[test]
fn expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types() {
    let models = expression_sweep();
    assert!(
        models.len() >= EXPRESSION_SWEEP_FLOOR,
        "expression sweep floor: expected >= {EXPRESSION_SWEEP_FLOOR}, got {}",
        models.len()
    );

    let mut designated_expr_rows = BTreeSet::new();
    let mut designated_aggregate_rows = BTreeSet::new();
    let mut observed_type_rows = BTreeSet::new();
    let mut summary = SweepSummary::default();

    for generated in models {
        let model = engines::build_expression(&generated.id, &generated.source, generated.build);
        let property = model
            .invariants
            .iter()
            .find(|property| property.name == "Variant")
            .unwrap_or_else(|| panic!("'{}': Variant invariant disappeared", generated.id));
        let mut expr_rows = BTreeSet::new();
        let mut aggregate_rows = BTreeSet::new();
        visit_expr(&property.expr, &mut expr_rows, &mut aggregate_rows);
        assert!(
            expr_rows.contains(generated.expr_variant),
            "'{}': generated model does not contain designated row {}; observed={expr_rows:?}",
            generated.id,
            generated.expr_variant
        );
        if let Some(kind) = generated.aggregate_kind {
            assert!(
                aggregate_rows.contains(kind),
                "'{}': generated model does not contain designated row {kind}; observed={aggregate_rows:?}",
                generated.id
            );
            designated_aggregate_rows.insert(kind);
        }

        let verdict = engines::run_agreement(&generated.id, &model, generated.depth);
        assert_eq!(
            verdict,
            engines::Verdict::Clean,
            "'{}': the positive expression model must satisfy Variant",
            generated.id
        );
        let mut negative = model.clone();
        let property = negative
            .invariants
            .iter_mut()
            .find(|property| property.name == "Variant")
            .unwrap_or_else(|| panic!("'{}': Variant invariant disappeared", generated.id));
        let positive = std::mem::replace(&mut property.expr, Expr::Bool(false));
        property.expr = Expr::Not(Box::new(positive));
        let negative_id = format!("{}_negative_control", generated.id);
        let negative_verdict = engines::run_agreement(&negative_id, &negative, generated.depth);
        assert_eq!(
            negative_verdict,
            engines::Verdict::Violated {
                kind: "invariant".to_owned(),
                name: "Variant".to_owned(),
                step: 0,
            },
            "'{}': negating the known-true expression must be detected as an initial invariant violation by all three engines",
            generated.id
        );
        designated_expr_rows.insert(generated.expr_variant);
        let type_rows = model_type_rows(&model);
        observed_type_rows.extend(type_rows.iter().copied());
        summary.record_expression_model(
            generated.expr_variant,
            generated.aggregate_kind,
            type_rows,
        );
    }

    let expected_expr_rows = checked_expr_variant_rows().into_iter().collect();
    assert_eq!(
        designated_expr_rows, expected_expr_rows,
        "the expression family must designate exactly the source-coupled evaluator-reachable Expr rows"
    );
    let expected_aggregate_rows = aggregate_kind_rows().into_iter().collect();
    assert_eq!(
        designated_aggregate_rows, expected_aggregate_rows,
        "every source-coupled AggregateKind row must have a generated model"
    );
    let expected_type_rows = type_rows().into_iter().collect();
    assert_eq!(
        observed_type_rows, expected_type_rows,
        "the expression family must carry exactly the source-coupled TypeRef/TypeDef rows through concrete and symbolic evaluation"
    );
    eprintln!("expression/type sweep summary: {summary}");
}

/// The main sweep: every domain-axis model must build and its Monitor
/// BFS / explicit / BMC verdicts must agree (`engines::run_agreement`
/// panics on disagreement, so a clean run here already *is* the "zero
/// cross-engine disagreements" evidence the brief asks to report).
#[test]
fn domain_sweep_agrees_across_all_three_engines() {
    let mut summary = SweepSummary::default();
    for model in domain_sweep() {
        let built = engines::build(&model.id, &model.source);
        engines::run_agreement(&model.id, &built, model.depth);
        summary.record_domain_model(
            model.domain_kind.label(),
            model.domain_size,
            model.property_kind.label(),
            model.state_vars,
            model.action_count,
            model.guarded,
            model.fair,
        );
    }
    eprintln!("domain sweep summary: {summary}");
}

#[test]
fn operation_sweep_agrees_across_all_three_engines() {
    let mut summary = SweepSummary::default();
    for model in operation_sweep() {
        let built = engines::build(&model.id, &model.source);
        engines::run_agreement(&model.id, &built, model.depth);
        summary.record_operation_model(model.operation, model.context);
    }
    eprintln!("operation sweep summary: {summary}");
}
