// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use support::{Oracle, continuation, finish, full_fixture, full_observe, stable_full_packet};

#[test]
fn admitted_map_member_reads_continue_through_existing_index() {
    let mut errors = Vec::new();
    for read in ["Seq {m,z}.head()", "Seq {z,m}.at(1)", "Seq {m,z}[0]"] {
        for negative in [false, true] {
            let text = format!(
                "spec MapRead {{\n  type K = 0..1\n  state {{ c: Bool, m: Map<K,K>, z: Map<K,K> }}\n  init {{ c = true forall k: K {{ m[k] = 0 z[k] = 0 }} }}\n  action go() {{ c = false m[0] = 1 }}\n  invariant Capture {{ c or ({read})[0] == {} }}\n}}\n",
                usize::from(!negative)
            );
            errors.extend(continuation(
                &format!("1467-map-read-{read}-{negative}"),
                &text,
                &Oracle {
                    outcome: if negative { "invariant" } else { "pass" },
                    step: 1,
                    line: 6,
                    frozen: false,
                    witness: None,
                },
            ));
        }
    }
    finish(&errors);
}

#[test]
fn masked_absence_ignores_independent_hidden_payloads() {
    // Init constrains presence, not either absent scalar payload. A symbolic
    // payload leak can choose different values; the concrete oracle remains None.
    let mut errors = Vec::new();
    for negative in [false, true] {
        let text = format!(
            "spec Masked {{\n  type K = 0..1\n  state {{ c: Bool, a: Option<K>, b: Option<K>, oo: Option<Option<K>> }}\n  init {{ c = true a = none b = none oo = some(none) }}\n  action go() {{ c = false }}\n  invariant Capture {{ {}(Set {{a,b}}.size() == 1 and Seq {{a,b}}.size() == 2 and Set {{oo,none,some(some(0))}}.size() == 3) }}\n}}\n",
            if negative { "not " } else { "" }
        );
        errors.extend(continuation(
            "1467-masked-independent-payloads",
            &text,
            &Oracle {
                outcome: if negative { "invariant" } else { "pass" },
                step: 0,
                line: 6,
                frozen: true,
                witness: Some(("oo", json!({"kind":"some","value":null}))),
            },
        ));
    }
    finish(&errors);
}

#[test]
fn masked_payload_count_is_an_observable_literal_state_value() {
    let mut errors = Vec::new();
    for negative in [false, true] {
        let mut text = format!(
            "spec VisibleCount {{\n  type K = 0..1 type Count = 0..2\n  state {{ c: Bool, a: Option<K>, b: Option<K>, total: Count }}\n  init {{ c = true a = none b = none total = 0 }}\n  action go() {{ c = false total = Set {{a,b}}.size() }}\n  invariant Capture {{ c or total == {} }}\n}}\n",
            usize::from(!negative)
        );
        if !negative {
            let predicate = "a is none and b is none";
            let alone = text.replace(
                "invariant Capture { c or total == 1 }",
                &format!("invariant ReachableAbsentInputs {{ {predicate} }}"),
            );
            assert!(!alone.contains("Capture"));
            finish(&continuation(
                "1467-aux-absent-inputs-alone",
                &alone,
                &Oracle {
                    outcome: "pass",
                    step: 1,
                    line: 6,
                    frozen: false,
                    witness: None,
                },
            ));
            let rejecting = alone
                .replace("b = none", "b = some(0)")
                .replace("ReachableAbsentInputs", "Capture");
            finish(&continuation(
                "1467-aux-absent-inputs-reject",
                &rejecting,
                &Oracle {
                    outcome: "invariant",
                    step: 0,
                    line: 6,
                    frozen: false,
                    witness: None,
                },
            ));
            text = text.replace(
                "\n}",
                &format!("\n  invariant ReachableAbsentInputs {{ {predicate} }}\n}}"),
            );
        }
        errors.extend(continuation(
            "1467-visible-masked-count",
            &text,
            &Oracle {
                outcome: if negative { "invariant" } else { "pass" },
                step: 1,
                line: 6,
                frozen: false,
                witness: Some(("total", json!(1))),
            },
        ));
    }
    finish(&errors);
}

