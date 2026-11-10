// SPDX-License-Identifier: Apache-2.0

//! Intended recursive semantics, not preservation of unsupported-engine errors.
#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod support;
use fsl_core::FslValue as V;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use support::{finish, full_fixture, full_observe, require, stable_full_packet};
struct Kind {
    name: &'static str,
    item: &'static str,
    state: &'static str,
    init: &'static str,
    effect: &'static str,
    zero: V,
    one: V,
}
fn kinds() -> Vec<Kind> {
    let int = V::Int;
    let some = |v| V::Some(Box::new(v));
    let color = |member: &str| V::Enum {
        type_name: "Color".into(),
        member: member.into(),
    };
    let seq = |v| V::Seq(vec![int(0), int(v)]);
    let set = |v| V::Set(BTreeSet::from([int(0), int(v)]));
    let p = |n, e| V::Struct {
        type_name: "P".into(),
        fields: BTreeMap::from([
            ("a".into(), int(n)),
            ("o".into(), some(some(int(n)))),
            ("z".into(), color(e)),
        ]),
    };
    let rows = [
        ("int", "$", int(0), int(1)),
        ("bool", "$ == 1", V::Bool(false), V::Bool(true)),
        (
            "enum",
            "if $ == 0 then Zed else Able",
            color("Zed"),
            color("Able"),
        ),
        (
            "struct",
            "P {a: $, z: if $ == 0 then Zed else Able, o: some(some($))}",
            p(0, "Zed"),
            p(1, "Able"),
        ),
        ("option", "some($)", some(int(0)), some(int(1))),
        (
            "option-enum",
            "some(if $ == 0 then Zed else Able)",
            some(color("Zed")),
            some(color("Able")),
        ),
        (
            "nested-absence",
            "if $ == 0 then oo else some(none)",
            some(some(int(0))),
            some(V::None),
        ),
        ("set", "Set {0, $}", set(0), set(1)),
        ("seq", "Seq {0, $}", seq(0), seq(1)),
        (
            "set-seq",
            "Set {Seq {0, $}, Seq {0, 0}}",
            V::Set(BTreeSet::from([seq(0)])),
            V::Set(BTreeSet::from([seq(0), seq(1)])),
        ),
        (
            "seq-set",
            "Seq {Set {0, $}, Set {0, 0}}",
            V::Seq(vec![set(0), set(0)]),
            V::Seq(vec![set(1), set(0)]),
        ),
    ];
    let mut result: Vec<_> = rows
        .into_iter()
        .map(|(name, item, zero, one)| Kind {
            name,
            item,
            state: "",
            init: "",
            effect: "",
            zero,
            one,
        })
        .collect();
    result.push(Kind {
        name: "relation",
        item: "if $ == 0 then q else r",
        state: ", r: relation K -> K, q: relation K -> L",
        init: "r = Set {} q = Set {}",
        effect: "r = r.add(0, 1)",
        zero: V::Relation(BTreeSet::new()),
        one: V::Relation(BTreeSet::from([(int(0), int(1))])),
    });
    // Admission of existing Map values was measured before this row was added.
    result.push(Kind {
        name: "map",
        item: "if $ == 0 then q else m",
        state: ", m: Map<K, K>, q: Map<K, K>",
        init: "forall k: K { m[k] = 0 q[k] = 0 }",
        effect: "m[0] = 1",
        zero: V::Map(BTreeMap::from([(int(0), int(0)), (int(1), int(0))])),
        one: V::Map(BTreeMap::from([(int(0), int(1)), (int(1), int(0))])),
    });
    result
}
fn source(kind: &Kind, predicate: &str, frozen: bool) -> String {
    format!(
        "spec Composite {{\n  type K = 0..1\n  type L = 0..1\n  enum Color {{ Zed, Able }}\n  struct P {{ a: K, z: Color, o: Option<Option<K>> }}\n  state {{ n: K, c: Bool, f: K, oo: Option<Option<K>>{} }}\n  init {{ n = 0 c = true f = 0 oo = some(some(0)) {} }}\n  action go() {{ n = 1 c = false {} }}\n  invariant Capture {{ {predicate} }}\n}}\n",
        kind.state,
        kind.init,
        if frozen { "" } else { kind.effect }
    )
}

