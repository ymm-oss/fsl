// SPDX-License-Identifier: Apache-2.0

//! Ordering must decide short-circuit/partiality, not just a commutative count.
#[path = "support/issue_1467_order_reachability.rs"]
mod order_reachability;
#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use fsl_core::FslValue as V;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use support::{finish, full_fixture, full_observe, require, stable_full_packet};

const TEMPLATE: &str = "spec Order {\n  type K = 0..1\n  enum Color { Zed, Able }\n  struct P { z: Color, a: K }\n  state { c: Bool, n: K, o0: Option<K>, o1: Option<K>, oo: Option<Option<K>>, s: Seq<K, 2>, t: Seq<K, 2>, empty: Seq<K, 0>, r: relation Color -> Color, q: relation Color -> Color, m: Map<K, K>, z: Map<K, K> }\n  init { c = true n = 0 o0 = some(0) o1 = some(1) oo = some(some(0)) s = Seq {1} t = Seq {0, 1} empty = Seq {} r = Set {} q = Set {} forall k: K { m[k] = 0 z[k] = 0 } }\n  action go() { c = false n = 1 s = Seq {1} t = Seq {0,1} o0 = none o1 = none r = r.remove(Zed,Zed).remove(Zed,Able).remove(Able,Zed).remove(Able,Able).add(Zed,Able).add(Able,Zed) q = q.remove(Zed,Zed).remove(Zed,Able).remove(Able,Zed).remove(Able,Able).add(Able,Zed) forall k: K { m[k] = 1 z[k] = 0 } }\n  invariant Capture { $P }\n}\n";

fn exercise(label: &str, text: &str, outcome: &str, binding: Option<&Value>) -> Vec<String> {
    let mut errors = Vec::new();
    let mut previous = BTreeMap::new();
    for repeat in 1..=2 {
        let dir = full_fixture(label, text);
        std::fs::write(
            dir.join("oracle.txt"),
            format!("outcome={outcome}; binding={binding:?}; loc=8:3; step=1"),
        )
        .unwrap();
        for route in ["check", "monitor", "bmc", "induction", "explicit"] {
            let (r, exit) = full_observe(&dir, route);
            let positive = outcome == "pass";
            let property = if outcome == "partial_op" {
                "_partial_property_Capture"
            } else if outcome.starts_with("_bounds_") {
                outcome
            } else {
                "Capture"
            };
            let kind = if outcome.starts_with("_bounds_") {
                "type_bound"
            } else if outcome == "binding" {
                "invariant"
            } else {
                outcome
            };
            let locations = [json!({"line":8,"column":3}), Value::Null];
            let ok = if route == "check" {
                exit == 0 && r["result"] == "ok"
            } else if route == "monitor" && positive {
                exit == 0 && r["result"] == "conformant" && r["steps_checked"] == 1
            } else if route == "monitor" {
                exit == 1
                    && r["result"] == "nonconformant"
                    && r["violation"]["kind"] == kind
                    && r["violation"]["name"] == property
                    && r["failed_at_event"] == 0
            } else if positive {
                exit == 0
                    && r["result"] == if route == "bmc" { "verified" } else { "proved" }
                    && r["completeness"]
                        == if route == "bmc" {
                            "bounded"
                        } else {
                            "unbounded"
                        }
            } else {
                exit == 1
                    && r["result"] == "violated"
                    && r["violation_kind"] == kind
                    && r["invariant"] == property
                    && r["violated_at_step"] == 1
                    && r["loc"] == locations[usize::from(kind == "type_bound")]
            };
            println!("ORDER {label} {repeat} {route} exit={exit} expected={outcome} ok={ok}");
            require(&mut errors, ok, label, route, &r, exit);
            if route == "check" && !ok {
                break;
            }
            if let Some(expected) = binding {
                let actual = if route == "monitor" && positive {
                    Some(&r["final_state"])
                } else if !matches!(route, "check" | "monitor") && !positive {
                    Some(if let Some(variable) = outcome.strip_prefix("_bounds_") {
                        &r["trace"][1]["state"][variable]
                    } else {
                        &r["violating_bindings"][0]["x"]
                    })
                } else {
                    None
                };
                if let Some(actual) = actual {
                    require(
                        &mut errors,
                        actual == expected,
                        label,
                        "literal first failed binding",
                        &r,
                        exit,
                    );
                }
            }
            let stable = (stable_full_packet(r), exit);
            if let Some(prior) = previous.insert(route, stable.clone()) {
                assert_eq!(prior, stable, "full packet {label}/{route}");
            }
        }
    }
    errors
}

