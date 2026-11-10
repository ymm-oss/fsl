// SPDX-License-Identifier: Apache-2.0

//! Private storage observation only; no new production exports or comparator oracle.
use super::*;
use fsl_solver::SmtSolver;
use fsl_solver_z3::{Z3Solver, Z3Term};

fn keys(solver: &Z3Solver, model: &KernelModel, value: &SymbolicValue<Z3Term>) -> Vec<String> {
    collection_entries(solver, model, value)
        .unwrap()
        .1
        .iter()
        .map(|(key, _)| format!("{:?}", literal_key(solver, key)))
        .collect()
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
fn declared_prefix_and_appended_support_survive_alias_transition_and_order_view() {
    let text = "spec Prefix { type K = 0..1 type L = 0..1 enum Color { Zed, Able } state { r: relation K -> K, q: relation L -> L, e: relation Color -> Color, f: relation Color -> Color } init { r = Set {} q = Set {} e = Set {} f = Set {} } action go() { r = q.add(3,0).add(0,3) e = f.add(Zed,Able) } invariant Closure { reachable(r,3,3) } invariant Ordered { exists x in Set {r,q}: x == q or Seq {1}.pop().head() == 0 } invariant EnumOrdered { exists x in Set {e,f}: x == f or Seq {1}.pop().head() == 0 } }";
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(text, &fsl_core::FsResolver::new(".")).unwrap(),
    )
    .unwrap();
    let expected = [
        "Some([Int(0), Int(0)])",
        "Some([Int(0), Int(1)])",
        "Some([Int(1), Int(0)])",
        "Some([Int(1), Int(1)])",
        "Some([Int(3), Int(0)])",
        "Some([Int(0), Int(3)])",
    ];
    let mut failures = Vec::new();
    for repeat in 0..2 {
        let mut solver = Z3Solver::new().unwrap();
        let current: SymbolicState<_> = model
            .state
            .iter()
            .map(|(name, ty)| {
                (
                    name.clone(),
                    concrete_value(&solver, &model, ty, &model.default_value(ty).unwrap()).unwrap(),
                )
            })
            .collect();
        let initial = keys(&solver, &model, &current["r"]);
        assert_eq!(initial, expected[..4]);
        let next = crate::transition::compute_updates_in(
            &solver,
            &model,
            &model.actions[0].statements,
            &current,
            current.clone(),
            &mut TypedBindings::new(),
            None,
        )
        .unwrap();
        let before = keys(&solver, &model, &next["r"]);
        // Enum storage follows declaration ordinals: Zed=0 precedes Able=1,
        // unlike recursive Value's spelling order. Sorting this grid is wrong.
        let enum_before = keys(&solver, &model, &next["e"]);
        assert_eq!(enum_before[..4], expected[..4]);
        assert_eq!(enum_before[..4], keys(&solver, &model, &current["e"]));
        println!(
            "ENUM_STORAGE repeat={repeat} Zed=0,Able=1 prefix={:?}",
            &enum_before[..4]
        );
        println!("STORAGE repeat={repeat} expected={expected:?} produced={before:?}");
        assert_eq!(
            before, expected,
            "declared prefix then insertion-order support, never sorted storage"
        );
        let prefix_bytes = format!(
            "{:?}",
            collection_entries(&solver, &model, &next["r"]).unwrap().1[..4]
                .iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>()
        );
        assert_eq!(ready(solver.check()).unwrap(), fsl_solver::SatResult::Sat);
        for invariant in &model.invariants {
            let defined = crate::eval::definedness(
                &solver,
                &model,
                &invariant.expr,
                &next,
                &TypedBindings::new(),
                None,
            )
            .and_then(|term| project_bool(&solver, &term));
            let observed = crate::eval::eval(
                &solver,
                &model,
                &invariant.expr,
                &next,
                &mut TypedBindings::new(),
                None,
            )
            .and_then(|value| project_value(&solver, &model, &value));
            println!(
                "PREFIX repeat={repeat} property={} expected=true/defined observed={observed:?}/{defined:?}",
                invariant.name
            );
            match observed {
                Ok(FslValue::Bool(true)) if matches!(defined, Ok(true)) => {}
                other => failures.push(format!("{}: {other:?}", invariant.name)),
            }
        }
        let after = collection_entries(&solver, &model, &next["r"]).unwrap().1;
        assert_eq!(
            prefix_bytes,
            format!(
                "{:?}",
                after[..4].iter().map(|(key, _)| key).collect::<Vec<_>>()
            )
        );
        assert_eq!(keys(&solver, &model, &next["r"]), expected);
        assert_eq!(keys(&solver, &model, &next["e"]), enum_before);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
