// SPDX-License-Identifier: Apache-2.0

//! Direct APIs avoid CLI vacuity masking the concrete domain consumers.
use fsl_core::{
    FsResolver, FslValue, KernelModel, TypeRef, build_model, parse_kernel_source,
    static_leadsto_bindings,
};
use fsl_runtime::{BoundedLivenessMonitor, Monitor, eval, verify_explicit};
use std::collections::BTreeMap;

fn model(source: &str) -> KernelModel {
    eprintln!("SOURCE\n{source}");
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse admission"))
        .expect("checked-model admission, not semantic RED")
}

fn spec(property: &str) -> String {
    format!("spec B {{ state {{ d: Bool = false }} action flip() {{ d = not d }} {property} }}")
}

#[test]
fn builtin_domain_identity_is_not_a_named_type_alias() {
    let m = model(&spec("invariant P { true }"));
    assert_eq!(
        m.domain_values(&TypeRef::Bool).unwrap(),
        [FslValue::Bool(false), FslValue::Bool(true)]
    );
    assert!(m.domain_values(&TypeRef::Named("Bool".into())).is_err());
}

#[test]
fn named_enum_and_bounded_int_consumers_remain_distinct_from_builtin_bool() {
    for (declaration, keys) in [
        (
            "enum Key { A, B }",
            vec![
                FslValue::Enum {
                    type_name: "Key".into(),
                    member: "A".into(),
                },
                FslValue::Enum {
                    type_name: "Key".into(),
                    member: "B".into(),
                },
            ],
        ),
        ("type Key = 0..1", vec![FslValue::Int(0), FslValue::Int(1)]),
    ] {
        let source = format!(
            "spec F {{ {declaration} state {{ m: Map<Key, Bool> }} init {{ forall k: Key {{ m[k] = false }} }} action flip() {{ forall k: Key {{ m[k] = not m[k] }} }} invariant P {{ forall k: Key {{ not m[k] }} and count(k: Key) == 2 }} leadsTo L {{ forall k: Key {{ true ~> within 1 false }} }} }}"
        );
        let m = model(&source);
        assert_eq!(
            m.domain_values(&TypeRef::Named("Key".into())).unwrap(),
            keys
        );
        let mut monitor = Monitor::new(m.clone()).unwrap();
        assert_eq!(
            eval(
                &m.invariants[0].expr,
                &monitor.state,
                &mut BTreeMap::new(),
                &m,
                None
            )
            .unwrap(),
            FslValue::Bool(true)
        );
        let changed = monitor.attempt("flip", &BTreeMap::new()).unwrap();
        eprintln!("NAMED_REJECTING {declaration}: {changed:?}");
        assert_eq!(changed.violation.as_ref().unwrap().name, "P");
        let attempted = changed.attempted_state.as_ref().unwrap();
        assert_eq!(
            attempted["m"],
            FslValue::Map(
                keys.iter()
                    .cloned()
                    .map(|k| (k, FslValue::Bool(true)))
                    .collect()
            )
        );
        assert_eq!(
            eval(
                &m.invariants[0].expr,
                attempted,
                &mut BTreeMap::new(),
                &m,
                None
            )
            .unwrap(),
            FslValue::Bool(false)
        );
        assert_eq!(
            static_leadsto_bindings(&m, &m.leadstos[0]).unwrap(),
            keys.into_iter()
                .map(|k| BTreeMap::from([("k".into(), k)]))
                .collect::<Vec<_>>()
        );
        let mut safety = m;
        safety.leadstos.clear(); // BFS explicitly does not support liveness.
        assert!(verify_explicit(safety, 2, 20).unwrap().violation.is_some());
    }
}