#[test]
fn recursive_order_matches_literal_first_observables() {
    // Each first-expression|JSON pair is an independent literal oracle.
    let rows = [
        ("enum-spelling", "Zed, Able", "Able|\"Able\"", "Zed|\"Zed\""),
        (
            "struct-field-order",
            "P {z: Able, a: 1}, P {z: Zed, a: 0}",
            "P {z: Zed, a: 0}|{\"a\":0,\"z\":\"Zed\"}",
            "P {z: Able, a: 1}|{\"a\":1,\"z\":\"Able\"}",
        ),
        (
            "absent-payload-dedup",
            "o0, o1, some(1)",
            "none|null",
            "none|null",
        ),
        (
            "nested-presence",
            "oo, some(none), none",
            "none|null",
            "oo|{\"kind\":\"some\",\"value\":0}",
        ),
        (
            "option-enum",
            "some(Zed), some(Able), none",
            "none|null",
            "some(Zed)|\"Zed\"",
        ),
        ("seq-live-prefix", "s, t", "t|[0,1]", "s|[1]"),
        (
            "set-live-prefix",
            "Set {1}, Set {0,1}, Set {0}",
            "Set {0}|[0]",
            "Set {1}|[1]",
        ),
        (
            "mixed-recursion",
            "Seq {Set {1}, Set {}}, Seq {Set {0}, Set {1}}",
            "Seq {Set {0}, Set {1}}|[[0],[1]]",
            "Seq {Set {1}, Set {}}|[[1],[]]",
        ),
        (
            "relation-prefix-view",
            "r, q, r",
            "q|[[\"Able\",\"Zed\"]]",
            "r|[[\"Able\",\"Zed\"],[\"Zed\",\"Able\"]]",
        ),
        (
            "map-key-order",
            "m, z",
            "z|{\"0\":0,\"1\":0}",
            "m|{\"0\":1,\"1\":1}",
        ),
    ];
    let mut errors = Vec::new();
    for (name, members, set, seq) in rows {
        for (outer, oracle) in [("Set", set), ("Seq", seq)] {
            let (first, expected) = oracle.split_once('|').unwrap();
            let expected: Value = serde_json::from_str(expected).unwrap();
            let domain = format!("{outer} {{{members}}}");
            for outcome in ["pass", "invariant", "partial_op", "binding"] {
                let body = match outcome {
                    "pass" => format!(
                        "exists x in {domain}: c or x == ({first}) or Seq {{1}}.pop().head() == 0"
                    ),
                    "invariant" => format!(
                        "forall x in {domain}: c or (x != ({first}) and Seq {{1}}.pop().head() == 0)"
                    ),
                    "binding" => format!("forall x in {domain}: c or x != ({first})"),
                    _ => format!(
                        "forall x in {domain}: c or (x == ({first}) and Seq {{1}}.pop().head() == 0)"
                    ),
                };
                let mut text = TEMPLATE.replace("$P", &body);
                if name == "absent-payload-dedup" && outer == "Seq" && outcome == "pass" {
                    // Independently proved by c0_auxiliary_is_inductive, not a changed Capture.
                    text = text.replace(
                        "\n}",
                        "\n  invariant ReachableAbsence { c or (o0 is none and o1 is none) }\n}",
                    );
                }
                text = order_reachability::order_positive(text, name, outer, outcome);
                errors.extend(exercise(
                    &format!("1467-order-{name}-{outer}-{outcome}"),
                    &text,
                    outcome,
                    (outcome == "binding").then_some(&expected),
                ));
            }
        }
    }
    finish(&errors);
}

#[test]
fn presence_multiplicity_live_lengths_and_relation_closure_remain_distinct() {
    let mut errors = Vec::new();
    for predicate in [
        "c or (Set {o0, o1}.size() == 1 and Seq {o0, o1}.size() == 2 and exactlyOne(x in Set {o0, o1} where x is none))",
        "c or (Set {oo, some(none), none}.size() == 3 and Seq {oo, some(none), none}.size() == 3)",
        "(if c then Seq {P {a:0,z:Zed}} else Seq {P {a:0,z:Zed}, P {a:1,z:Able}}).size() == 2 - (if c then 1 else 0)",
        "c or (Set {r, r.remove(Zed, Able).add(Zed, Able)}.size() == 1 and reachable(r, Able, Able) and not reachable(q, Zed, Able))",
        "c or (Set {Set {n + 3}, Set {n + 3}}.size() == 1 and Seq {P {a:n + 3,z:Zed}}.head().a == 4)",
        "c or (Set {empty.push(n + 3), empty}.size() == 2 and Seq {empty.push(n + 3)}.head().head() == 4)",
    ] {
        errors.extend(exercise(
            "1467-recursive-preservation",
            &order_reachability::preservation_positive(
                TEMPLATE.replace("$P", predicate),
                predicate,
            ),
            "pass",
            None,
        ));
        errors.extend(exercise(
            "1467-recursive-rejecting",
            &TEMPLATE.replace("$P", &format!("c or not ({predicate})")),
            "invariant",
            None,
        ));
    }
    for (effect, outcome, expected) in [
        ("ys = Set {n + 0}", "pass", r#"{"n":0,"ys":[0],"empty":[]}"#),
        ("ys = Set {n + 3}", "_bounds_ys", "[3]"),
        ("empty = empty.push(n)", "_bounds_empty", "[0]"),
    ] {
        let expected: Value = serde_json::from_str(expected).unwrap();
        let text = format!(
            "spec Bounds {{\n  type K = 0..1\n  type L = 0..1\n  state {{ n: K, ys: Set<K>, empty: Seq<K, 0> }}\n  init {{ n = 0 ys = Set {{0}} empty = Seq {{}} }}\n  // Int-derived temporaries retain members until target-context bounds.\n  action go() {{ {effect} }}\n  invariant Capture {{ ys.contains(n) }}\n}}\n"
        );
        errors.extend(exercise("1467-retag", &text, outcome, Some(&expected)));
    }
    finish(&errors);
}

fn ready<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    match std::pin::pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("native solver yielded"),
    }
}

