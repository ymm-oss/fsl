// SPDX-License-Identifier: Apache-2.0
//! Independent finite execution lift oracles; no new admission restrictions.
#[path = "support/issue_1480_dynamic_range_support.rs"]
mod support;
use serde_json::json;
use support::*;

#[test]
fn bounded_state_endpoints_literal_values_and_rejecting_opposites() {
    let mut errors = Vec::new();
    // Ordered literal candidates are authored, never computed by domain helpers.
    for (n, candidates, sum) in [(0, vec![0], 0), (1, vec![0, 1], 1), (2, vec![0, 1, 2], 3)] {
        let membership = candidates
            .iter()
            .map(|i| format!("i=={i}"))
            .collect::<Vec<_>>()
            .join(" or ");
        let expressions = [
            format!("forall i in 0..n {{ {membership} }}"),
            format!("exists i in 0..n {{ i=={n} }}"),
            format!("count(i in 0..n)=={}", candidates.len()),
            format!("sum(i in 0..n of i)=={sum}"),
        ];
        for expression in expressions {
            for bad in [false, true] {
                let predicate = if bad {
                    format!("not ({expression})")
                } else {
                    expression.clone()
                };
                exercise(
                    &model(n, &format!("n!={n} or ({predicate})")),
                    bad,
                    &mut errors,
                );
            }
        }
        for candidate in candidates {
            exercise(
                &model(
                    n,
                    &format!("n!={n} or (forall i in 0..n {{ i!={candidate} }})"),
                ),
                true,
                &mut errors,
            );
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn signed_both_endpoints_and_dynamic_empty_neutrals() {
    let mut errors = Vec::new();
    for (n, sum) in [(0, 0), (1, 3), (2, 6)] {
        for expression in [
            "count(i in (n-1)..(n+1))==3".into(),
            format!("sum(i in (n-1)..(n+1) of i)=={sum}"),
            "forall i in (-1*n)..n { i>=(-1*n) and i<=n }".into(),
            "exists i in (-1*n)..n { i==0 }".into(),
            "forall i in (n+1)..n { 1/0>0 }".into(),
            "not (exists i in (n+1)..n { 1/0>0 })".into(),
            "count(i in (n+1)..n)==0".into(),
            "sum(i in (n+1)..n of (1/0))==0".into(),
        ] {
            for bad in [false, true] {
                let p = if bad {
                    format!("not ({expression})")
                } else {
                    expression.clone()
                };
                exercise(&model(n, &format!("n!={n} or ({p})")), bad, &mut errors);
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn lexical_outer_exact_six_pairs_aggregates_and_shadowing() {
    let mut errors = Vec::new();
    let pairs = [(0, 0), (1, 0), (1, 1), (2, 0), (2, 1), (2, 2)];
    let membership = pairs
        .iter()
        .map(|(i, j)| format!("(i=={i} and j=={j})"))
        .collect::<Vec<_>>()
        .join(" or ");
    let predicates = [
        format!("forall i in 0..2 {{ forall j in 0..i {{ {membership} }} }}"),
        "sum(i in 0..2 of count(j in 0..i))==6".into(),
        "sum(i in 0..2 of sum(j in 0..i of (10*i+j)))==84".into(),
        "forall i in 0..2 { forall i in 0..1 { i<=1 } }".into(),
    ];
    for p in predicates {
        exercise(&model(0, &p), false, &mut errors);
        exercise(&model(0, &format!("not ({p})")), true, &mut errors);
    }
    for (i, j) in pairs {
        exercise(
            &model(
                0,
                &format!("forall i in 0..2 {{ forall j in 0..i {{ i!={i} or j!={j} }} }}"),
            ),
            true,
            &mut errors,
        );
    }
    // Wrong rectangular pair (0,1) must never enter the triangle.
    exercise(
        &model(0, "forall i in 0..2 { forall j in 0..i { i!=0 or j!=1 } }"),
        false,
        &mut errors,
    );
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn parameter_membership_preserves_full_map_poststate_inductively() {
    let mut errors = Vec::new();
    let src = "spec A { type K=0..2 state { m:Map<K,Bool> } init { forall k:K { m[k]=false } } action fill(lim:K) { forall i in 0..lim { m[i]=true } } action go() { forall i in 0..1 { m[i]=true } } invariant P { (not m[1] or m[0]) and (not m[2] or m[1]) } }";
    fill(src, &mut errors);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn assigned_init_rhs_is_not_init_statement_admission() {
    let mut errors = Vec::new();
    for (n, q) in [(0, true), (1, true), (2, true)] {
        for bad in [false, true] {
            let p = if bad { "not q" } else { "q" };
            let src = format!(
                "spec A {{ type K=0..2 state {{ n:K, q:Bool }} init {{ n={n} q=exists i in 0..n {{ i=={n} }} }} action go() {{}} invariant P {{ {p} }} }}"
            );
            rhs(&src, &json!({"n":n,"q":q}), bad, &mut errors);
        }
    }
    // Admitted outer statement / inner EXPRESSION; nested-statement modes stay frozen.
    let src = "spec A { type K=0..2 state { m:Map<K,Bool> } init { forall i in 0..2 { m[i]=exists j in 0..i { j==i } } } action go() {} invariant P { forall k:K { m[k] } } }";
    rhs(
        src,
        &json!({"m":{"0":true,"1":true,"2":true}}),
        false,
        &mut errors,
    );
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn static_outer_temporal_three_bindings_and_selected_deadline() {
    let mut errors = Vec::new();
    for bad in [false, true] {
        let response = if bad { "d and (i!=1 or j!=1)" } else { "d" };
        let src = format!(
            "spec A {{ state {{ d:Bool }} init {{ d=false }} action go() {{ d=true }} leadsTo P {{ forall i in 0..1 {{ forall j in 0..i {{ not d ~> within 1 ({response}) }} }} }} }}"
        );
        temporal(&src, bad, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn dynamic_pq_expressions_are_not_time_varying_wrapper_domains() {
    let mut errors = Vec::new();
    for bad in [false, true] {
        let q = if bad {
            "forall i in 0..n { i<0 }"
        } else {
            "exists i in 0..n { i==n }"
        };
        let src = format!(
            "spec A {{ type K=0..2 state {{ d:Bool }} init {{ d=false }} action go() {{ d=true }} leadsTo P {{ forall n:K {{ (not d and (exists i in 0..n {{ i==n }})) ~> within 1 (d and ({q})) }} }} }}"
        );
        cli(&src, bad, true, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn already_outside_carrier_type_bound_has_priority_over_range_property() {
    let mut errors = Vec::new();
    let src = model(0, "forall i in 0..n { i<=2 }");
    cli(&src, false, false, &mut errors);
    concrete(&src, &mut errors, |m, e| {
        m.state.insert("n".into(), fsl_core::FslValue::Int(3));
        let observed = m.current_violation();
        note(
            e,
            "outside carrier must not be clipped",
            observed
                .as_ref()
                .is_ok_and(|v| v.as_ref().is_some_and(|v| v.kind == "type_bound")),
            &observed,
        );
    });
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn empty_dynamic_action_body_and_reached_sibling_partial_operation() {
    let mut errors = Vec::new();
    for (range, reached) in [("(n+1)..n", false), ("n..n", true)] {
        let src = format!(
            "spec A {{ type K=0..2 state {{ n:K, m:Map<K,Int> }} init {{ n=0 forall k:K {{ m[k]=0 }} }} action go() {{ forall i in {range} {{ m[i]=1/i }} }} invariant P {{ true }} }}"
        );
        partial(&src, reached, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn protected_kernel_empty_inventory_is_not_partial_expansion_failure() {
    let mut errors = Vec::new();
    for (body, partial) in [
        ("m[k]=1", false),
        ("m[k]=m[k]", false),
        ("m[k]=1/k", true),
        ("m[k]=xs[k]", true),
    ] {
        let src = format!(
            "spec A {{ type K=0..2 state {{ n:K, m:Map<K,Int>, xs:Seq<Int,1> }} init {{ n=1 xs=Seq {{}} forall k:K {{ m[k]=0 }} }} action go() {{ forall k in 0..n {{ {body} }} }} invariant P {{ true }} }}"
        );
        let (exit, p) = probe(&src, "kernel", &json!(null));
        note(
            &mut errors,
            "protected Kernel route",
            if partial {
                exit == 2
                    && p == json!({"fsl":"1.0","result":"error","kind":"semantics","message":"'n' is not an integer const"})
            } else {
                exit == 0
                    && p["result"] == "kernel"
                    && p["actions"][0]["partial_operations"] == json!([])
            },
            &p,
        );
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn changing_bounded_state_is_not_clipped_to_initial_value() {
    let mut errors = Vec::new();
    for p in [
        "exists i in 0..n { i==n }",
        "count(i in 0..n)==n+1",
        "sum(i in 0..n of i)==(n*(n+1))/2",
    ] {
        let src = format!(
            "spec A {{ type K=0..2 state {{ n:K }} init {{ n=0 }} action go() {{ n=2 }} invariant P {{ {p} }} }}"
        );
        exercise(&src, false, &mut errors);
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
