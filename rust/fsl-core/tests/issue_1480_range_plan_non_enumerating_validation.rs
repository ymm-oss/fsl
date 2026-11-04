// SPDX-License-Identifier: Apache-2.0
//! Prospective R1 validation calls; `UNEXECUTED/API_ABSENT` at frozen 2a26 base.
//! Raw aggregate bounds must not enumerate during endpoint type validation.
use fsl_core::binder_range::{RangePlan as R, RangeUnknown as U};
use fsl_core::{FsResolver, KernelExpr as E, TypeRef, build_model, parse_kernel_source};
use serde_json::json;

const BASE: &str = r"spec PlannerValidation {
const MAX=9223372036854775807 const MIN=(-9223372036854775807-1)
state { n:Int }
init { n=0 }
action go() {} invariant P { true }
}";

#[test]
fn raw_aggregate_bounds_keep_full_validation_without_enumeration() {
    // Build only this small aggregate-free model, NEVER a huge aggregate fixture.
    let m = build_model(parse_kernel_source(BASE, &FsResolver::new(".")).unwrap()).unwrap();
    assert_eq!(m.state, vec![("n".to_owned(), TypeRef::Int)]);
    assert_eq!(m.consts["MIN"], fsl_core::FslValue::Int(i64::MIN));
    assert_eq!(m.consts["MAX"], fsl_core::FslValue::Int(i64::MAX));
    let hi = E::Num(0);
    for (label, source, raw_ast, expected, wrong_reason) in [
        (
            "static_free",
            "count(j in MIN..MAX)",
            json!([
                "aggregate",
                "count",
                ["binder_range", "j", ["var", "MIN"], ["var", "MAX"]],
                null
            ]),
            R::Unknown(U::ConstantFoldFailed),
            R::Unknown(U::UnsupportedExpression),
        ),
        (
            "dynamic_state",
            "count(j in n..MAX)",
            json!([
                "aggregate",
                "count",
                ["binder_range", "j", ["var", "n"], ["var", "MAX"]],
                null
            ]),
            R::Unknown(U::UnsupportedExpression),
            R::Unknown(U::MissingFiniteFact),
        ),
        (
            "inactive_illtyped_filter",
            "if true then 0 else count(j in MIN..MAX where 1)",
            json!([
                "ite",
                ["bool", true],
                ["num", 0],
                [
                    "aggregate",
                    "count",
                    [
                        "binder_range",
                        "j",
                        ["var", "MIN"],
                        ["var", "MAX"],
                        ["num", 1]
                    ],
                    null
                ]
            ]),
            R::Unknown(U::UnsupportedExpression),
            R::Unknown(U::ConstantFoldFailed),
        ),
    ] {
        // Frozen T2's parser-only extraction: no model build/typecheck of source.
        let lo = fsl_syntax::parse_expr(source).expect("raw AST grammar only");
        assert_eq!(
            lo.kernel_ast_v1(),
            raw_ast,
            "{label}: authored AST retained"
        );
        let actual = m.plan_integer_range(&lo, &hi, &[]);
        eprintln!(
            "VALIDATION_PLAN {label} source={source:?} lo={lo:?} hi={hi:?} lexical=[] actual={actual:?} expected={expected:?}"
        );
        assert_ne!(actual, wrong_reason, "{label}: Unknown reason separation");
        assert_ne!(
            actual,
            R::Static { lo: 0, hi: 0 },
            "{label}: raw endpoint must not bypass validation/folding"
        );
        assert_eq!(actual, expected, "{label}");
    }
}