#[test]
fn invalid_enum_ordinals_reach_ordering_at_existing_snapshot_boundary() {
    let text = "spec Invalid {\n  enum E { Zed, Able }\n  state { a: E, b: E }\n  init { a = Zed b = Able }\n  action go() {}\n  invariant Capture { forall x in Set {Able, a, b}: x != b and Seq {1}.pop().head() == 0 }\n}\n";
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(text, &fsl_core::FsResolver::new(".")).unwrap(),
    )
    .unwrap();
    let snapshot = BTreeMap::from([("a".into(), V::Int(-1)), ("b".into(), V::Int(-2))]);
    let mut errors = Vec::new();
    for _ in 0..2 {
        let dir = full_fixture("1467-existing-invalid-enum-boundary", text);
        let (checked, code) = full_observe(&dir, "check");
        assert_eq!((code, checked["result"].as_str()), (0, Some("ok")));
        let concrete = fsl_runtime::eval(
            &model.invariants[0].expr,
            &snapshot,
            &mut BTreeMap::new(),
            &model,
            None,
        )
        .unwrap();
        assert_eq!(
            concrete,
            V::Bool(false),
            "signed Int(-2) must precede invalid -1 and valid Enum"
        );
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let result = ready(fsl_verifier::verify_bounded_from_state(
            &model,
            &mut solver,
            1,
            Some(&BTreeSet::new()),
            &snapshot,
        ));
        std::fs::write(dir.join("snapshot-boundary.txt"), format!("snapshot={snapshot:?}\nexpected=invariant Capture step0, raw -1/-2\nconcrete={concrete:?}\nproduced={result:?}\n")).unwrap();
        let ok = result
            .as_ref()
            .ok()
            .and_then(|r| r.violation.as_ref())
            .is_some_and(|v| {
                v.kind == "invariant"
                    && v.name == "Capture"
                    && v.step == 0
                    && v.trace[0].state == snapshot
            });
        if !ok {
            errors.push(format!("invalid enum order produced={result:?}; expected invariant Capture with retained signed integers"));
        }
    }
    finish(&errors);
}

#[test]
fn public_bindings_root_export_and_variant_census_are_preserved() {
    use fsl_verifier::{Bindings, SymbolicValue as S};
    fn exact<T>(bindings: Bindings<T>) -> BTreeMap<String, S<T>> {
        bindings
    }
    fn census<T>(value: &S<T>) -> &'static str {
        match value {
            S::Scalar { .. } => "Scalar",
            S::None => "None",
            S::Option { .. } => "Option",
            S::Struct { .. } => "Struct",
            S::Map { .. } => "Map",
            S::Set { .. } => "Set",
            S::Seq { .. } => "Seq",
            S::Relation { .. } => "Relation",
            S::Collection { .. } => "Collection",
            S::SetLiteral(_) => "SetLiteral",
            S::SeqLiteral(_) => "SeqLiteral",
        }
    }
    let original = S::<bool>::None;
    let bindings = exact(BTreeMap::from([("x".into(), original)]));
    assert_eq!(census(&bindings["x"]), "None");
}

#[test]
fn breaking_collection_constructor_migration_is_not_semantic_red() {
    use fsl_core::TypeRef as T;
    use fsl_verifier::SymbolicValue as S;
    let mut statuses = Vec::new();
    for _ in 0..2 {
        let dir = full_fixture("1467-breaking-field-migration", "");
        let expected: Vec<(Vec<S<bool>>, bool)> = vec![(vec![S::None], true)];
        let value = S::<bool>::Collection {
            ty: T::Set(Box::new(T::Bool)),
            entries: Vec::new(),
        };
        let S::Collection { entries, .. } = value else {
            unreachable!()
        };
        let (actual, expected) = (
            std::any::type_name_of_val(&entries),
            std::any::type_name_of_val(&expected),
        );
        std::fs::write(
            dir.join("field-migration.txt"),
            format!("actual={actual}\nexpected={expected}\n"),
        )
        .unwrap();
        statuses.push(actual == expected);
    }
    assert!(
        statuses.iter().all(|ok| *ok),
        "PUBLIC API MIGRATION ONLY, never verifier semantic RED"
    );
}
