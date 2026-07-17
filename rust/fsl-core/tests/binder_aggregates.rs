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