#[test]
fn concrete_bool_quantifiers_and_cardinality_have_both_deciding_controls() {
    let mut failures = Vec::new();
    for (expr, expected) in [
        ("forall b: Bool { b or not b }", true),
        ("forall b: Bool { b }", false),
        ("forall b: Bool { not b }", false),
        ("exists b: Bool { b }", true),
        ("exists b: Bool { not b }", true),
        ("exists b: Bool { false }", false),
        ("unique(b: Bool where b)", true),
        ("unique(b: Bool where not b)", true),
        ("unique(b: Bool)", false),
        ("exactlyOne(b: Bool where b)", true),
        ("exactlyOne(b: Bool where not b)", true),
        ("exactlyOne(b: Bool)", false),
        ("exactlyOne(b: Bool where false)", false),
        ("count(b: Bool) == 2", true),
        ("count(b: Bool) == 1", false),
        ("sum(b: Bool of if b then 1 else 2) == 3", true),
        ("sum(b: Bool of if b then 1 else 2) == 2", false),
    ] {
        let m = model(&spec(&format!("invariant P {{ {expr} }}")));
        let monitor = Monitor::new(m.clone()).expect("deterministic init");
        let actual = eval(
            &m.invariants[0].expr,
            &monitor.state,
            &mut BTreeMap::new(),
            &m,
            None,
        );
        eprintln!("CONCRETE {expr}: expected={expected} produced={actual:?}");
        if actual != Ok(FslValue::Bool(expected)) {
            failures.push(format!("{expr}: {actual:?}"));
        }
        let bfs = verify_explicit(m, 3, 20);
        eprintln!("BFS {expr}: {bfs:?}");
        match bfs {
            Ok(r) if r.violation.is_some() != expected => {
                if let Some(v) = r.violation {
                    assert_eq!(v.violation.name, "P");
                }
            }
            other => failures.push(format!("BFS {expr}: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bool_init_coverage_and_action_execution_are_separate_consumers() {
    let mut failures = Vec::new();
    for init_loop in [false, true] {
        let initial = if init_loop {
            "forall b: Bool { m[b] = b }"
        } else {
            "forall k in Set { false, true } { m[k] = k }"
        };
        let m = model(&format!(
            "spec S {{ state {{ m: Map<Bool, Bool> }} init {{ {initial} }} action flip() {{ forall b: Bool {{ m[b] = not m[b] }} }} invariant P {{ true }} }}"
        ));
        let initial_result = Monitor::new(m.clone());
        eprintln!("INIT init_loop={init_loop}: {initial_result:?}");
        if let Ok(mut monitor) = initial_result {
            let original = FslValue::Map(BTreeMap::from([
                (FslValue::Bool(false), FslValue::Bool(false)),
                (FslValue::Bool(true), FslValue::Bool(true)),
            ]));
            assert_eq!(monitor.state["m"], original);
            let result = monitor.attempt("flip", &BTreeMap::new());
            eprintln!("ACTION init_loop={init_loop}: {result:?}");
            match result {
                Ok(r) => assert_eq!(
                    r.state["m"],
                    FslValue::Map(BTreeMap::from([
                        (FslValue::Bool(false), FslValue::Bool(true)),
                        (FslValue::Bool(true), FslValue::Bool(false))
                    ]))
                ),
                Err(e) => failures.push(format!("action: {e:?}")),
            }
        } else {
            failures.push(format!("init_loop={init_loop}: {initial_result:?}"));
        }
        let bfs = verify_explicit(m, 3, 20);
        eprintln!("COVERAGE/BFS init_loop={init_loop}: {bfs:?}");
        if bfs.is_err() {
            failures.push(format!("coverage init_loop={init_loop}: {bfs:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bool_leadsto_expansion_pending_and_each_zero_deadline_failure() {
    let mut failures = Vec::new();
    for response in ["true", "false", "b", "not b"] {
        let source = spec(&format!(
            "leadsTo P {{ forall b: Bool {{ true ~> within 1 {response} }} }}"
        ));
        let m = model(&source);
        let expected = vec![
            BTreeMap::from([("b".into(), FslValue::Bool(false))]),
            BTreeMap::from([("b".into(), FslValue::Bool(true))]),
        ];
        let expanded = static_leadsto_bindings(&m, &m.leadstos[0]);
        eprintln!("EXPANSION expected={expected:?} produced={expanded:?}");
        if expanded != Ok(expected.clone()) {
            failures.push(format!("expansion: {expanded:?}"));
        }
        let state = Monitor::new(m.clone()).unwrap().state;
        let monitored = BoundedLivenessMonitor::new(m);
        eprintln!("MONITOR construction: {monitored:?}");
        if let Ok(mut live) = monitored {
            assert_eq!(live.observe(&state, 0).unwrap(), None);
            let pending: Vec<_> = live
                .status()
                .pending
                .into_iter()
                .map(|p| {
                    assert_eq!((p.pending_since, p.deadline, p.within), (0, 1, 1));
                    p.bindings
                })
                .collect();
            let want: Vec<_> = expected
                .into_iter()
                .filter(|b| match response {
                    "true" => false,
                    "b" => b["b"] == FslValue::Bool(false),
                    "not b" => b["b"] == FslValue::Bool(true),
                    _ => true,
                })
                .collect();
            assert_eq!(pending, want);
        } else {
            failures.push(format!("monitor: {monitored:?}"));
        }
        let zero = model(&source.replace("within 1", "within 0"));
        match BoundedLivenessMonitor::new(zero) {
            Ok(mut live) => {
                let v = live.observe(&state, 0).unwrap();
                if response == "true" {
                    assert!(v.is_none());
                } else {
                    let v = v.expect("deciding binding must violate");
                    assert_eq!((v.pending_since, v.deadline, v.within), (0, 0, 0));
                    assert_eq!(v.bindings["b"], FslValue::Bool(response == "not b"));
                }
            }
            Err(e) => failures.push(format!("zero: {e:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