#[test]
fn nested_absence_bindings_are_literal_json_not_debug_values() {
    let mut errors = Vec::new();
    for (members, target, expected) in [
        (
            "oo, some(none), none",
            "some(none)",
            json!({"kind":"some","value":null}),
        ),
        ("oo, some(none), none", "none", Value::Null),
        (
            "oo, some(none), none",
            "oo",
            json!({"kind":"some","value":0}),
        ),
        ("o0, o1, some(1)", "none", Value::Null),
        (
            "W {a:o0,b:o1,o:ao}",
            "W {a:o0,b:o1,o:ao}",
            json!({"a":null,"b":null,"o":{"kind":"some","value":null}}),
        ),
    ] {
        for outer in ["Set", "Seq"] {
            let initial = if members.starts_with('W') {
                "o0 = none o1 = none"
            } else {
                "o0 = some(0) o1 = some(1)"
            };
            let text = format!(
                "spec Binding {{\n  type K = 0..1 struct W {{ a: Option<K>, b: Option<K>, o: Option<Option<K>> }}\n  state {{ c: Bool, oo: Option<Option<K>>, ao: Option<Option<K>>, o0: Option<K>, o1: Option<K> }}\n  init {{ c = true oo = some(some(0)) ao = some(none) {initial} }}\n  action go() {{ c = false o0 = none o1 = none }}\n  invariant Capture {{ forall x in {outer} {{{members}}}: c or x != ({target}) }}\n}}\n"
            );
            let dir = full_fixture("1467-nested-json-binding", &text);
            let mut previous = BTreeMap::new();
            for _ in 0..2 {
                for route in ["check", "bmc", "induction", "explicit"] {
                    let (r, exit) = full_observe(&dir, route);
                    let ok = if route == "check" {
                        exit == 0 && r["result"] == "ok"
                    } else {
                        exit == 1
                            && r["result"] == "violated"
                            && r["invariant"] == "Capture"
                            && r["violation_kind"] == "invariant"
                            && r["violated_at_step"] == 1
                            && r["loc"] == json!({"line":6,"column":3})
                            && r["violating_bindings"][0]["x"] == expected
                    };
                    support::require(&mut errors, ok, members, route, &r, exit);
                    let packet = (stable_full_packet(r), exit);
                    if let Some(before) = previous.insert(route, packet.clone()) {
                        assert_eq!(before, packet);
                    }
                }
            }
            std::fs::write(dir.join("expected-binding.json"), expected.to_string()).unwrap();
        }
    }
    finish(&errors);
}

#[test]
fn independent_solver_free_bfs_rejects_recursive_frozen_cells() {
    let mut errors = Vec::new();
    for (zero, one) in [
        ("0", "n"),
        ("false", "n == 1"),
        ("Zed", "Able"),
        ("P {a:0}", "P {a:n}"),
        ("some(0)", "some(n)"),
        ("some(Zed)", "some(Able)"),
        ("oo", "some(none)"),
        ("Set {0}", "Set {0,n}"),
        ("Seq {0,0}", "Seq {0,n}"),
        ("Set {Seq {0,0}}", "Set {Seq {0,n}}"),
        ("Seq {Set {0}}", "Seq {Set {0,n}}"),
        ("r", "r.add(n,0)"),
        ("z", "m"),
    ] {
        for outer in ["Set", "Seq"] {
            for consumer in ["Q", "C", "B"] {
                let a = format!("({zero})");
                let b = format!("({one})");
                let predicate = match consumer {
                    "Q" => format!("{outer} {{{a}}} == {outer} {{{b}}}"),
                    "C" => format!(
                        "(if n == 1 then {outer} {{{a}}} else {outer} {{{b}}}) == {outer} {{{b}}}"
                    ),
                    _ => format!("forall x in {outer} {{{a},{b}}}: x == {a}"),
                };
                let text = format!(
                    "spec Bfs {{\n  type K = 0..1\n  enum E {{ Zed, Able }}\n  struct P {{ a: K }}\n  state {{ c: Bool, n: K, oo: Option<Option<K>>, r: relation K -> K, m: Map<K,K>, z: Map<K,K> }}\n  init {{ c = true n = 1 oo = some(some(0)) r = Set {{}} forall k: K {{ m[k] = 1 z[k] = 0 }} }}\n  action go() {{ c = false }}\n  invariant Capture {{ {predicate} }}\n}}\n"
                );
                let dir = full_fixture("1467-bfs-reject", &text);
                for repeat in 0..2 {
                    let (admission, exit) = full_observe(&dir, "check");
                    assert_eq!((exit, &admission["result"]), (0, &json!("ok")));
                    let model = fsl_core::build_model(
                        fsl_core::parse_kernel_source(&text, &fsl_core::FsResolver::new("."))
                            .unwrap(),
                    )
                    .unwrap();
                    let result = fsl_runtime::verify_explicit(model, 4, 100).unwrap();
                    let ok = result.violation.as_ref().is_some_and(|v| {
                        v.violation.kind == "invariant"
                            && v.violation.name == "Capture"
                            && v.violation.step == 0
                    });
                    std::fs::write(
                        dir.join(format!("bfs-{repeat}.txt")),
                        format!("expected=invariant Capture step0\nproduced={result:#?}"),
                    )
                    .unwrap();
                    if !ok {
                        errors.push(format!("BFS {outer}/{consumer}/{one}: {result:?}"));
                    }
                }
            }
        }
    }
    finish(&errors);
}

