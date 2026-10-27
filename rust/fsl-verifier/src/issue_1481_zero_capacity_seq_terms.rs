// SPDX-License-Identifier: Apache-2.0

//! L2: ordinary symbolic zero-capacity read terms must be unconstrained and
//! fresh per evaluation, not fixed fallbacks or a shared unconstrained term.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, KernelExpr, KernelModel, build_model, parse_kernel_source};
use fsl_solver::{SatResult, SmtSolver};

use crate::{VerifyError, eval, value};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn assert_sat_read_conditions<S: SmtSolver>(
    solver: &mut S,
    read: &str,
    polarity: &str,
    conditions: &[&S::Term],
) {
    solver.push();
    for condition in conditions {
        solver.assert(condition).expect("assert read condition");
    }
    let result = block_on(solver.check()).expect("query read conditions");
    solver.pop(1).expect("restore identical path conditions");
    eprintln!("L2/{read}/{polarity}: {result:?}");
    assert_eq!(
        result,
        SatResult::Sat,
        "{read}: {polarity} must be satisfiable, not a fixed or reused read slot"
    );
}

fn read_term<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    read_expr: &KernelExpr,
    state: &value::SymbolicState<S::Term>,
) -> Result<S::Term, VerifyError> {
    let read_value = eval::eval(solver, model, read_expr, state, &mut BTreeMap::new(), None)?;
    let value::SymbolicValue::Scalar { term, .. } = read_value else {
        panic!("scalar read term required");
    };
    Ok(term)
}

#[cfg(test)]
fn assert_read_terms(read: &str, require_fresh_read: bool) {
    let source = format!(
        "spec UnobservedRead {{
  type K = 0..1
  state {{ xs: Seq<K, 0> }}
  init {{ xs = Seq {{}} }}
  action idle() {{ }}
  invariant Capture {{ {read} == 0 }}
}}"
    );
    let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("parse fixture");
    let model = build_model(kernel).expect("build fixture");
    let KernelExpr::Binary {
        left: read_expr, ..
    } = &model.invariants[0].expr
    else {
        panic!("fixture's equality must contain the read");
    };
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("native solver");
    let state = value::symbolic_state(&solver, &model, 0).expect("ordinary symbolic state");
    let xs = &state["xs"];
    let value::SymbolicValue::Seq { len, .. } = xs else {
        panic!("fixture state must be a Seq");
    };
    solver
        .assert(&value::bounds(&solver, &model, xs).expect("state bounds"))
        .expect("assert state bounds");
    solver
        .assert(
            &solver
                .equal(len, &solver.int_value(0))
                .expect("empty prefix"),
        )
        .expect("assert len=0 path condition");
    assert_eq!(
        block_on(solver.check()).expect("path is satisfiable"),
        SatResult::Sat
    );

    // Construct exactly one term, then keep the very same path assertions and
    // term for both queries. Re-evaluating the read in two solvers would not
    // establish that both polarities of this term are satisfiable.
    let term = read_term(&solver, &model, read_expr, &state)
        .unwrap_or_else(|error| panic!("{read}: construct ordinary unobserved read: {error:?}"));
    let equals_zero = solver
        .equal(&term, &solver.int_value(0))
        .expect("term == c");
    let differs_from_zero = solver.not(&equals_zero).expect("term != c");
    for (polarity, condition) in [
        ("term == 0", &equals_zero),
        ("term != 0", &differs_from_zero),
    ] {
        assert_sat_read_conditions(&mut solver, read, polarity, &[condition]);
    }

    if !require_fresh_read {
        return;
    }

    // Evaluate the same expression again, in the same solver/state/path. Keep
    // these two terms for both queries: a shared free constant passes the
    // single-term checks above but cannot satisfy either opposing assignment.
    let second_term = read_term(&solver, &model, read_expr, &state).unwrap_or_else(|error| {
        panic!("{read}: construct second ordinary unobserved read: {error:?}")
    });
    let second_equals_zero = solver
        .equal(&second_term, &solver.int_value(0))
        .expect("second term == c");
    let second_differs_from_zero = solver.not(&second_equals_zero).expect("second term != c");
    for (polarity, first_condition, second_condition) in [
        (
            "t1 == 0 && t2 != 0",
            &equals_zero,
            &second_differs_from_zero,
        ),
        (
            "t1 != 0 && t2 == 0",
            &differs_from_zero,
            &second_equals_zero,
        ),
    ] {
        assert_sat_read_conditions(
            &mut solver,
            read,
            polarity,
            &[first_condition, second_condition],
        );
    }
}

#[test]
fn zero_capacity_head_term_is_unconstrained() {
    assert_read_terms("xs.head()", false);
}

#[test]
fn zero_capacity_at_term_is_unconstrained() {
    assert_read_terms("xs.at(0)", false);
}

#[test]
fn zero_capacity_index_term_is_unconstrained() {
    assert_read_terms("xs[0]", false);
}

#[test]
fn zero_capacity_head_terms_are_fresh_per_read() {
    assert_read_terms("xs.head()", true);
}

#[test]
fn zero_capacity_at_terms_are_fresh_per_read() {
    assert_read_terms("xs.at(0)", true);
}

#[test]
fn zero_capacity_index_terms_are_fresh_per_read() {
    assert_read_terms("xs[0]", true);
}