fn predicates(kind: &Kind, outer: &str) -> Vec<(String, String, String, bool)> {
    let item = |v| format!("({})", kind.item.replace('$', v));
    let (a, b, f) = (item("0"), item("n"), item("f"));
    let lit = |items: &str| format!("{outer} {{{items}}}");
    let (base, live, both) = (lit(&a), lit(&b), lit(&format!("{a}, {b}")));
    let mut rows = Vec::new();
    for reverse in [false, true] {
        let (left, right) = if reverse {
            (&live, &base)
        } else {
            (&base, &live)
        };
        let item = if reverse { &b } else { &a };
        let operation = if outer == "Set" { "add" } else { "push" };
        let suffix = if outer == "Seq" { ".pop()" } else { "" };
        let typed = format!("{left}.{operation}({item}){suffix}");
        let (ql, qr) = [(left, &typed), (&typed, left)][usize::from(reverse)];
        rows.push((
            format!("Q-{reverse}"),
            format!("{left} == {left} and {ql} == {qr}"),
            format!("{left} == {right}"),
            false,
        ));
        let join = if reverse {
            format!("if not c then {live} else {base}")
        } else {
            format!("if c then {base} else {live}")
        };
        rows.push((
            format!("C-{reverse}"),
            format!("({join}) == {live}"),
            format!("({join}) == {base}"),
            false,
        ));
    }
    let (positive, negative) = if outer == "Set" {
        (
            format!(
                "{both}.add({a}).remove({b}).add({a}).contains({a}) and {outer} {{{b}, {b}}}.size() == 1"
            ),
            format!("{live}.add({b}).contains({a})"),
        )
    } else {
        (
            format!(
                "{outer} {{{b}, {a}}}.pop().push({a}).head() == {a} and {outer} {{{b}, {b}}}.size() == 2 and {both}[1] == {b}"
            ),
            format!("{outer} {{{b}, {a}}}.pop().push({b}).at(1) == {a}"),
        )
    };
    rows.push(("M".into(), positive, negative, false));
    let projection = if outer == "Set" {
        format!(
            "count(k: K where k == 0 and {both}.contains({a})) == 1 and sum(k: K of if k == 0 and {both}.contains({a}) then 1 else 0) == 1"
        )
    } else {
        format!(
            "count(x in {both}) == 2 and sum(x in {both} of 1) == 2 and count(x in {both} where false) == 0"
        )
    };
    rows.push(("B".into(), format!("(forall x in {both}: x == {a} or x == {b}) and (exists x in {both}: x == {a}) and {projection} and unique(x in {both} where x != {a}) and exactlyOne(x in {live} where x == {b})"),
        format!("forall x in {both}: x == {a}"), false));
    rows.push((
        "F".into(),
        format!("{}.contains({f})", lit(&f)),
        format!("not {}.contains({f})", lit(&f)),
        true,
    ));
    rows
}

fn verdict(report: &Value, exit: i32, route: &str, negative: bool, frozen: bool) -> bool {
    match route {
        "check" => exit == 0 && report["result"] == "ok",
        route if route.starts_with("kernel-") => exit == 0 && report["result"] == "kernel",
        "monitor" if negative => {
            exit == 1
                && report["result"] == "nonconformant"
                && report["violation"]["kind"] == "invariant"
                && report["violation"]["name"] == "Capture"
                && report["failed_at_event"] == 0
        }
        "monitor" => exit == 0 && report["result"] == "conformant" && report["steps_checked"] == 1,
        _ if negative => {
            exit == 1
                && report["result"] == "violated"
                && report["violation_kind"] == "invariant"
                && report["invariant"] == "Capture"
                && report["violated_at_step"] == usize::from(!frozen)
                && report["loc"] == json!({"line":9,"column":3})
        }
        _ => {
            exit == 0
                && report["result"] == if route == "bmc" { "verified" } else { "proved" }
                && report["completeness"]
                    == if route == "bmc" {
                        "bounded"
                    } else {
                        "unbounded"
                    }
                && report["invariant"].is_null()
                && report["trace"].is_null()
        }
    }
}

