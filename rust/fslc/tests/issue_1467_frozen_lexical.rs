// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use support::{Oracle, continuation, finish};

const HEADER: &str = "spec Frozen {\n  type K = 0..1\n  type L = 0..1\n  enum Color { Zed, Able }\n  struct P { a: K, o: Option<Option<K>>, z: Color }\n  state { c: Bool, f: K, oo: Option<Option<K>>, r: relation K -> L, m: Map<K, K>, z: Map<K, K> }\n  init { c = true f = 1 oo = some(some(0)) r = Set {} forall k: K { m[k] = k z[k] = 0 } }\n  action go() { c = false }\n  invariant Capture { $P }\n}\n";

fn frozen_matrix(nested_absence: bool) -> Vec<String> {
    let mut errors = Vec::new();
    for (kind, zero, live) in [
        ("int", "0", "f"),
        ("bool", "false", "f == 1"),
        ("enum", "Zed", "if f == 1 then Able else Zed"),
        (
            "struct",
            "P {a:0,o:some(some(0)),z:Zed}",
            "P {a:f,o:some(some(f)),z:Able}",
        ),
        ("option", "some(0)", "some(f)"),
        (
            "option-enum",
            "some(Zed)",
            "some(if f == 1 then Able else Zed)",
        ),
        ("nested-absence", "oo", "if f == 0 then oo else some(none)"),
        ("set", "Set {0}", "Set {0,f}"),
        ("seq", "Seq {0,0}", "Seq {0,f}"),
        ("set-seq", "Set {Seq {0,0}}", "Set {Seq {0,f},Seq {0,0}}"),
        (
            "seq-set",
            "Seq {Set {0},Set {0}}",
            "Seq {Set {0,f},Set {0}}",
        ),
        ("relation", "r", "r.add(f,0)"),
        ("map", "z", "if f == 1 then m else z"),
    ] {
        if (kind == "nested-absence") != nested_absence {
            continue;
        }
        for outer in ["Set", "Seq"] {
            let a = format!("({zero})");
            let b = format!("({live})");
            let singleton = format!("{outer} {{{b}}}");
            let typed = if outer == "Set" {
                format!("{singleton}.add({b})")
            } else {
                format!("{singleton}.push({b}).pop()")
            };
            for consumer in ["Q", "C", "B"] {
                for reverse in [false, true] {
                    let positive = match consumer {
                        "Q" if reverse => format!("{typed} == {singleton}"),
                        "Q" => format!("{singleton} == {typed}"),
                        "C" => {
                            let arms = if reverse {
                                format!("if f == 0 then {outer} {{{a}}} else {typed}")
                            } else {
                                format!("if f == 1 then {typed} else {outer} {{{a}}}")
                            };
                            format!("({arms}) == {singleton}")
                        }
                        _ => {
                            let projection = if outer == "Seq" {
                                format!(
                                    "count(x in {outer} {{{a},{b}}} where x == {b}) == 1 and sum(x in {outer} {{{a},{b}}} of if x == {b} then 1 else 0) == 1"
                                )
                            } else {
                                format!(
                                    "count(k: K where k == f and {singleton}.contains({b})) == 1 and sum(k: K of k where k == f and {singleton}.contains({b})) == 1"
                                )
                            };
                            format!(
                                "(forall x in {outer} {{{a},{b}}}: x == {a} or x == {b}) and (exists x in {singleton}: x == {b}) and exactlyOne(x in {outer} {{{a},{b}}} where x == {b}) and {projection}"
                            )
                        }
                    };
                    for negative in [false, true] {
                        let body = if negative {
                            format!("not ({positive})")
                        } else {
                            positive.clone()
                        };
                        errors.extend(continuation(
                            &format!("1467-frozen-{kind}-{outer}-{consumer}-{reverse}-{negative}"),
                            &HEADER.replace("$P", &body),
                            &Oracle {
                                outcome: if negative { "invariant" } else { "pass" },
                                step: 0,
                                line: 9,
                                frozen: true,
                                witness: None,
                            },
                        ));
                    }
                }
            }
        }
    }
    errors
}

