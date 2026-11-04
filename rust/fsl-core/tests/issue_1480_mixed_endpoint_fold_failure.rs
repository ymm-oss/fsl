// SPDX-License-Identifier: Apache-2.0
//! Test-first F1 controls: authored admission is not symbolic execution permission.

use std::collections::BTreeMap;

use fsl_core::binder_range::{RangePlan, RangeUnknown};
use fsl_core::{
    ActionGuard, FsResolver, KernelExpr as Expr, KernelModel, KernelSpec, ModelError, OriginChain,
    OriginId, OriginSite, Span, TypeRef, build_model, parse_kernel_source,
};
use fsl_syntax::SpecItem;

const CLOSED_BOUND: &str = "public Kernel requires static aggregate range bounds";

fn row(failures: &mut Vec<String>, id: &str, check: impl FnOnce() + std::panic::UnwindSafe) {
    // Keep the original assertion/panic diagnostics, but do not lose later rows.
    let failed = std::panic::catch_unwind(check).is_err();
    eprintln!(
        "MIXED_ROW id={id} outcome={}",
        if failed { "FAIL" } else { "PASS" }
    );
    if failed {
        failures.push(id.to_owned());
    }
}

fn source(domain: &str, predicate: &str) -> String {
    format!(
        "spec Mixed {{\n  type K = 0..2\n  state {{ n: {domain} }}\n  init {{ n = 1 }}\n  action idle() {{}}\n  invariant Check {{ {predicate} }}\n}}\n"
    )
}

fn invariant(kernel: &KernelSpec) -> (Expr, Span) {
    kernel
        .syntax()
        .items
        .iter()
        .find_map(|item| match item {
            SpecItem::Invariant { expr, span, .. } => Some((expr.as_ref().clone(), *span)),
            _ => None,
        })
        .expect("the source contains its authored invariant")
}

fn admitted(domain: &str, predicate: &str) -> KernelModel {
    let text = source(domain, predicate);
    let kernel = parse_kernel_source(&text, &FsResolver::new(".")).expect("source parses");
    let (authored, span) = invariant(&kernel);
    assert_eq!((span.start.line, span.start.column), (6, 3));
    let actual = build_model(kernel);
    let model = actual.expect("a well-typed dynamic endpoint must defer, not fail static folding");
    assert_eq!(model.invariants.len(), 1);
    let property = &model.invariants[0];
    assert_eq!(property.name, "Check");
    assert_eq!(property.span, span);
    assert!(property.expr == authored, "retain the entire authored AST");
    assert_eq!(
        model.expression_type(&property.expr, &[], None),
        Ok(Some(TypeRef::Bool))
    );
    model
}

fn outer_plan(model: &KernelModel) -> RangePlan {
    let Expr::Binary { left, .. } = &model.invariants[0].expr else {
        panic!("expected the original aggregate comparison")
    };
    let Expr::Aggregate {
        binder: fsl_core::KernelBinder::Range { lo, hi, .. },
        ..
    } = left.as_ref()
    else {
        panic!("expected the original range aggregate")
    };
    model.plan_integer_range(lo, hi, &[])
}

fn rejected(domain: &str, predicate: &str, message: &str) {
    let text = source(domain, predicate);
    let kernel = parse_kernel_source(&text, &FsResolver::new(".")).expect("source parses");
    let (_, span) = invariant(&kernel);
    assert_eq!((span.start.line, span.start.column), (6, 3));
    assert_eq!(&text[span.start.offset..span.end.offset], "invariant");
    let Err(actual) = build_model(kernel) else {
        panic!("expected the exact authored ModelError, observed Ok")
    };
    let expected = ModelError {
        message: format!("invalid model expression: {message}"),
        origin: Some(Box::new(OriginChain {
            id: OriginId(format!(
                "kernel:type-diagnostic:{}:{}",
                span.start.offset, span.end.offset
            )),
            dialect: "kernel".into(),
            primary: Some(OriginSite {
                source_file: None,
                span: Some(span),
                dialect: "kernel".into(),
                declaration_path: Vec::new(),
            }),
            secondary: Vec::new(),
            lowering_steps: Vec::new(),
            generated: false,
        })),
        span: None,
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    };
    assert_eq!(actual, expected, "full typed diagnostic, not any Err");
    assert_eq!(
        actual.to_string(),
        format!("invalid model expression: {message} at 6:3")
    );
}

