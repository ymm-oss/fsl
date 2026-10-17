// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1259, second half: the symbolic `x is some(v)` bound `v` whether or
//! not `x` was present, while the concrete evaluator binds only when present.
//! With `v` also a state variable, the right of `or` read the state variable
//! concretely but the unconstrained absent payload symbolically, so bmc and
//! induction reported a false `violated` where explicit proved the spec.
//!
//! `check` now rejects the collision, so these models are built with a fresh
//! pattern name and renamed afterwards: the shape a producer that bypasses
//! `check` could still hand the engines. Explicit, bmc and induction must agree.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, KernelExpr, KernelModel, Pattern, build_model, parse_kernel_source};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn rename_pattern(expr: &mut KernelExpr, from: &str, to: &str) -> usize {
    match expr {
        KernelExpr::Is {
            expr: item,
            pattern,
        } => {
            let mut renamed = rename_pattern(item, from, to);
            if let Pattern::Some(name) = pattern
                && name == from
            {
                to.clone_into(name);
                renamed += 1;
            }
            renamed
        }
        KernelExpr::Binary { left, right, .. } => {
            rename_pattern(left, from, to) + rename_pattern(right, from, to)
        }
        KernelExpr::Not(item) => rename_pattern(item, from, to),
        _ => 0,
    }
}

/// Build `source` (whose patterns bind the fresh name `fresh`) and rename that
/// pattern to the state variable `exact`.
fn colliding_model(source: &str) -> KernelModel {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let mut model = build_model(kernel).expect("build model");
    let mut renamed = 0;
    for property in model.invariants.iter_mut().chain(&mut model.reachables) {
        renamed += rename_pattern(&mut property.expr, "fresh", "exact");
    }
    for action in &mut model.actions {
        for expr in &mut action.requires {
            renamed += rename_pattern(expr, "fresh", "exact");
        }
        for guard in &mut action.guards {
            if let fsl_core::ActionGuard::Requires(expr) = guard {
                renamed += rename_pattern(expr, "fresh", "exact");
            }
        }
    }
    assert!(renamed > 0, "fixture has no `is some(fresh)` to rename");
    model
}

const HEADER: &str = "
  type K = 0..2
  state { p: Option<K>, exact: K }
  init { p = none  exact = 0 }
";

const DEPTH: usize = 4;

/// The issue's `AbsentBindFalse`: in the only state where `p` is absent,
/// `exact == 0`, so `Y` is `false or 0 <= 2`.
#[test]
fn absent_payload_does_not_shadow_a_state_variable_in_an_invariant() {
    let model = colliding_model(&format!(
        "spec AbsentBindFalse {{{HEADER}
  action set(k: K) {{ p = some(k) }}
  action clear() {{ p = none }}
  invariant Y {{ p is some(fresh) or exact <= 2 }}
}}"
    ));
    let explicit = fsl_runtime::verify_explicit(model.clone(), DEPTH, 10_000).expect("explicit");
    assert!(explicit.violation.is_none(), "explicit: {explicit:?}");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let bmc = block_on(fsl_verifier::verify_bounded(&model, &mut solver, DEPTH)).expect("bmc");
    assert!(bmc.violation.is_none(), "bmc: {:?}", bmc.violation);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let induction =
        block_on(fsl_verifier::prove_induction(&model, &mut solver, DEPTH)).expect("induction");
    assert!(induction.cti.is_none(), "induction: {:?}", induction.cti);
}

/// The triage's `AbsentBindReach`: `p` is always absent and `exact` always
/// `0`, so `R` is unreachable; bmc used to witness it at step 0.
#[test]
fn absent_payload_does_not_witness_a_reachable() {
    let model = colliding_model(&format!(
        "spec AbsentBindReach {{{HEADER}
  action clear() {{ p = none }}
  reachable R {{ p is some(fresh) or exact == 2 }}
}}"
    ));
    let explicit = fsl_runtime::verify_explicit(model.clone(), DEPTH, 10_000).expect("explicit");
    assert!(explicit.closure, "explicit: {explicit:?}");
    assert_eq!(
        explicit.reachables.get("R"),
        Some(&None),
        "explicit: {explicit:?}"
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let bmc = block_on(fsl_verifier::verify_bounded(&model, &mut solver, DEPTH)).expect("bmc");
    assert!(
        matches!(bmc.reachables.get("R"), Some(None)),
        "bmc witnessed R: {:?}",
        bmc.reachables
    );
}

/// The triage's `AbsentBindCover`: the guard is the same disjunction, so
/// `poke` is never enabled; bmc used to call it covered.
#[test]
fn absent_payload_does_not_enable_a_guard() {
    let model = colliding_model(&format!(
        "spec AbsentBindCover {{{HEADER}
  action clear() {{ p = none }}
  action poke() {{ requires p is some(fresh) or exact == 2  p = none }}
}}"
    ));
    let explicit = fsl_runtime::verify_explicit(model.clone(), DEPTH, 10_000).expect("explicit");
    assert_eq!(
        explicit.action_coverage.get("poke"),
        Some(&false),
        "explicit"
    );
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let bmc = block_on(fsl_verifier::verify_bounded(&model, &mut solver, DEPTH)).expect("bmc");
    assert_eq!(bmc.action_coverage.get("poke"), Some(&false), "bmc");
}
