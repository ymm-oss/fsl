// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use fsl_core::FslValue as V;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use support::{Oracle, continuation, finish, full_fixture, full_observe, ready};

#[test]
fn outside_relation_endpoints_survive_recursive_reads_and_storage_bounds() {
    let text = "spec Outside {\n  type K = 0..1\n  type L = 0..1\n  state { c: Bool, r: relation K -> K, q: relation L -> L }\n  init { c = true r = Set {} q = Set {} }\n  action go() { c = false r = Seq {q.add(3,0).add(0,3)}.head() }\n  invariant Capture { c or reachable(r,3,3) }\n}\n";
    finish(&continuation(
        "1467-outside-relation-retained",
        text,
        &Oracle {
            outcome: "_bounds_r",
            step: 1,
            line: 7,
            frozen: false,
            witness: Some(("r", json!([[0, 3], [3, 0]]))),
        },
    ));
}

#[test]
fn outside_relation_order_has_literal_prefix_and_closure_witnesses() {
    let mut errors = Vec::new();
    for (outer, first, expected) in [
        ("Set", "q", json!([[0, 3]])),
        ("Seq", "r", json!([[0, 3], [3, 0]])),
    ] {
        let text = format!(
            "spec RelationOrder {{\n  type K = 0..1\n  type L = 0..1\n  state {{ r: relation K -> K, q: relation L -> L }}\n  init {{ r = Set {{}} q = Set {{}} }}\n  action go() {{ r = r.remove(3,0).add(3,0) }}\n  invariant Capture {{ forall x in {outer} {{r,q,r}}: x != {first} and Seq {{1}}.pop().head() == 0 }}\n  invariant Closure {{ reachable(r,3,3) and not reachable(q,3,3) }}\n}}\n"
        );
        let model = fsl_core::build_model(
            fsl_core::parse_kernel_source(&text, &fsl_core::FsResolver::new(".")).unwrap(),
        )
        .unwrap();
        let pair = |a, b| (V::Int(a), V::Int(b));
        let snapshot = BTreeMap::from([
            (
                "r".into(),
                V::Relation(BTreeSet::from([pair(0, 3), pair(3, 0)])),
            ),
            ("q".into(), V::Relation(BTreeSet::from([pair(0, 3)]))),
        ]);
        let dir = full_fixture("1467-outside-relation-order", &text);
        for repeat in 0..2 {
            let (r, exit) = full_observe(&dir, "check");
            assert_eq!((exit, &r["result"]), (0, &json!("ok")));
            assert_eq!(
                fsl_runtime::eval(
                    &model.invariants[0].expr,
                    &snapshot,
                    &mut BTreeMap::new(),
                    &model,
                    None
                )
                .unwrap(),
                V::Bool(false)
            );
            assert_eq!(
                fsl_runtime::eval(
                    &model.invariants[1].expr,
                    &snapshot,
                    &mut BTreeMap::new(),
                    &model,
                    None
                )
                .unwrap(),
                V::Bool(true)
            );
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
            let result = ready(fsl_verifier::verify_bounded_from_state(
                &model,
                &mut solver,
                1,
                Some(&BTreeSet::new()),
                &snapshot,
            ));
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
            std::fs::write(
                dir.join(format!("boundary-{repeat}.txt")),
                format!("expected=invariant Capture step0 first={expected}\nproduced={result:#?}"),
            )
            .unwrap();
            if !ok {
                errors.push(format!(
                    "outside {outer} expected first={expected} got {result:?}"
                ));
            }
        }
    }
    finish(&errors);
}

#[test]
fn signed_positive_invalid_and_valid_enums_use_existing_snapshot_api() {
    let mut errors = Vec::new();
    // Value variant Int precedes Enum; invalid Int order stays signed, not rank0.
    for (a, b, first) in [(-1, -2, "b"), (-2, -1, "a"), (3, 2, "b"), (2, 3, "a")] {
        for outcome in ["invariant", "partial_op"] {
            let body = if outcome == "invariant" {
                format!(
                    "forall x in Set {{Able,a,b}}: x != {first} and Seq {{1}}.pop().head() == 0"
                )
            } else {
                format!(
                    "forall x in Set {{Able,a,b}}: x == {first} and Seq {{1}}.pop().head() == 0"
                )
            };
            let text = format!(
                "spec Invalid {{\n  enum E {{ Zed, Able }}\n  state {{ a: E, b: E }}\n  init {{ a = Zed b = Able }}\n  action go() {{}}\n  invariant Capture {{ {body} }}\n}}\n"
            );
            let model = fsl_core::build_model(
                fsl_core::parse_kernel_source(&text, &fsl_core::FsResolver::new(".")).unwrap(),
            )
            .unwrap();
            let snapshot = BTreeMap::from([("a".into(), V::Int(a)), ("b".into(), V::Int(b))]);
            let dir = full_fixture("1467-invalid-enum-extension", &text);
            for repeat in 0..2 {
                let (r, exit) = full_observe(&dir, "check");
                assert_eq!((exit, &r["result"]), (0, &json!("ok")));
                let concrete = fsl_runtime::eval(
                    &model.invariants[0].expr,
                    &snapshot,
                    &mut BTreeMap::new(),
                    &model,
                    None,
                );
                if outcome == "invariant" {
                    assert_eq!(concrete.as_ref().unwrap(), &V::Bool(false));
                } else {
                    assert!(
                        concrete.is_err(),
                        "first invalid must reach the partial operation"
                    );
                }
                let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
                let result = ready(fsl_verifier::verify_bounded_from_state(
                    &model,
                    &mut solver,
                    1,
                    Some(&BTreeSet::new()),
                    &snapshot,
                ));
                let property = if outcome == "invariant" {
                    "Capture"
                } else {
                    "_partial_property_Capture"
                };
                let ok = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.violation.as_ref())
                    .is_some_and(|v| {
                        v.kind == outcome
                            && v.name == property
                            && v.step == 0
                            && v.trace[0].state == snapshot
                    });
                std::fs::write(dir.join(format!("snapshot-{repeat}.txt")), format!("expected={outcome} {property} step0 raw={snapshot:?}\nconcrete={concrete:?}\nproduced={result:#?}")).unwrap();
                if !ok {
                    errors.push(format!(
                        "a={a},b={b},first={first}: expected {outcome}, got {result:?}"
                    ));
                }
            }
        }
    }
    finish(&errors);
}