#[test]
fn frozen_q_c_b_cross_admitted_kinds_and_both_outers() {
    finish(&frozen_matrix(false));
}

#[test]
fn frozen_nested_absence_q_c_b_keep_typed_conditional_context() {
    finish(&frozen_matrix(true));
}

#[test]
fn nonconstant_projection_totals_filters_and_lexical_statements() {
    let mut errors = Vec::new();
    let header = "spec Lexical {\n  type K = 0..2\n  struct P { a: K, o: Option<K> }\n  state { c: Bool, n: K, total: K, saved: K, m: Map<K,K> }\n  init { c = true n = 0 total = 0 saved = 0 forall k: K { m[k] = 0 } }\n  action go() { $E }\n  invariant Capture { $P }\n}\n";
    for (name, expression, expected) in [
        (
            "fields",
            "sum(x in Seq {P {a:n,o:some(n)}, P {a:1,o:none}} of x.a)",
            "n + 1",
        ),
        (
            "payloads",
            "sum(k: K of k where Seq {some(n), none}.contains(some(k)))",
            "n",
        ),
        (
            "filter",
            "sum(x in Seq {P {a:n,o:none}, P {a:1,o:none}} where x.a != 0 of x.a)",
            "n + 1",
        ),
        (
            "empty",
            "sum(x in Seq {P {a:n,o:none}} where false of x.a)",
            "0",
        ),
    ] {
        for negative in [false, true] {
            let predicate = format!(
                "c or ({expression}) == ({expected}){}",
                if negative { " + 1" } else { "" }
            );
            errors.extend(continuation(
                &format!("1467-projection-{name}-{negative}"),
                &header
                    .replace("$E", "c = false n = 1")
                    .replace("$P", &predicate),
                &Oracle {
                    outcome: if negative { "invariant" } else { "pass" },
                    step: 1,
                    line: 7,
                    frozen: false,
                    witness: None,
                },
            ));
        }
    }
    for (name, effect, predicate, outcome) in [
        (
            "capture-shadow-old",
            "let x = P {a:n,o:some(n)} let hold = Seq {x} let n = 2 saved = hold.head().a total = 1 c = false ensures Seq {P {a:old(saved),o:none}}.head().a == saved",
            "n == 0 and saved == 0 and (c or total == 1)",
            "pass",
        ),
        (
            "capture-reject",
            "let x = P {a:n,o:some(n)} let hold = Seq {x} let n = 2 saved = hold.head().a total = 1 c = false ensures Seq {P {a:old(saved),o:none}}.head().a == saved",
            "n == 0 and (c or saved == 2)",
            "invariant",
        ),
        (
            "statement-forall",
            "forall k: K { m[k] = Seq {P {a:k,o:some(k)}}.head().a } c = false",
            "c or m[1] == 1",
            "pass",
        ),
        (
            "statement-reject",
            "forall k: K { m[k] = Seq {P {a:k,o:some(k)}}.head().a } c = false",
            "c or m[1] == 0",
            "invariant",
        ),
        (
            "unreached",
            "requires (exists x in Seq {P {a:n,o:none}}: x.a == n or Seq {1}.pop().head() == 0) c = false",
            "c or (exists x in Seq {P {a:0,o:none}}: x.a == 0 or Seq {1}.pop().head() == 0)",
            "pass",
        ),
        (
            "reached",
            "c = false",
            "c or (forall x in Seq {P {a:0,o:none}}: x.a == 0 and Seq {1}.pop().head() == 0)",
            "partial_op",
        ),
    ] {
        errors.extend(continuation(
            &format!("1467-lexical-{name}"),
            &header.replace("$E", effect).replace("$P", predicate),
            &Oracle {
                outcome,
                step: 1,
                line: 7,
                frozen: false,
                witness: None,
            },
        ));
    }
    finish(&errors);
}