#[test]
fn serialized_recursive_state_roundtrip_rejects_presence_corruption() {
    let text = "spec Packet {\n  type K = 0..1\n  struct P { a: K, o: Option<Option<K>> }\n  state { c: Bool, p: P, m: Map<K,P>, absent: Option<Option<K>> }\n  init { c = true p = P {a:0,o:some(some(0))} absent = none forall k: K { m[k] = P {a:k,o:some(none)} } }\n  action go() { c = false p = P {a:1,o:some(none)} }\n  invariant Capture { c or p.o == some(none) }\n}\n";
    let expected = json!({"c":false,"p":{"a":1,"o":{"kind":"some","value":null}},"m":{"0":{"a":0,"o":{"kind":"some","value":null}},"1":{"a":1,"o":{"kind":"some","value":null}}},"absent":null});
    let dir = full_fixture("1467-serialized-roundtrip", text);
    let mut errors = continuation(
        "1467-recursive-state-packet",
        text,
        &Oracle {
            outcome: "pass",
            step: 1,
            line: 7,
            frozen: false,
            witness: Some(("p", expected["p"].clone())),
        },
    );
    for corrupt in ["clean", "none", "some-some", "map-none"] {
        let mut state = expected.clone();
        match corrupt {
            "none" => state["p"]["o"] = Value::Null,
            "some-some" => state["p"]["o"] = json!({"kind":"some","value":0}),
            "map-none" => state["m"]["0"]["o"] = Value::Null,
            _ => {}
        }
        let initial = json!({"c":true,"p":{"a":0,"o":{"kind":"some","value":0}},"m":{"0":{"a":0,"o":{"kind":"some","value":null}},"1":{"a":1,"o":{"kind":"some","value":null}}},"absent":null});
        let trace = json!({"$schema":fsl_core::REPLAY_TRACE_V1_SCHEMA_ID,
            "schema_version":fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION,
            "kernel_schema_version":fsl_core::KERNEL_V2_SCHEMA_VERSION,
            "spec":"Packet", "initial":initial,
            "events":[{"tick":1,"action":"go","params":{},"state":state}]});
        let bytes = serde_json::to_vec(&trace).unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, trace);
        assert!(fslc_rust::replay_trace::parse_replay_trace(parsed).is_ok());
        std::fs::write(dir.join("actions.json"), bytes).unwrap();
        let mut previous = None;
        for _ in 0..2 {
            let (r, exit) = full_observe(&dir, "monitor");
            let ok = if corrupt == "clean" {
                exit == 0 && r["result"] == "conformant" && r["final_state"] == expected
            } else {
                exit == 1
                    && r["result"] == "nonconformant"
                    && r["violation"]["kind"] == "state_mismatch"
                    && r["failed_at_event"] == 0
                    && r["violation"]["expected_state"] == expected
                    && r["violation"]["observed_state"] == state
            };
            support::require(&mut errors, ok, "recursive roundtrip", "monitor", &r, exit);
            if let Some(before) = previous.replace((r.clone(), exit)) {
                assert_eq!(before, (r, exit));
            }
        }
    }
    finish(&errors);
}
