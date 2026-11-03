// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, KernelAggregateKind as Kind, KernelBinder as Binder, KernelExpr as Expr,
    build_model, parse_kernel_source, validate_boolean_expression,
};

const MODEL: &str = "spec NestedDeferred { state { n: Int } init { n = 0 } invariant I { true } }";
const CLOSED_BOUND_ERROR: &str = "public Kernel requires static aggregate range bounds";

#[test]
fn deferred_outer_value_still_checks_nested_closed_invalid_bound() {
    let model = build_model(parse_kernel_source(MODEL, &FsResolver::new(".")).unwrap()).unwrap();
    let nested = Expr::Aggregate {
        kind: Kind::Count,
        binder: Binder::Range {
            name: "j".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Binary {
                op: "/".into(),
                left: Box::new(Expr::Num(1)),
                right: Box::new(Expr::Num(0)),
            }),
            where_expr: None,
        },
        value: None,
    };
    let outer = Expr::Aggregate {
        kind: Kind::Sum,
        binder: Binder::Range {
            name: "i".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: None,
        },
        value: Some(Box::new(nested)),
    };
    let predicate = Expr::Binary {
        op: ">=".into(),
        left: Box::new(outer),
        right: Box::new(Expr::Num(0)),
    };
    assert_eq!(
        validate_boolean_expression(&predicate, &model),
        Err(CLOSED_BOUND_ERROR.into())
    );
}

#[test]
fn deferred_nested_sibling_cannot_skip_later_closed_invalid_bound() {
    let model = build_model(parse_kernel_source(MODEL, &FsResolver::new(".")).unwrap()).unwrap();
    let first = Expr::Aggregate {
        kind: Kind::Count,
        binder: Binder::Range {
            name: "j".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: None,
        },
        value: None,
    };
    let later = Expr::Aggregate {
        kind: Kind::Count,
        binder: Binder::Range {
            name: "k".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Binary {
                op: "/".into(),
                left: Box::new(Expr::Num(1)),
                right: Box::new(Expr::Num(0)),
            }),
            where_expr: None,
        },
        value: None,
    };
    let outer = Expr::Aggregate {
        kind: Kind::Sum,
        binder: Binder::Range {
            name: "i".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: None,
        },
        value: Some(Box::new(Expr::Binary {
            op: "+".into(),
            left: Box::new(first),
            right: Box::new(later),
        })),
    };
    let predicate = Expr::Binary {
        op: ">=".into(),
        left: Box::new(outer),
        right: Box::new(Expr::Num(0)),
    };
    assert_eq!(
        validate_boolean_expression(&predicate, &model),
        Err(CLOSED_BOUND_ERROR.into())
    );
}

#[test]
fn deferred_outer_filter_still_checks_nested_closed_invalid_bound() {
    let model = build_model(parse_kernel_source(MODEL, &FsResolver::new(".")).unwrap()).unwrap();
    let nested = Expr::Aggregate {
        kind: Kind::Count,
        binder: Binder::Range {
            name: "j".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Binary {
                op: "/".into(),
                left: Box::new(Expr::Num(1)),
                right: Box::new(Expr::Num(0)),
            }),
            where_expr: None,
        },
        value: None,
    };
    let outer = Expr::Aggregate {
        kind: Kind::Sum,
        binder: Binder::Range {
            name: "i".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: Some(Box::new(Expr::Binary {
                op: ">".into(),
                left: Box::new(nested),
                right: Box::new(Expr::Num(0)),
            })),
        },
        value: Some(Box::new(Expr::Num(1))),
    };
    let predicate = Expr::Binary {
        op: ">=".into(),
        left: Box::new(outer),
        right: Box::new(Expr::Num(0)),
    };
    assert_eq!(
        validate_boolean_expression(&predicate, &model),
        Err(CLOSED_BOUND_ERROR.into())
    );
}

#[test]
fn valid_nested_dynamic_bounds_remain_accepted() {
    let model = build_model(parse_kernel_source(MODEL, &FsResolver::new(".")).unwrap()).unwrap();
    let nested = Expr::Aggregate {
        kind: Kind::Count,
        binder: Binder::Range {
            name: "j".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: None,
        },
        value: None,
    };
    let outer = Expr::Aggregate {
        kind: Kind::Sum,
        binder: Binder::Range {
            name: "i".into(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Var("n".into())),
            where_expr: None,
        },
        value: Some(Box::new(nested)),
    };
    let predicate = Expr::Binary {
        op: ">=".into(),
        left: Box::new(outer),
        right: Box::new(Expr::Num(0)),
    };
    assert_eq!(validate_boolean_expression(&predicate, &model), Ok(()));
}

#[test]
fn locked_empty_range_keeps_division_by_zero_sum_value_accepted() {
    let model = build_model(parse_kernel_source(MODEL, &FsResolver::new(".")).unwrap()).unwrap();
    let aggregate = Expr::Aggregate {
        kind: Kind::Sum,
        binder: Binder::Range {
            name: "i".into(),
            lo: Box::new(Expr::Num(1)),
            hi: Box::new(Expr::Num(0)),
            where_expr: None,
        },
        value: Some(Box::new(Expr::Binary {
            op: "/".into(),
            left: Box::new(Expr::Num(1)),
            right: Box::new(Expr::Num(0)),
        })),
    };
    let predicate = Expr::Binary {
        op: "==".into(),
        left: Box::new(aggregate),
        right: Box::new(Expr::Num(0)),
    };
    assert_eq!(validate_boolean_expression(&predicate, &model), Ok(()));
}