#[test]
fn bounded_mixed_closed_aggregate_endpoints_defer_and_retain_ast() {
    let mut failures = Vec::new();
    for (index, endpoint) in ["n + count(j in 0..0)", "count(j in 0..0) + n"]
        .into_iter()
        .enumerate()
    {
        row(&mut failures, &format!("bounded-{index}"), || {
            let model = admitted("K", &format!("count(i in 0..({endpoint})) >= 0"));
            // This planner limitation is not proof that the whole endpoint is closed.
            assert_eq!(
                outer_plan(&model),
                RangePlan::Unknown(RangeUnknown::ConstantFoldFailed)
            );
        });
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn unbounded_mixed_closed_aggregate_endpoints_defer_in_both_orders() {
    let mut failures = Vec::new();
    for (index, (endpoint, reason)) in [
        ("n + count(j in 0..0)", RangeUnknown::MissingFiniteFact),
        ("count(j in 0..0) + n", RangeUnknown::ConstantFoldFailed),
    ]
    .into_iter()
    .enumerate()
    {
        row(&mut failures, &format!("unbounded-{index}"), || {
            let model = admitted("Int", &format!("count(i in 0..({endpoint})) >= 0"));
            assert_eq!(outer_plan(&model), RangePlan::Unknown(reason));
        });
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn lexical_mixed_closed_aggregate_endpoints_defer_in_both_orders() {
    let mut failures = Vec::new();
    for (index, endpoint) in ["x + count(j in 0..0)", "count(j in 0..0) + x"]
        .into_iter()
        .enumerate()
    {
        row(&mut failures, &format!("lexical-{index}"), || {
            let predicate = format!("count(i in 0..({endpoint})) >= 0");
            let text = format!(
                "spec Mixed {{\n  type K = 0..2\n  state {{ n: Bool }}\n  init {{ n = false }}\n  action inspect(x: K) {{ requires {predicate} }}\n  invariant Check {{ true }}\n}}\n"
            );
            let kernel = parse_kernel_source(&text, &FsResolver::new(".")).expect("source parses");
            let authored = fsl_syntax::parse_expr(&predicate).expect("existing expression parser");
            let actual = build_model(kernel);
            let model = actual.expect("lexical typing is not a premise for static expansion");
            let action = &model.actions[0];
            assert_eq!(action.name, "inspect");
            assert_eq!((action.span.start.line, action.span.start.column), (5, 3));
            assert_eq!(action.guards.len(), 1);
            let ActionGuard::Requires(stored) = &action.guards[0] else {
                panic!("expected the authored requires guard")
            };
            assert!(
                stored.kernel_ast_v1() == authored.kernel_ast_v1(),
                "retain guard AST"
            );
            let bindings = BTreeMap::from([("x".into(), TypeRef::Named("K".into()))]);
            assert_eq!(
                model.expression_type(stored, &Vec::from_iter(bindings), None),
                Ok(Some(TypeRef::Bool))
            );
        });
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn mixed_closed_arithmetic_children_do_not_make_dynamic_endpoints_closed() {
    let mut failures = Vec::new();
    for domain in ["K", "Int"] {
        for (child_index, child) in ["(1/0)", "(9223372036854775807+1)"].into_iter().enumerate() {
            for (order, endpoint) in [format!("n + {child}"), format!("{child} + n")]
                .into_iter()
                .enumerate()
            {
                row(
                    &mut failures,
                    &format!("arithmetic-{domain}-{child_index}-{order}"),
                    || {
                        // Admission only: undefined arithmetic is not executed or declared safe.
                        admitted(domain, &format!("count(i in 0..({endpoint})) >= 0"));
                    },
                );
            }
        }
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn genuinely_closed_fold_failures_keep_full_diagnostics() {
    let mut failures = Vec::new();
    for (index, endpoint) in ["(1/0)", "(9223372036854775807+1)", "count(j in 0..0)"]
        .into_iter()
        .enumerate()
    {
        for (order, (lo, hi)) in [("0", endpoint), (endpoint, "0")].into_iter().enumerate() {
            row(&mut failures, &format!("closed-{index}-{order}"), || {
                rejected("K", &format!("count(i in {lo}..{hi}) >= 0"), CLOSED_BOUND);
            });
        }
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn dynamic_lower_does_not_hide_invalid_static_upper() {
    let mut failures = Vec::new();
    for (index, hi) in ["0", "2"].into_iter().enumerate() {
        row(&mut failures, &format!("upper-valid-{index}"), || {
            admitted("Int", &format!("count(i in n..{hi}) >= 0"));
        });
    }
    for (index, hi) in ["(1/0)", "(9223372036854775807+1)", "count(j in 0..0)"]
        .into_iter()
        .enumerate()
    {
        row(&mut failures, &format!("upper-invalid-{index}"), || {
            rejected("Int", &format!("count(i in n..{hi}) >= 0"), CLOSED_BOUND);
        });
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn deferred_ancestors_still_reject_nested_closed_invalid_bounds() {
    let mut failures = Vec::new();
    for (index, predicate) in [
        "sum(i in 0..n of count(j in 0..(1/0))) >= 0",
        "count(i in 0..n where count(j in 0..(1/0)) > 0) >= 0",
        "count(i in 0..(n + count(j in 0..(1/0)))) >= 0",
    ]
    .into_iter()
    .enumerate()
    {
        row(&mut failures, &format!("nested-{index}"), || {
            rejected("Int", predicate, CLOSED_BOUND);
        });
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}

#[test]
fn authored_typing_and_unreached_sum_values_keep_existing_boundaries() {
    let mut failures = Vec::new();
    for (index, (predicate, message)) in [
        (
            "count(i in missing..(1/0)) >= 0",
            "public Kernel cannot type identifier 'missing'",
        ),
        (
            "count(i in 0..n where 1) >= 0",
            "expression of type Int is not assignable to Bool",
        ),
        (
            "sum(i in 1..0 of true) == 0",
            "expression of type Bool is not assignable to Int",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        row(&mut failures, &format!("typing-{index}"), || {
            rejected("Int", predicate, message);
        });
    }
    row(&mut failures, "empty-sum-value", || {
        admitted("Int", "sum(i in 1..0 of (1/0)) == 0");
    });
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}
