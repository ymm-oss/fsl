// SPDX-License-Identifier: Apache-2.0

#[path = "support/collection_replay.rs"]
mod collection_replay;

use collection_replay::{ReplayCase, finish, require};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn exercise(label: &str, predicate: &str, effect: &str, outcome: &str) {
    let source = format!(
        "spec Edges {{\n  type K = 0..2\n  enum Color {{ Zed, Able }}\n  struct P {{ a: K }}\n  state {{ n: K, c: Bool, xs: Set<K>, ys: Seq<K, 2>, empty: Seq<K, 0>, out: Int }}\n  init {{ n = 0 c = true xs = Set {{ 1 }} ys = Seq {{ 1 }} empty = Seq {{}} out = 0 }}\n  action go() {{ n = 1 c = false {effect} }}\n  invariant Capture {{ {predicate} }}\n}}"
    );
    let partial = matches!(outcome, "head" | "pop" | "at");
    if partial {
        let kernel =
            fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(".")).unwrap();
        let model = fsl_core::build_model(kernel).unwrap();
        let state = BTreeMap::from([("c".to_owned(), fsl_core::FslValue::Bool(false))]);
        let error = fsl_runtime::eval(
            &model.invariants[0].expr,
            &state,
            &mut BTreeMap::new(),
            &model,
            None,
        )
        .unwrap_err();
        assert_eq!(error.partial_operation.unwrap().name(), outcome);
    }
    let name = "_partial_property_Capture";
    let mut errors = Vec::new();
    for _ in 0..2 {
        let mut case = ReplayCase::new(label, &source, &["go"]);
        if !partial {
            if outcome == "invariant" {
                case = case.with_expected_property("Capture", None);
            }
            let bound = outcome.starts_with("_bounds_").then_some(outcome);
            case.agreement(4, outcome == "pass", bound, &mut errors);
            continue;
        }
        let mut check = |condition, site, report: &Value, exit| {
            require(&mut errors, condition, label, site, report, exit);
        };
        for route in ["check", "bmc", "induction", "explicit", "monitor"] {
            let (report, exit) = case.observe(route, 4);
            let (actual, expected) = if route == "check" {
                (json!([exit, report["result"]]), json!([0, "ok"]))
            } else if route == "monitor" {
                (
                    json!([
                        exit,
                        report["result"],
                        report["violation"]["kind"],
                        report["violation"]["name"],
                        report["failed_at_event"]
                    ]),
                    json!([1, "nonconformant", "partial_op", name, 0]),
                )
            } else {
                (
                    json!([
                        exit,
                        report["result"],
                        report["violation_kind"],
                        report["invariant"],
                        report["violated_at_step"]
                    ]),
                    json!([1, "violated", "partial_op", name, 1]),
                )
            };
            println!("EDGE {label} {route} produced={actual} expected={expected}");
            check(actual == expected, route, &report, exit);
            if !matches!(route, "check" | "monitor") {
                let property = source.lines().nth(7).unwrap();
                assert!(property.starts_with("  invariant Capture"));
                check(
                    report["loc"] == json!({"line":8,"column":3}),
                    "partial location",
                    &report,
                    exit,
                );
            }
        }
    }
    finish(&errors);
}

#[test]
fn contextual_and_computed_values_preserve_their_meaning() {
    for (index, predicate) in [
        "Set {} != Set {1}",
        "Set {1} != Set {}",
        "Seq {} != Seq {1}",
        "Seq {1} != Seq {}",
        "Set {1} == xs",
        "xs == Set {1}",
        "Seq {1} == ys",
        "ys == Seq {1}",
        "Set {n, n}.size() == 1",
        "Seq {n, n}.size() == 2 and Seq {n, n}.at(1) == n",
        "Seq {n, 2}.pop().head() == 2",
        "(if c then ys else (if n == 1 then Seq {2} else Seq {1})).size() == 1",
        "(if c then ys else Seq {2}).size() == 0 or (if c then ys else Seq {2}).pop().push(1).size() == (if c then ys else Seq {2}).size()",
        "if true then true else Seq {1}.pop().head() == 0",
        "c or (exists color in Set {Zed, Able}: color == Able or Seq {1}.pop().head() == 0)",
        "empty.size() == 0 and empty.push(n + 3).head() == n + 3",
        "forall x in Set {n + 3}: Set {x}.contains(x)",
        "forall p in Seq {P {a: 0}, P {a: n}}: p.a >= 0",
        "forall o in Seq {some(0), some(n)}: o is some(v) and v >= 0",
        "c or some(n) == (if c then none else some(n))",
        "c or (if c then none else some(n)) == some(n)",
    ]
    .into_iter()
    .enumerate()
    {
        exercise(&format!("positive-{index}"), predicate, "", "pass");
    }
    exercise(
        "local-seq",
        "out >= 0",
        "let local = Seq {1, n} let other = Set {1, n} ys = local xs = other out = local.at(1)",
        "pass",
    );
    exercise(
        "local-nested-if",
        "ys.size() <= 2",
        "let local = if c then (if n == 0 then Seq {1} else Seq {2}) else ys ys = local",
        "pass",
    );
}

#[test]
fn rejecting_controls_pin_partiality_bounds_and_composite_preservation() {
    for (index, (predicate, operation)) in [
        ("c or Seq {1}.pop().head() == 0", "head"),
        ("c or Seq {1}.pop().pop().size() == 0", "pop"),
        ("c or Seq {1}.at(0 - 1) == 0", "at"),
        ("c or Seq {1}.at(1) == 0", "at"),
    ]
    .into_iter()
    .enumerate()
    {
        exercise(&format!("partial-{index}"), predicate, "", operation);
    }
    for (index, predicate) in [
        "c or (forall color in Set {Zed, Able}: color == Zed and Seq {1}.pop().head() == 0)",
        "Set {n + 2}.contains(2)",
        "forall p in Seq {P {a: 0}, P {a: n}}: p.a == 0",
        "forall o in Seq {some(0), some(n)}: o is some(v) and v == 0",
    ]
    .into_iter()
    .enumerate()
    {
        exercise(&format!("negative-{index}"), predicate, "", "invariant");
    }
    exercise(
        "local-negative",
        "out == 0",
        "let local = Seq {1, n + 1} out = local.at(1)",
        "invariant",
    );
    exercise("storage-outside", "true", "xs = Set {n + 3}", "_bounds_xs");
    exercise(
        "storage-capacity",
        "true",
        "empty = empty.push(1)",
        "_bounds_empty",
    );
}
