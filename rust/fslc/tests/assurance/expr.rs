// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `expr` axis: all 24 live `fsl_syntax::Expr` variants plus all four
//! `AggregateKind` values (issue #537 C3 slice 2).
//!
//! Declared columns are `Monitor` and `BMC`, the concrete and symbolic
//! expression evaluators. The explicit engine is not a third expression
//! implementation: it drives the same concrete Monitor evaluator, and the
//! C6 sweep cited below still includes it as a three-engine verdict and
//! successor-agreement control.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{FsResolver, build_surface_model, parse_kernel_source};
use fsl_syntax::{Expr, SourcePos, Span, SpecItem};

use crate::claim::{Axis, Citation, Claim};
use crate::enum_rows::expression_rows;

const SWEEP: Citation = Citation {
    path: "rust/fslc/tests/typed_agreement.rs",
    anchor: "fn expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types()",
};

const FAIL_CLOSED_CONTROL: Citation = Citation {
    path: "rust/fslc/tests/assurance/expr.rs",
    anchor: "fn unlowered_call_and_stage_fail_closed_before_evaluator_entry()",
};

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

#[must_use]
pub fn axis() -> Axis {
    let rows = expression_rows();
    let columns = vec!["Monitor", "BMC"];
    let mut cells: BTreeMap<(&'static str, &'static str), Claim> = BTreeMap::new();
    for &row in &rows {
        let unsupported = matches!(row, "Expr::Call" | "Expr::Stage");
        for &column in &columns {
            cells.insert(
                (row, column),
                if unsupported {
                    Claim::UnsupportedFailClosed {
                        by: FAIL_CLOSED_CONTROL,
                    }
                } else {
                    Claim::Exercised { by: SWEEP }
                },
            );
        }
    }
    Axis {
        name: "expr",
        rows,
        columns,
        cells,
    }
}

#[test]
fn expression_rows_are_complete_and_unique() {
    let rows = expression_rows();
    assert_eq!(rows.len(), 28, "24 Expr rows plus 4 AggregateKind rows");
    assert_eq!(
        rows.iter().copied().collect::<BTreeSet<_>>().len(),
        rows.len(),
        "expression-axis rows must be unique"
    );
}

fn replace_variant_expression(expression: Expr) -> Result<(), String> {
    let source = "spec SurfaceLeak { state { ready: Bool } init { ready = true } \
                  action stay() { ready = ready } invariant Variant { true } }";
    let kernel = parse_kernel_source(source, &FsResolver::new("."))
        .map_err(|error| format!("parse control model: {error}"))?;
    let mut syntax = kernel.into_syntax();
    let target = syntax
        .items
        .iter_mut()
        .find_map(|item| match item {
            SpecItem::Invariant { name, expr, .. } if name == "Variant" => Some(expr),
            _ => None,
        })
        .ok_or_else(|| "control model has no Variant invariant".to_owned())?;
    **target = expression;
    build_surface_model(syntax)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Observation control for the two surface-only variants. Direct-spec
/// predicate expansion and business/requirements stage resolution normally
/// eliminate these forms. Injecting either into the typed surface tree and
/// re-running the semantic build gate must reject it before Monitor or BMC
/// can evaluate a misleading default.
#[test]
fn unlowered_call_and_stage_fail_closed_before_evaluator_entry() {
    let location = span();
    let call_error = replace_variant_expression(Expr::Call {
        name: "leaked_predicate".to_owned(),
        args: Vec::new(),
        span: location,
    })
    .expect_err("unlowered Call must fail closed");
    assert!(
        call_error.contains("unlowered predicate call 'leaked_predicate' in public Kernel"),
        "{call_error}"
    );

    let stage_error = replace_variant_expression(Expr::Stage {
        process: None,
        entity: Box::new(Expr::Var("ready".to_owned())),
        entity_span: location,
        span: location,
    })
    .expect_err("unlowered Stage must fail closed");
    assert!(
        stage_error.contains("unlowered stage access in public Kernel"),
        "{stage_error}"
    );
}
