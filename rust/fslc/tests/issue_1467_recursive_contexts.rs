// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use serde_json::json;
use support::{Oracle, continuation, finish};

#[test]
fn c0_auxiliary_is_inductive() {
    let source = "spec Absence {\n  type K = 0..1\n  state { c: Bool, o0: Option<K>, o1: Option<K> }\n  init { c = true o0 = some(0) o1 = some(1) }\n  action go() { c = false o0 = none o1 = none }\n  invariant Capture { c or (o0 is none and o1 is none) }\n}\n";
    finish(&continuation(
        "1467-c0-auxiliary",
        source,
        &Oracle {
            outcome: "pass",
            step: 1,
            line: 6,
            frozen: false,
            witness: None,
        },
    ));
}

#[test]
fn recursive_target_context_retains_outside_support_until_bounds() {
    let mut errors = Vec::new();
    for branch in [true, false] {
        for (ty, initial, value, good, bad) in [
            (
                "Set<L>",
                "Set {}",
                "Seq {Set {n + $}}.head()",
                json!([0]),
                json!([3]),
            ),
            (
                "Seq<L,2>",
                "Seq {}",
                "Seq {Seq {n + $}}.head()",
                json!([0]),
                json!([3]),
            ),
            (
                "P",
                "P {a:0}",
                "Seq {P {a: n + $}}.head()",
                json!({"a":0}),
                json!({"a":3}),
            ),
            (
                "Option<K>",
                "none",
                "Seq {some(n + $), none}.head()",
                json!(0),
                json!(3),
            ),
            (
                "Option<Option<K>>",
                "some(none)",
                "Seq {some(some(n + $))}.head()",
                json!({"kind":"some","value":0}),
                json!({"kind":"some","value":3}),
            ),
        ] {
            for (offset, expected) in [(0, good), (3, bad)] {
                let value = value.replace('$', &offset.to_string());
                let selected = if branch {
                    format!("if b then {value} else ys")
                } else {
                    format!("if b then ys else {value}")
                };
                let text = format!(
                    "spec Target {{\n  type K = 0..1\n  type L = 0..1\n  struct P {{ a: K }}\n  state {{ n: K, b: Bool, ys: {ty} }}\n  init {{ n = 0 b = {branch} ys = {initial} }}\n  action go() {{ ys = {selected} }}\n  invariant Capture {{ n == 0 }}\n}}\n"
                );
                errors.extend(continuation(
                    &format!("1467-target-{ty}-{branch}-{offset}"),
                    &text,
                    &Oracle {
                        outcome: if offset == 0 { "pass" } else { "_bounds_ys" },
                        step: 1,
                        line: 8,
                        frozen: false,
                        witness: Some(("ys", expected)),
                    },
                ));
            }
        }
    }
    finish(&errors);
}

