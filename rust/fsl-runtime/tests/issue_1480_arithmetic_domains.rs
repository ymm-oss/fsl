// SPDX-License-Identifier: Apache-2.0

//! Position-specific measurements, not a blanket admission policy.
use fsl_core::{
    FsResolver, FslValue, KernelModel, build_model, parse_kernel_source, static_leadsto_bindings,
};
use fsl_runtime::{BoundedLivenessMonitor, Monitor, eval, verify_explicit};
use std::collections::BTreeMap;

#[test]
fn finite_arithmetic_execution_and_coverage_measurements() {
    // None is an invalid/noninteger bound, NOT an empty literal domain.
    for (range, values) in [
        ("0..1", Some(vec![0, 1])),
        ("0..N", Some(vec![0, 1])),
        ("-1..0", Some(vec![-1, 0])),
        ("0..(1 + 0)", Some(vec![0, 1])),
        ("0..(3 - 2)", Some(vec![0, 1])),
        ("0..(1 * 1)", Some(vec![0, 1])),
        ("1..0", Some(vec![])),
        ("0..(9223372036854775807 + 1)", None),
        ("false..true", None),
    ] {
        for position in [
            "ordinary",
            "aggregate",
            "init",
            "action",
            "leadsTo",
            "coverage",
        ] {
            let loop_text = format!("forall i in {range} {{ m[i] = true }}");
            let initial = match position {
                "init" | "coverage" => loop_text.clone(),
                _ => "forall k: Key { m[k] = false }".into(),
            };
            let action = if position == "action" {
                loop_text
            } else {
                String::new()
            };
            let property = match position {
                "ordinary" => {
                    format!("invariant P {{ forall i in {range} {{ i >= -1 and i <= 1 }} }}")
                }
                "aggregate" => format!(
                    "invariant P {{ count(i in {range}) == {} }}",
                    values.as_ref().map_or(0, Vec::len)
                ),
                "leadsTo" => {
                    format!("leadsTo P {{ forall i in {range} {{ true ~> within 1 false }} }}")
                }
                _ => "invariant P { true }".into(),
            };
            let key = if range == "-1..0" { "-1..0" } else { "0..1" };
            let source = format!(
                "spec A {{ const N = 1 type Key = {key} state {{ m: Map<Key, Bool> }} init {{ {initial} }} action go() {{ {action} }} {property} }}"
            );
            eprintln!("SOURCE {range}/{position}\n{source}");
            let admitted = build_model(
                parse_kernel_source(&source, &FsResolver::new(".")).expect("fixture syntax"),
            );
            eprintln!(
                "MEASURE {range}/{position} literal_values={values:?} admission={:?}",
                admitted.as_ref().map(|_| ()).map_err(ToString::to_string)
            );
            let Ok(m) = admitted else { continue };
            measure_arithmetic_domain_position(m, range, position, values.as_deref());
        }
    }
}

fn measure_arithmetic_domain_position(
    m: KernelModel,
    range: &str,
    position: &str,
    values: Option<&[i64]>,
) {
    let state = Monitor::new(m.clone());
    eprintln!(
        "MEASURE {range}/{position} Monitor={:?}",
        state.as_ref().map(|s| &s.state)
    );
    if let Ok(mut monitor) = state {
        if position == "init" {
            assert_eq!(
                monitor.state["m"],
                FslValue::Map(
                    values
                        .as_ref()
                        .expect("invalid init must refuse")
                        .iter()
                        .map(|v| (FslValue::Int(*v), FslValue::Bool(true)))
                        .collect()
                )
            );
        }
        if position == "action" {
            let r = monitor.attempt("go", &BTreeMap::new());
            eprintln!("MEASURE {range}/{position} action={r:?}");
            if let Ok(r) = r {
                let expected = if values.is_some_and(<[i64]>::is_empty) {
                    vec![
                        (FslValue::Int(0), FslValue::Bool(false)),
                        (FslValue::Int(1), FslValue::Bool(false)),
                    ]
                    .into_iter()
                    .collect()
                } else {
                    values
                        .as_ref()
                        .expect("invalid action must refuse")
                        .iter()
                        .map(|v| (FslValue::Int(*v), FslValue::Bool(true)))
                        .collect()
                };
                assert_eq!(r.state["m"], FslValue::Map(expected));
            }
        }
        if matches!(position, "ordinary" | "aggregate") {
            let actual = eval(
                &m.invariants[0].expr,
                &monitor.state,
                &mut BTreeMap::new(),
                &m,
                None,
            );
            eprintln!("MEASURE {range}/{position} eval={actual:?}");
            if let Ok(v) = actual {
                assert!(values.is_some(), "invalid bound must not evaluate");
                assert_eq!(v, FslValue::Bool(true));
            }
        }
    }
    if position == "leadsTo" {
        let actual = static_leadsto_bindings(&m, &m.leadstos[0]);
        eprintln!("MEASURE {range}/{position} expansion={actual:?}");
        if let Ok(v) = actual {
            assert_eq!(
                v,
                values
                    .as_ref()
                    .expect("invalid expansion must refuse")
                    .iter()
                    .map(|i| BTreeMap::from([("i".into(), FslValue::Int(*i))]))
                    .collect::<Vec<_>>()
            );
            let state = Monitor::new(m.clone()).unwrap().state;
            let mut live = BoundedLivenessMonitor::new(m.clone()).unwrap();
            assert_eq!(live.observe(&state, 0).unwrap(), None);
            assert_eq!(
                live.status()
                    .pending
                    .into_iter()
                    .map(|p| p.bindings)
                    .collect::<Vec<_>>(),
                v
            );
        }
    }
    if position == "coverage" {
        eprintln!(
            "MEASURE {range}/{position} coverage={:?}",
            verify_explicit(m, 1, 10)
        );
    }
}

