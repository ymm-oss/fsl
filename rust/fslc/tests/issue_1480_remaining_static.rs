// SPDX-License-Identifier: Apache-2.0
//! Independent static arithmetic R2 on verified local stacked B; no admission policy.
#[path = "support/issue_1480_remaining_static_support.rs"]
mod support;
use fsl_core::{FslValue as V, static_leadsto_bindings};
use fsl_runtime::{BoundedLivenessMonitor, eval};
use serde_json::json;
use std::collections::BTreeMap;
use support::{ROWS, cli, concrete, note, simple, source};

#[test]
fn empty_filtered_and_reached_arithmetic_bodies() {
    let mut errors = Vec::new();
    for &(expr, bad) in support::BODY_CASES {
        let src = simple(&format!("invariant P {{ {expr} }}"));
        cli(&src, bad, false, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn static_expression_consumers_and_per_value_controls() {
    let mut errors = Vec::new();
    for &(range, values, sum) in ROWS {
        let membership = values
            .iter()
            .map(|i| format!("i == ({i})"))
            .collect::<Vec<_>>()
            .join(" or ");
        let membership = ["false", &membership][usize::from(!membership.is_empty())];
        let mut predicates = vec![
            (format!("forall i in {range} {{ {membership} }}"), false),
            (format!("exists i in {range} {{ true }}"), values.is_empty()),
            (format!("count(i in {range}) == {}", values.len()), false),
            (format!("sum(i in {range} of i) == ({sum})"), false),
            (format!("count(i in {range}) != {}", values.len()), true),
            (format!("sum(i in {range} of i) != ({sum})"), true),
        ];
        predicates.extend(
            values
                .iter()
                .map(|i| (format!("forall i in {range} {{ i != ({i}) }}"), true)),
        );
        for (expr, bad) in predicates {
            let src = simple(&format!("invariant P {{ {expr} }}"));
            concrete(&src, &mut errors, |m, e| {
                let actual = eval(
                    &m.model.invariants[0].expr,
                    &m.state,
                    &mut BTreeMap::new(),
                    &m.model,
                    None,
                );
                let ok = actual.as_ref().ok() == Some(&V::Bool(!bad));
                note(e, &expr, ok, actual);
            });
            cli(&src, bad, false, &mut errors);
        }
        let selected = values.first().copied().unwrap_or(0);
        for (expr, want) in [
            (format!("forall i in {range} {{ {membership} }}"), true),
            (
                format!("exists i in {range} {{ i == ({selected}) }}"),
                !values.is_empty(),
            ),
        ] {
            for bad in [false, true] {
                let property = format!("invariant P {{ q == {} }}", want != bad);
                let src = source(&format!("q={expr}"), "", &property, "state { q: Bool }");
                concrete(&src, &mut errors, |m, e| {
                    let expected = BTreeMap::from([("q".into(), V::Bool(want))]);
                    note(
                        e,
                        "RHS full initial state",
                        m.state == expected,
                        (&m.state, &expected),
                    );
                });
                cli(&src, bad, false, &mut errors);
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn statement_candidates_full_poststates_and_init_coverage() {
    let mut errors = Vec::new();
    for &(range, values, _) in &ROWS[..20] {
        for position in ["action", "init"] {
            if position == "init" && values.is_empty() {
                continue;
            }
            let lo = if position == "init" { values[0] } else { -3 };
            let hi = if position == "init" {
                *values.last().unwrap()
            } else {
                4
            };
            let loop_text = format!("forall i in {range} {{ m[i]=true }}");
            let init = if position == "init" {
                loop_text.clone()
            } else {
                "forall k: K { m[k]=false }".into()
            };
            let action = if position == "action" {
                format!("{loop_text} d=true")
            } else {
                "d=true".into()
            };
            let expected = (lo..=hi)
                .map(|i| (V::Int(i), V::Bool(values.contains(&i))))
                .collect();
            let flag = if position == "init" { "true" } else { "d" };
            let property = (lo..=hi)
                .map(|i| format!("m[{i}] == ({} and {flag})", values.contains(&i)))
                .collect::<Vec<_>>()
                .join(" and ");
            let src = source(
                &format!("{init} d=false"),
                &action,
                &format!("invariant P {{ {property} }}"),
                &format!("type K={lo}..{hi} state {{ m: Map<K,Bool>, d: Bool }}"),
            );
            concrete(&src, &mut errors, |m, e| {
                if position == "action" {
                    let r = m.attempt("go", &BTreeMap::new());
                    note(e, "action execution", r.is_ok(), &r);
                }
                let want = BTreeMap::from([
                    ("m".into(), V::Map(expected)),
                    ("d".into(), V::Bool(position == "action")),
                ]);
                note(e, range, m.state == want, (&m.state, &want));
            });
            cli(&src, false, false, &mut errors);
            let bad = if position == "action" {
                "not d or "
            } else {
                ""
            };
            let broken = src.replace(
                &format!("invariant P {{ {property} }}"),
                &format!("invariant P {{ {bad}not ({property}) }}"),
            );
            cli(&broken, true, false, &mut errors);
        }
    }
    for end in ["MAX", "MIN"] {
        for position in ["init", "action"] {
            let body = format!("forall i in {end}..{end} {{ m[i]=true }}");
            let init = if position == "init" {
                format!("{body} d=true")
            } else {
                "forall k: K { m[k]=false } d=false".into()
            };
            let action = if position == "action" {
                format!("{body} d=true")
            } else {
                String::new()
            };
            let src = source(
                &init,
                &action,
                &format!("invariant P {{ m[{end}]==d }}"),
                &format!("type K={end}..{end} state {{ m: Map<K,Bool>, d: Bool }}"),
            );
            concrete(&src, &mut errors, |m, e| {
                if position == "action" {
                    let r = m.attempt("go", &BTreeMap::new());
                    note(e, "singleton action", r.is_ok(), r);
                }
                let key = V::Int(if end == "MAX" { i64::MAX } else { i64::MIN });
                let want = BTreeMap::from([
                    ("m".into(), V::Map(BTreeMap::from([(key, V::Bool(true))]))),
                    ("d".into(), V::Bool(true)),
                ]);
                note(e, "singleton full state", m.state == want, &m.state);
            });
            cli(&src, false, false, &mut errors);
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn temporal_wrapper_bindings_deadlines_and_pq_quantifiers_are_distinct() {
    let mut errors = Vec::new();
    for &(range, values, _) in ROWS {
        for response in ["true", "false"] {
            let src = simple(&format!(
                "leadsTo P {{ forall i in {range} {{ true ~> within 1 {response} }} }}"
            ));
            concrete(&src, &mut errors, |m, e| {
                let want = values
                    .iter()
                    .map(|i| BTreeMap::from([("i".into(), V::Int(*i))]))
                    .collect::<Vec<_>>();
                let bindings = static_leadsto_bindings(&m.model, &m.model.leadstos[0]);
                note(
                    e,
                    "ordered bindings",
                    bindings.as_ref().ok() == Some(&want),
                    &bindings,
                );
                match BoundedLivenessMonitor::new(m.model.clone()) {
                    Ok(mut live) => {
                        let ok = live.observe(&m.state, 0).is_ok_and(|v| v.is_none());
                        note(e, "tick0", ok, live.status());
                        let pending = live.status().pending;
                        let got = pending
                            .iter()
                            .map(|p| (p.bindings.clone(), p.pending_since, p.deadline, p.within))
                            .collect::<Vec<_>>();
                        let expected = if response == "false" {
                            want.iter().map(|b| (b.clone(), 0, 1, 1)).collect()
                        } else {
                            vec![]
                        };
                        note(e, "pending", got == expected, (&got, &expected));
                        let observed = live.observe(&m.state, 1);
                        let deciding =
                            observed.as_ref().ok().and_then(|v| v.as_ref()).map(|v| {
                                (&v.bindings, v.pending_since, v.deadline, v.within, v.step)
                            });
                        let expected = if response == "false" {
                            want.first().map(|b| (b, 0, 1, 1, 1))
                        } else {
                            None
                        };
                        let ok = observed.is_ok() && deciding == expected;
                        note(e, "deciding binding/time", ok, &observed);
                    }
                    Err(err) => note(e, "Monitor wrapper gap", false, err),
                }
            });
            cli(
                &src,
                response == "false" && !values.is_empty(),
                true,
                &mut errors,
            );
        }
        let src = simple(&format!(
            "leadsTo P {{ (exists i in {range} {{ true }}) ~> within 1 false }}"
        ));
        cli(&src, !values.is_empty(), true, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn selected_binding_and_quantified_q_controls() {
    let mut errors = Vec::new();
    for range in ["0..1", "0..(1+0)"] {
        let src = simple(&format!(
            "leadsTo P {{ forall i in {range} {{ true ~> within 1 i!=1 }} }}"
        ));
        concrete(
            &src,
            &mut errors,
            |m, e| match BoundedLivenessMonitor::new(m.model.clone()) {
                Ok(mut live) => {
                    let _ = live.observe(&m.state, 0);
                    let pending = live.status().pending;
                    let want = BTreeMap::from([("i".into(), V::Int(1))]);
                    let ok = pending.len() == 1
                        && pending[0].bindings == want
                        && pending[0].pending_since == 0
                        && pending[0].deadline == 1;
                    note(e, "selected pending", ok, pending);
                    let v = live.observe(&m.state, 1).unwrap();
                    let ok = v.as_ref().is_some_and(|v| {
                        v.bindings == want && v.pending_since == 0 && v.deadline == 1 && v.step == 1
                    });
                    note(e, "selected deadline", ok, &v);
                }
                Err(err) => note(e, "selected Monitor gap", false, err),
            },
        );
        for (route, p) in cli(&src, true, true, &mut errors) {
            if matches!(route.as_str(), "bmc" | "induction") && p["result"] == "violated" {
                let ok = p.get("bindings") == Some(&json!({"i":1}))
                    && p["pending_since"] == 0
                    && p["deadline"] == 1
                    && p["within"] == 1
                    && p["violation_kind"] == "leadsTo";
                note(&mut errors, "symbolic selected witness/deadline", ok, &p);
            }
        }
        for response in ["exists i", "forall i"] {
            let src = simple(&format!(
                "leadsTo P {{ true ~> within 1 ({response} in {range} {{ i==1 }}) }}"
            ));
            cli(&src, response == "forall i", true, &mut errors);
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