#[test]
fn recursive_contextual_none_and_nominal_identity() {
    let header = "spec Nominal {\n  type K = 0..1\n  enum E { Zed, Able }\n  enum F { ZedF, AbleF }\n  struct P { a: K, e: E }\n  struct Q { a: K, e: E }\n  state { c: Bool, o: Option<Option<K>> }\n  init { c = true o = some(none) }\n  action go() { c = false }\n  invariant Capture { $P }\n}\n";
    let mut errors = Vec::new();
    for outer in ["Set", "Seq"] {
        for reverse in [false, true] {
            let (left, right) = if reverse {
                (
                    format!("{outer} {{o,o}}"),
                    format!("{outer} {{o,some(none)}}"),
                )
            } else {
                (
                    format!("{outer} {{o,some(none)}}"),
                    format!("{outer} {{o,o}}"),
                )
            };
            for negative in [false, true] {
                let predicate = format!(
                    "c or {}({left} == {right})",
                    if negative { "not " } else { "" }
                );
                errors.extend(continuation(
                    "1467-recursive-none-context",
                    &header.replace("$P", &predicate),
                    &Oracle {
                        outcome: if negative { "invariant" } else { "pass" },
                        step: 1,
                        line: 10,
                        frozen: false,
                        witness: None,
                    },
                ));
            }
        }
        for (left, right) in [("P {a:0,e:Zed}", "Q {a:0,e:Zed}"), ("Zed", "ZedF")] {
            let text = header.replace("$P", &format!("{outer} {{{left}}} == {outer} {{{right}}}"));
            let dir = support::full_fixture("1467-nominal-refusal", &text);
            for _ in 0..2 {
                let (r, exit) = support::full_observe(&dir, "check");
                assert_eq!(exit, 2, "different nominal identities must not unify: {r}");
                assert_eq!(r["result"], "error");
                assert_eq!(r["kind"], "semantics");
                assert_eq!(r["loc"], json!({"line":10,"column":3}));
                let (from, to) = if left.starts_with('P') {
                    ("Q", "P")
                } else {
                    ("F", "E")
                };
                assert_eq!(
                    r["message"],
                    format!(
                        "invalid model expression: expression of type Named(\"{from}\") is not assignable to Named(\"{to}\") at 10:3"
                    )
                );
            }
        }
    }
    finish(&errors);
}

#[test]
fn empty_opposites_contextual_none_aliases_and_live_lengths() {
    let mut errors = Vec::new();
    let header = "spec Context {\n  type K = 0..1\n  type L = 0..1\n  enum E { Zed, Able }\n  enum F { ZedF, AbleF }\n  struct P { a: K, e: E }\n  struct Q { a: K, e: E }\n  state { b: Bool, n: K, k: Seq<K, 2>, l: Seq<L, 2>, o: Option<Option<K>> }\n  init { b = true n = 0 k = Seq {0} l = Seq {0} o = some(none) }\n  action go() { b = false n = 1 }\n  invariant Capture { $P }\n}\n";
    for outer in ["Set", "Seq"] {
        for item in [
            "P {a:n,e:Able}",
            "some(none)",
            "Seq {n}",
            "Set {n}",
            "some(Able)",
        ] {
            let typed = if item == "some(none)" { "o" } else { item };
            let other = match item {
                "P {a:n,e:Able}" => "P {a:0,e:Zed}",
                "Seq {n}" => "Seq {}",
                "Set {n}" => "Set {}",
                _ => "none",
            };
            let live = format!("{outer} {{{typed}}}");
            let computed = if outer == "Seq" {
                format!("{live}.pop()")
            } else {
                format!("{live}.remove({typed})")
            };
            for reverse in [false, true] {
                // Typed computed empties keep both public Kernel operand orders admitted.
                let (left, right) = if reverse {
                    (&computed, &live)
                } else {
                    (&live, &computed)
                };
                let join = if reverse {
                    format!("if b then {live} else {computed}")
                } else {
                    format!("if not b then {computed} else {live}")
                };
                let (ql, qr) = if reverse {
                    (&computed, &join)
                } else {
                    (&join, &computed)
                };
                let equality = format!(
                    "({left}).size() + ({right}).size() == 1 and {computed} == {outer} {{}} and (b or (({ql}) == ({qr})))"
                );
                let uneven = format!(
                    "(if b then {live} else {outer} {{{typed}, ({other})}}).size() == (if b then 1 else 2)"
                );
                for negative in [false, true] {
                    let predicate = format!(
                        "({equality}) and ({join}).size() == (if b then 1 else 0) and {uneven} and k == l"
                    );
                    let predicate = if negative {
                        format!("b or not ({predicate})")
                    } else {
                        predicate
                    };
                    errors.extend(continuation(
                        &format!("1467-context-{outer}-{item}-{reverse}-{negative}"),
                        &header.replace("$P", &predicate),
                        &Oracle {
                            outcome: if negative { "invariant" } else { "pass" },
                            step: 1,
                            line: 11,
                            frozen: false,
                            witness: None,
                        },
                    ));
                }
            }
        }
    }
    finish(&errors);
}