#[test]
fn arithmetic_expression_per_value_rejecting_controls() {
    for (range, values) in [
        ("0..N", vec![0, 1]),
        ("-1..0", vec![-1, 0]),
        ("0..(1 + 0)", vec![0, 1]),
        ("0..(3 - 2)", vec![0, 1]),
        ("0..(1 * 1)", vec![0, 1]),
    ] {
        for value in values {
            for expr in [
                format!("forall i in {range} {{ i != {value} }}"),
                format!("count(i in {range} where i == {value}) == 0"),
            ] {
                let source = format!(
                    "spec A {{ const N = 1 state {{ d: Bool = false }} action flip() {{ d = not d }} invariant P {{ {expr} }} }}"
                );
                eprintln!("SOURCE\n{source}");
                let admitted =
                    build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap());
                eprintln!(
                    "PER_VALUE {range}/{value} admission={:?}",
                    admitted.as_ref().map(|_| ()).map_err(ToString::to_string)
                );
                if let Ok(m) = admitted {
                    let monitor = Monitor::new(m.clone()).unwrap();
                    let actual = eval(
                        &m.invariants[0].expr,
                        &monitor.state,
                        &mut BTreeMap::new(),
                        &m,
                        None,
                    );
                    eprintln!("PER_VALUE expected=false produced={actual:?}");
                    if let Ok(v) = actual {
                        assert_eq!(v, FslValue::Bool(false));
                    }
                }
            }
        }
    }
}

#[test]
fn init_conservative_domains_and_read_order_preservation() {
    for (initial, message) in [
        (
            "forall i in 0..n { m[i] = true } n = 0",
            "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        ),
        (
            "n = 1 forall i in 0..n { m[i] = true }",
            "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        ),
        (
            "forall i in 0..1 where n == 0 { m[i] = true } n = 0",
            "init references state variable 'n' before it is assigned",
        ),
    ] {
        let source = format!(
            "spec C {{ type Key = 0..1 state {{ n: Key, m: Map<Key, Bool> }} init {{ {initial} }} action go() {{}} }}"
        );
        let m = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
        let actual = verify_explicit(m, 2, 20).unwrap_err();
        eprintln!("PRESERVE source={source} expected={message} produced={actual:?}");
        assert_eq!(actual.message, message);
    }
    let outer = "spec O { struct T { a: Bool, b: Bool, c: Bool } state { s: T, free: Bool } init { s.a = true s.b = true forall limit in 0..0 { forall j in 1..limit { s.c = false } } } action idle() {} }";
    let admitted = build_model(parse_kernel_source(outer, &FsResolver::new(".")).unwrap());
    let actual = verify_explicit(admitted.unwrap(), 2, 20).unwrap_err();
    eprintln!("OUTER_DEPENDENT source={outer} produced={actual:?}");
    assert_eq!(
        actual.message,
        "state variable 's' assigned more than once in init"
    );
    assert_eq!(
        (
            actual.span.unwrap().start.line,
            actual.span.unwrap().start.column
        ),
        (1, 94)
    );
    for (initial, expected) in [
        ("y = 1 x = y", None),
        (
            "x = y y = 1",
            Some("init references state variable 'y' before it is assigned"),
        ),
    ] {
        let source = format!(
            "spec R {{ type K = 0..1 state {{ x: K, y: K }} init {{ {initial} }} action go() {{}} }}"
        );
        let m = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
        let actual = verify_explicit(m, 2, 20);
        eprintln!("READ_ORDER {initial}: {actual:?}");
        if let Some(message) = expected {
            assert_eq!(actual.unwrap_err().message, message);
        } else {
            assert!(actual.is_ok());
        }
    }
}