fn matrix(name: &str) -> Vec<String> {
    let kind = kinds().into_iter().find(|kind| kind.name == name).unwrap();
    let mut errors = Vec::new();
    for outer in ["Set", "Seq"] {
        for (consumer, positive, negative, frozen) in predicates(&kind, outer) {
            for (negative, predicate) in [(false, positive), (true, negative)] {
                let text = source(&kind, &predicate, frozen);
                let label = format!("1467-{name}-{outer}-{consumer}-{negative}");
                let mut previous = BTreeMap::new();
                let dir = full_fixture(&label, &text);
                for repeat in 1..=2 {
                    std::fs::write(dir.join("oracle.txt"), format!("negative={negative}; frozen={frozen}; property=Capture; kind=invariant; step={}; loc=9:3", usize::from(!frozen))).unwrap();
                    for route in ["check", "monitor", "bmc", "induction", "explicit"]
                        .into_iter()
                        .chain(["kernel-v1", "kernel-v2"])
                    {
                        let (report, exit) = full_observe(&dir, route);
                        let mut ok = verdict(&report, exit, route, negative, frozen);
                        if route == "check" && !ok {
                            errors.push(format!("NON-ADMISSION {label}: {report}"));
                            break; // A rejected spelling is never semantic RED.
                        }
                        if frozen && !negative && route == "explicit" {
                            ok &= report["warnings"].as_array().is_some_and(|warnings| {
                                warnings.iter().any(|w| {
                                    w["kind"] == "tautology_over_frozen" && w["name"] == "Capture"
                                })
                            });
                        }
                        println!(
                            "MATRIX {label} {repeat} {route} exit={exit} result={} expected_negative={negative} ok={ok}",
                            report["result"]
                        );
                        require(&mut errors, ok, &label, route, &report, exit);
                        let stable = (stable_full_packet(report, route), exit);
                        if let Some(prior) = previous.insert(route, stable.clone())
                            && prior != stable
                        {
                            errors.push(format!(
                                "FULL PACKET DRIFT {label} {route}: {prior:?} != {stable:?}"
                            ));
                        }
                    }
                }
            }
        }
    }
    errors
}

#[test]
fn admitted_kinds_cross_both_outer_collections_and_all_consumers() {
    let errors: Vec<_> = kinds().iter().flat_map(|kind| matrix(kind.name)).collect();
    finish(&errors);
}

#[test]
fn literal_recursive_values_have_independent_concrete_and_symbolic_oracles() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut errors = Vec::new();
    for kind in kinds() {
        for outer in ["Set", "Seq"] {
            let a = kind.item.replace('$', "0");
            let b = kind.item.replace('$', "n");
            let text = source(
                &kind,
                &format!("{outer} {{({a}), ({b})}} == {outer} {{({a}), ({b})}}"),
                false,
            );
            for _ in 0..2 {
                let dir = full_fixture(&format!("1467-projection-{}-{outer}", kind.name), &text);
                let kernel =
                    fsl_core::parse_kernel_source(&text, &fsl_core::FsResolver::new(".")).unwrap();
                let model = fsl_core::build_model(kernel).expect("projection source admitted");
                let fsl_core::KernelExpr::Binary { left, .. } = &model.invariants[0].expr else {
                    panic!("equality probe")
                };
                let mut monitor = fsl_runtime::Monitor::new(model.clone()).unwrap();
                let go = monitor
                    .enabled()
                    .unwrap()
                    .into_iter()
                    .find(|a| a.action == "go")
                    .unwrap();
                let step = monitor.step(&go).unwrap();
                let expected = if outer == "Set" {
                    V::Set(BTreeSet::from([kind.zero.clone(), kind.one.clone()]))
                } else {
                    V::Seq(vec![kind.zero.clone(), kind.one.clone()])
                };
                let concrete =
                    fsl_runtime::eval(left, &step.state, &mut BTreeMap::new(), &model, None)
                        .unwrap();
                let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
                let mut future = std::pin::pin!(fsl_verifier::expression_matches_value(
                    &model,
                    &mut solver,
                    left,
                    &step.state,
                    &expected
                ));
                let symbolic = match future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                {
                    Poll::Ready(value) => value,
                    Poll::Pending => panic!("native solver yielded"),
                };
                let bfs = fsl_runtime::verify_explicit(model.clone(), 4, 100).unwrap();
                std::fs::write(dir.join("literal-oracle.txt"), format!("expected={expected:?}\nconcrete={concrete:?}\nsymbolic={symbolic:?}\nbfs={bfs:?}\n")).unwrap();
                if concrete != expected
                    || !matches!(symbolic, Ok(true))
                    || !bfs.closure
                    || bfs.violation.is_some()
                {
                    errors.push(format!("{} {outer}: expected={expected:?}, concrete={concrete:?}, symbolic={symbolic:?}, bfs={bfs:?}", kind.name));
                }
            }
        }
    }
    finish(&errors);
}
