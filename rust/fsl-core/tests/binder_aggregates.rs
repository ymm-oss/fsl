// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, KernelAggregateKind, KernelBinder, KernelExpr, build_model, parse_kernel_source,
    public_kernel_contract, substitute_expr,
};
use serde_json::Value;
use std::collections::HashMap;

fn contains_kind(value: &Value, expected: &str) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| contains_kind(value, expected)),
        Value::Object(values) => {
            values.get("kind").and_then(Value::as_str) == Some(expected)
                || values.values().any(|value| contains_kind(value, expected))
        }
        _ => false,
    }
}

fn collect_aggregates<'a>(expr: &'a KernelExpr, output: &mut Vec<&'a KernelExpr>) {
    match expr {
        KernelExpr::Binary { left, right, .. } => {
            collect_aggregates(left, output);
            collect_aggregates(right, output);
        }
        KernelExpr::Aggregate { value, .. } => {
            output.push(expr);
            if let Some(value) = value {
                collect_aggregates(value, output);
            }
        }
        _ => {}
    }
}

#[test]
fn aggregate_ir_uses_one_binder_shape_and_public_kernel_keeps_v1_expression_kinds() {
    let source = r"
spec AggregateIr {
  type Item = 0..2
  state { queue: Seq<Item, 3> }
  init { queue = Seq { 1, 1 } }
  action stay() { queue = queue }
  invariant Values {
    count(item in queue where item == 1) == 2 and
    sum(item in queue of item where item > 0) == 2 and
    unique(item in queue where item == 2)
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower source");
    let model = build_model(kernel.clone()).expect("build model");
    let property = &model.invariants[0].expr;
    let mut aggregates = Vec::new();
    collect_aggregates(property, &mut aggregates);
    assert_eq!(aggregates.len(), 3);
    assert!(aggregates.iter().all(|aggregate| matches!(
        aggregate,
        KernelExpr::Aggregate {
            binder: KernelBinder::Collection { .. },
            ..
        }
    )));

    let public = public_kernel_contract(&kernel, &model, "aggregate.fsl", "spec")
        .expect("export public Kernel");
    assert!(!contains_kind(&public, "count"));
    assert!(!contains_kind(&public, "sum"));
    assert!(contains_kind(&public, "ite"));
}

#[test]
fn aggregate_filters_are_checked_for_every_binder_kind() {
    for expression in [
        "count(item: Item where 1)",
        "count(item in 0..2 where 1)",
        "count(item in queue where 1)",
    ] {
        let source = format!(
            "spec Invalid {{ type Item = 0..2 state {{ queue: Seq<Item, 2> }} init {{ queue = Seq {{}} }} action stay() {{ queue = queue }} invariant Bad {{ {expression} == 0 }} }}"
        );
        let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("parse source");
        let error = build_model(kernel).expect_err("non-Bool filter must fail");
        assert!(error.message.contains("Bool"), "{}", error.message);
    }
}

#[test]
fn aggregate_rejects_map_binders() {
    let source = r"
spec InvalidMapAggregate {
  type Item = 0..1
  state { values: Map<Item, Item> }
  init { forall item: Item { values[item] = 0 } }
  action stay() { values[0] = values[0] }
  invariant Bad { count(item in values) == 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
    let error = build_model(kernel).expect_err("Map binder must fail");
    assert!(
        error
            .message
            .contains("collection binder requires Set or Seq")
    );
}

#[test]
fn business_and_requirements_kpis_are_typed_metadata_projections() {
    for source in [
        r"
business KpiProjection {
  actor User
  entity Claim
  process Claim {
    stages Draft, Paid
    initial Draft
    transition Pay Draft -> Paid by User
  }
  kpi paid = count Claim in Paid
}
verify { instances Claim = 2 }
",
        r"
requirements KpiProjection {
  process Claim {
    stages Draft, Paid
    initial Draft
    transition Pay Draft -> Paid by User
  }
  kpi paid = count Claim in Paid
}
verify { instances Claim = 2 }
",
    ] {
        let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower KPI");
        assert_eq!(kernel.projections().len(), 1);
        assert!(matches!(
            kernel.projections()[0].expr,
            KernelExpr::Aggregate {
                kind: KernelAggregateKind::Count,
                binder: KernelBinder::Typed { .. },
                value: None,
            }
        ));
        let model = build_model(kernel).expect("validate KPI projection");
        assert_eq!(model.projections[0].name, "paid");
        assert!(
            !model
                .invariants
                .iter()
                .any(|property| property.name.starts_with("_kpi"))
        );
    }
}

#[test]
fn kpi_rejects_unknown_stages_during_lowering() {
    let source = r"
business InvalidKpi {
  actor User
  entity Claim
  process Claim {
    stages Draft, Paid
    initial Draft
    transition Pay Draft -> Paid by User
  }
  kpi missing = count Claim in Missing
}
verify { instances Claim = 1 }
";
    let error = parse_kernel_source(source, &FsResolver::new(".")).expect_err("unknown stage");
    assert!(error.message.contains("unknown stage 'Missing'"));
}

#[test]
fn substitution_respects_nested_binder_shadowing() {
    let expr = KernelExpr::Aggregate {
        kind: KernelAggregateKind::Sum,
        binder: KernelBinder::Range {
            name: "item".to_owned(),
            lo: Box::new(KernelExpr::Var("item".to_owned())),
            hi: Box::new(KernelExpr::Num(2)),
            where_expr: Some(Box::new(KernelExpr::Var("item".to_owned()))),
        },
        value: Some(Box::new(KernelExpr::Var("item".to_owned()))),
    };
    let substituted = substitute_expr(
        expr,
        &HashMap::from([("item".to_owned(), KernelExpr::Num(7))]),
    );
    let KernelExpr::Aggregate { binder, value, .. } = substituted else {
        panic!("expected aggregate");
    };
    let KernelBinder::Range { lo, where_expr, .. } = binder else {
        panic!("expected range binder");
    };
    assert_eq!(*lo, KernelExpr::Num(7));
    assert_eq!(
        *where_expr.expect("filter"),
        KernelExpr::Var("item".to_owned())
    );
    assert_eq!(
        *value.expect("sum value"),
        KernelExpr::Var("item".to_owned())
    );

    let candidate = KernelExpr::Method {
        receiver: Box::new(KernelExpr::Var("queue".to_owned())),
        name: "at".to_owned(),
        args: vec![KernelExpr::Num(0)],
    };
    let nested = KernelExpr::Aggregate {
        kind: KernelAggregateKind::Count,
        binder: KernelBinder::Collection {
            name: "queue".to_owned(),
            collection: Box::new(KernelExpr::Var("selected".to_owned())),
            where_expr: Some(Box::new(KernelExpr::Binary {
                op: "==".to_owned(),
                left: Box::new(KernelExpr::Var("queue".to_owned())),
                right: Box::new(KernelExpr::Var("item".to_owned())),
            })),
        },
        value: None,
    };
    let substituted = substitute_expr(
        nested,
        &HashMap::from([("item".to_owned(), candidate.clone())]),
    );
    let KernelExpr::Aggregate { binder, .. } = substituted else {
        panic!("expected nested aggregate");
    };
    let KernelBinder::Collection {
        name, where_expr, ..
    } = binder
    else {
        panic!("expected collection binder");
    };
    assert_ne!(name, "queue");
    let KernelExpr::Binary { left, right, .. } = *where_expr.expect("filter") else {
        panic!("expected equality filter");
    };
    assert_eq!(*left, KernelExpr::Var(name));
    assert_eq!(*right, candidate);

    let source_scoped_free_variable = KernelExpr::Aggregate {
        kind: KernelAggregateKind::Count,
        binder: KernelBinder::Collection {
            name: "queue".to_owned(),
            collection: Box::new(KernelExpr::Var("queue".to_owned())),
            where_expr: None,
        },
        value: None,
    };
    let target = KernelExpr::Aggregate {
        kind: KernelAggregateKind::Count,
        binder: KernelBinder::Collection {
            name: "queue".to_owned(),
            collection: Box::new(KernelExpr::Var("selected".to_owned())),
            where_expr: Some(Box::new(KernelExpr::Var("item".to_owned()))),
        },
        value: None,
    };
    let substituted = substitute_expr(
        target,
        &HashMap::from([("item".to_owned(), source_scoped_free_variable)]),
    );
    let KernelExpr::Aggregate {
        binder: KernelBinder::Collection { name, .. },
        ..
    } = substituted
    else {
        panic!("expected collection aggregate");
    };
    assert_ne!(name, "queue");
}

/// Indexed (per-element) substitution (`substitute_expr_indexed`, issue #483)
/// must be as capture-avoiding as the existing scalar case above: a `forall`
/// binder that happens to share a name with a free variable of the indexed
/// map's own mapping expression must be renamed before the read is expanded,
/// not left to accidentally capture that variable.
#[test]
fn indexed_substitution_avoids_capturing_the_mapping_expressions_free_variable() {
    // map a[i] = queue[i] — the indexed map's own binder is `i`, and its
    // mapping expression `queue[i]` has `queue` free.
    let state_expr = KernelExpr::Index(
        Box::new(KernelExpr::Var("queue".to_owned())),
        Box::new(KernelExpr::Var("i".to_owned())),
    );
    let state_binder = KernelBinder::Range {
        name: "i".to_owned(),
        lo: Box::new(KernelExpr::Num(0)),
        hi: Box::new(KernelExpr::Num(2)),
        where_expr: None,
    };
    let indexed = HashMap::from([("a".to_owned(), (state_binder, state_expr))]);

    // forall queue: T { a[queue] } — the *outer* binder is also named
    // `queue`, colliding with the mapping expression's free variable.
    let target = KernelExpr::Quantified {
        quantifier: "forall".to_owned(),
        binder: KernelBinder::Range {
            name: "queue".to_owned(),
            lo: Box::new(KernelExpr::Num(0)),
            hi: Box::new(KernelExpr::Num(2)),
            where_expr: None,
        },
        body: Box::new(KernelExpr::Index(
            Box::new(KernelExpr::Var("a".to_owned())),
            Box::new(KernelExpr::Var("queue".to_owned())),
        )),
    };

    let substituted =
        fsl_core::substitute_expr_indexed(target, &HashMap::<String, KernelExpr>::new(), &indexed);
    let KernelExpr::Quantified {
        binder: KernelBinder::Range { name: fresh, .. },
        body,
        ..
    } = substituted
    else {
        panic!("expected forall");
    };
    assert_ne!(
        fresh, "queue",
        "the outer binder must be renamed to avoid capture"
    );
    // The result must be `queue[<fresh>]`, i.e. the mapping expression's own
    // `queue` still refers to the free implementation variable, not to the
    // (renamed) outer binder.
    let KernelExpr::Index(base, index) = *body else {
        panic!("expected an index expression after indexed substitution");
    };
    assert_eq!(*base, KernelExpr::Var("queue".to_owned()));
    assert_eq!(*index, KernelExpr::Var(fresh));
}

#[test]
fn public_partial_operations_expand_aggregate_binders_without_free_variables() {
    let source = r"
spec AggregatePartialOperations {
  type Item = 0..1
  state { queue: Seq<Item, 2>, total: Int }
  init { queue = Seq {} total = 0 }
  action collection() { total = sum(item in queue of 1 / item where 1 / item > 0) }
  action typed() { total = sum(item: Item of 1 / item) }
  action distinct() { requires unique(item: Item where 1 / item > 0) total = total }
  action quantified() { requires forall item: Item { 1 / item > 0 } total = total }
  action existential() { requires exists item: Item { item == 0 or 1 / (item - 1) > 0 } total = total }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower source");
    let model = build_model(kernel.clone()).expect("build model");
    let public = public_kernel_contract(&kernel, &model, "aggregate.fsl", "spec")
        .expect("export public Kernel");
    let actions = public["actions"].as_array().expect("actions");
    let mut saw_existential = false;
    for action in actions {
        let operations = action["partial_operations"]
            .as_array()
            .expect("partial operations");
        assert!(!operations.is_empty());
        assert!(operations.iter().all(|operation| {
            !operation["failure_condition"]
                .to_string()
                .contains("\"name\":\"item\"")
        }));
        assert!(operations.iter().all(|operation| {
            operation["operation"] != "at"
                || operation["failure_condition"]
                    .to_string()
                    .contains("\"operator\":\"and\"")
        }));
        if action["name"] == "existential" {
            saw_existential = true;
            let divisions = operations
                .iter()
                .filter(|operation| operation["operation"] == "divide")
                .collect::<Vec<_>>();
            assert_eq!(divisions.len(), 2);
            assert!(
                divisions[1]["failure_condition"]
                    .to_string()
                    .contains("\"kind\":\"not\"")
            );
        }
    }
    assert!(saw_existential);
}

#[test]
fn checked_model_rejects_dynamic_aggregate_ranges_in_every_expression_position() {
    let sources = [
        "spec DynamicInit { state { lo: Int, hi: Int, total: Int } init { lo = 0 hi = 2 total = count(i in lo..hi) } action stay() { lo = lo hi = hi total = total } invariant I { true } }",
        "spec DynamicAction { state { lo: Int, hi: Int, total: Int } init { lo = 0 hi = 2 total = 0 } action stay() { lo = lo hi = hi total = count(i in lo..hi) } invariant I { true } }",
        "spec DynamicProperty { state { lo: Int, hi: Int, total: Int } init { lo = 0 hi = 2 total = 0 } action stay() { lo = lo hi = hi total = total } invariant I { count(i in lo..hi) >= 0 } }",
    ];

    for source in sources {
        let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
        let error = build_model(kernel).expect_err("dynamic aggregate range must fail closed");
        assert!(
            error.message.contains("'lo' is not an integer const"),
            "{}",
            error.message
        );
    }
}

// #1569: these tests exercise the unresolved Stage AST directly, before dialect
// lowering can accidentally resolve a stale operand to an outer entity binder.
mod stage_substitution {
    use super::*;

    fn stage(name: &str) -> KernelExpr {
        let span = serde_json::from_value(serde_json::json!({
            "start": { "offset": 10, "line": 2, "column": 3 },
            "end": { "offset": 18, "line": 2, "column": 11 },
        }))
        .expect("span");
        KernelExpr::Stage {
            process: Some(Box::new(
                fsl_core::SymbolPath::new(["Claim".into()], span).unwrap(),
            )),
            entity: Box::new(KernelExpr::Var(name.into())),
            entity_span: span,
            span,
        }
    }

    fn binary(op: &str, left: KernelExpr, right: KernelExpr) -> KernelExpr {
        KernelExpr::Binary {
            op: op.into(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn condition(name: &str) -> KernelExpr {
        binary(
            "and",
            binary("==", stage(name), KernelExpr::Var("Approved".into())),
            binary("==", KernelExpr::Var("amount".into()), KernelExpr::Num(0)),
        )
    }

    fn replacement() -> HashMap<String, KernelExpr> {
        HashMap::from([(
            "amount".into(),
            KernelExpr::Index(
                Box::new(KernelExpr::Var("claim_amount".into())),
                Box::new(KernelExpr::Var("c".into())),
            ),
        )])
    }

    fn expected_condition(name: &str) -> KernelExpr {
        binary(
            "and",
            binary("==", stage(name), KernelExpr::Var("Approved".into())),
            binary("==", replacement()["amount"].clone(), KernelExpr::Num(0)),
        )
    }

    fn binder(kind: usize, name: &str, filter: Option<KernelExpr>) -> KernelBinder {
        let where_expr = filter.map(Box::new);
        match kind {
            0 => KernelBinder::Typed {
                name: name.into(),
                type_name: fsl_core::QualifiedName {
                    namespace: None,
                    name: "Claim".into(),
                },
                where_expr,
            },
            1 => KernelBinder::Range {
                name: name.into(),
                lo: Box::new(KernelExpr::Num(0)),
                hi: Box::new(KernelExpr::Num(1)),
                where_expr,
            },
            2 => KernelBinder::Collection {
                name: name.into(),
                collection: Box::new(KernelExpr::Var("claims".into())),
                where_expr,
            },
            _ => unreachable!(),
        }
    }

    fn name_and_filter(binder: &KernelBinder) -> (&str, &Option<Box<KernelExpr>>) {
        match binder {
            KernelBinder::Typed {
                name, where_expr, ..
            }
            | KernelBinder::Range {
                name, where_expr, ..
            }
            | KernelBinder::Collection {
                name, where_expr, ..
            } => (name, where_expr),
        }
    }

    #[test]
    fn detector_free_stage_operand_is_substituted_with_metadata_preserved() {
        assert_eq!(
            substitute_expr(
                stage("x"),
                &HashMap::from([("x".into(), KernelExpr::Var("y".into()))])
            ),
            stage("y")
        );
    }

    #[test]
    fn detector_free_stage_operand_in_every_binder_filter_is_substituted() {
        for kind in 0..3 {
            let expr = KernelExpr::Quantified {
                quantifier: "exists".into(),
                binder: binder(kind, "d", Some(stage("x"))),
                body: Box::new(KernelExpr::Bool(true)),
            };
            let result = substitute_expr(
                expr,
                &HashMap::from([("x".into(), KernelExpr::Var("y".into()))]),
            );
            let KernelExpr::Quantified { binder, .. } = result else {
                panic!("quantifier")
            };
            assert_eq!(
                name_and_filter(&binder).1.as_deref(),
                Some(&stage("y")),
                "binder kind {kind}"
            );
        }
    }

    #[test]
    fn detector_quantifier_renames_stage_with_binder_in_body_and_filter() {
        for kind in 0..3 {
            let expr = KernelExpr::Quantified {
                quantifier: "exists".into(),
                binder: binder(kind, "c", Some(condition("c"))),
                body: Box::new(condition("c")),
            };
            let KernelExpr::Quantified { binder, body, .. } = substitute_expr(expr, &replacement())
            else {
                panic!("quantifier")
            };
            let (fresh, filter) = name_and_filter(&binder);
            assert_ne!(fresh, "c");
            let expected = expected_condition(fresh);
            assert_eq!(*body, expected, "body, binder kind {kind}");
            assert_eq!(
                filter.as_deref(),
                Some(&expected),
                "filter, binder kind {kind}"
            );
        }
    }

    #[test]
    fn detector_aggregate_renames_stage_with_binder_in_value_and_filter() {
        for kind in 0..3 {
            let expr = KernelExpr::Aggregate {
                kind: KernelAggregateKind::Sum,
                binder: binder(kind, "c", Some(condition("c"))),
                value: Some(Box::new(stage("c"))),
            };
            let KernelExpr::Aggregate { binder, value, .. } = substitute_expr(expr, &replacement())
            else {
                panic!("aggregate")
            };
            let (fresh, filter) = name_and_filter(&binder);
            assert_ne!(fresh, "c");
            assert_eq!(
                value.as_deref(),
                Some(&stage(fresh)),
                "value, binder kind {kind}"
            );
            assert_eq!(
                filter.as_deref(),
                Some(&expected_condition(fresh)),
                "filter, binder kind {kind}"
            );
        }
    }

    #[test]
    fn detector_nested_shadowing_stage_reads_follow_their_own_binders() {
        // The inner c shadows the outer c; the d scope still reads the outer c.
        let inner = KernelExpr::Quantified {
            quantifier: "exists".into(),
            binder: binder(0, "c", None),
            body: Box::new(condition("c")),
        };
        let middle = KernelExpr::Quantified {
            quantifier: "forall".into(),
            binder: binder(0, "d", None),
            body: Box::new(binary("and", stage("c"), inner)),
        };
        let outer = KernelExpr::Quantified {
            quantifier: "exists".into(),
            binder: binder(0, "c", None),
            body: Box::new(middle),
        };
        let KernelExpr::Quantified {
            binder: outer_binder,
            body,
            ..
        } = substitute_expr(outer, &replacement())
        else {
            panic!("outer")
        };
        let outer_name = name_and_filter(&outer_binder).0;
        assert_ne!(outer_name, "c");
        let KernelExpr::Quantified {
            binder: middle_binder,
            body,
            ..
        } = *body
        else {
            panic!("middle")
        };
        assert_eq!(name_and_filter(&middle_binder).0, "d");
        let KernelExpr::Binary { left, right, .. } = *body else {
            panic!("and")
        };
        assert_eq!(*left, stage(outer_name));
        let KernelExpr::Quantified {
            binder: inner_binder,
            body,
            ..
        } = *right
        else {
            panic!("inner")
        };
        let inner_name = name_and_filter(&inner_binder).0;
        assert_ne!(inner_name, "c");
        assert_eq!(*body, expected_condition(inner_name));
    }

    #[test]
    fn control_noncapturing_stage_binder_and_free_replacement_stay_distinct() {
        let expr = KernelExpr::Quantified {
            quantifier: "exists".into(),
            binder: binder(0, "d", Some(condition("d"))),
            body: Box::new(condition("d")),
        };
        let expected = KernelExpr::Quantified {
            quantifier: "exists".into(),
            binder: binder(0, "d", Some(expected_condition("d"))),
            body: Box::new(expected_condition("d")),
        };
        assert_eq!(substitute_expr(expr, &replacement()), expected);
    }
}
