// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

const ABS: &str = "spec MinAbs { type AQty = 0..1 state { n: AQty } init { n = 1 } \
     action dec() { requires n > 0 n = n - 1 } }";

/// Negative control for #466: an impl whose own guardless `dec()` walks its
/// state past its declared type bound must be reported as violating itself
/// — never `refines`. Before the fix, `check_refinement`'s BFS silently
/// `continue`d past any `stepped.violation`, discarding the impl's own
/// type-bound violation and reporting `refines` because the correspondence
/// check was never reached for that step.
#[test]
fn check_refinement_reports_an_impl_self_violation_instead_of_refines() {
    let implementation = model(
        "spec MinImplBounded { type IQty = 0..1 state { n: IQty } init { n = 1 } \
         action dec() { n = n - 1 } }",
    );
    let abstraction = model(ABS);
    let mapping = parse_refinement(
        "refinement M { impl MinImplBounded abs MinAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 4)
        .expect("check_refinement runs");

    let (violation, trace) = checked
        .impl_violation
        .expect("impl's own type-bound violation must be reported");
    assert_eq!(violation.kind, "type_bound");
    assert_eq!(violation.name, "_bounds_n");
    assert!(!trace.is_empty());
    assert!(
        checked.failure.is_none(),
        "impl_violation and failure must be mutually exclusive: {:?}",
        checked.failure
    );
}

/// Regression control: an impl that never breaks its own bounds and
/// genuinely refines the abstraction must still report `impl_violation:
/// None` and `failure: None` (i.e. `refines`).
#[test]
fn check_refinement_still_refines_when_the_impl_is_internally_consistent() {
    let implementation = model(
        "spec MinImplRefines { type IQty = 0..1 state { n: IQty } init { n = 1 } \
         action dec() { requires n > 0 n = n - 1 } }",
    );
    let abstraction = model(ABS);
    let mapping = parse_refinement(
        "refinement M { impl MinImplRefines abs MinAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 4)
        .expect("check_refinement runs");

    assert!(checked.impl_violation.is_none());
    assert!(checked.failure.is_none());
}

/// Regression control: a genuine refinement fidelity failure (impl weakens
/// a guard the abstraction relies on, without ever breaking its own type
/// bounds) must still be reported as `refinement_failed` via `failure`, not
/// swallowed into `impl_violation`.
#[test]
fn check_refinement_still_reports_abs_requires_failed_for_a_pure_guard_weakening() {
    let implementation = model(
        "spec MinImplWide { type IQty = -1..1 state { n: IQty } init { n = 1 } \
         action dec() { n = n - 1 } }",
    );
    let abstraction = model(ABS);
    let mapping = parse_refinement(
        "refinement M { impl MinImplWide abs MinAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    // Depth 2 is enough to hit the guard-weakening mismatch (impl steps
    // n: 1 -> 0 -> -1, both within IQty's -1..1 bound) before the impl's own
    // bound would break at n = -2 (depth 3+, see #480/#466 commit history).
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");

    assert!(
        checked.impl_violation.is_none(),
        "impl_violation: {:?}",
        checked.impl_violation
    );
    let failure = checked.failure.expect("guard weakening must be detected");
    assert_eq!(failure.kind, "abs_requires_failed");
}

/// Negative control for #493, false-green direction (R3): a nondeterministic
/// impl `init` (an `if` reading an unassigned `Bool`) must have *every*
/// concrete initial valuation checked against the abs init constraints, not
/// just the one default state `Monitor::new` used to materialize before the
/// fix. Here `choose = false` (the default) maps to a value the abs init
/// accepts, but `choose = true` does not — the fix must not miss it.
#[test]
fn check_refinement_finds_init_mismatch_reachable_only_from_a_nondeterministic_impl_branch() {
    let implementation = model(
        "spec MinImplNondetInit { type IQty = 0..1 state { choose: Bool, x: IQty } \
         init { if choose { x = 1 } else { x = 0 } } action noop() { x = x } }",
    );
    let abstraction = model(
        "spec MinAbsDetInit { type AQty = 0..1 state { x: AQty } init { x = 0 } \
         action noop() { x = x } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl MinImplNondetInit abs MinAbsDetInit maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");

    assert!(
        checked.impl_violation.is_none(),
        "impl_violation: {:?}",
        checked.impl_violation
    );
    let failure = checked
        .failure
        .expect("the choose = true initial branch must be checked and must not refine");
    assert_eq!(failure.kind, "abs_state_mismatch");
    assert_eq!(failure.at.as_deref(), Some("init"));
    assert_eq!(
        failure.impl_trace[0].state["choose"],
        fsl_core::FslValue::Bool(true),
        "the reported counterexample must be the failing branch, not the default"
    );
}

/// Negative control for #493, false-positive direction (R4): a
/// nondeterministic abs `init` must be checked as *set membership* — does
/// some valid abs initial valuation match α(s₀)? — not equality against the
/// single default abs state `Monitor::new` used to materialize before the
/// fix. Here the impl deterministically starts in the abs's non-default
/// initial branch (`choose = true`), which is a genuinely correct
/// refinement that the old equality check rejected.
#[test]
fn check_refinement_accepts_a_correct_refinement_of_a_nondeterministic_abs_init() {
    let implementation = model(
        "spec MinImplDetInit { type IQty = 0..1 state { choose: Bool, x: IQty } \
         init { choose = true x = 1 } action noop() { x = x } }",
    );
    let abstraction = model(
        "spec MinAbsNondetInit { type AQty = 0..1 state { choose: Bool, x: AQty } \
         init { if choose { x = 1 } else { x = 0 } } action noop() { x = x } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl MinImplDetInit abs MinAbsNondetInit maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");

    assert!(
        checked.impl_violation.is_none(),
        "impl_violation: {:?}",
        checked.impl_violation
    );
    assert!(
        checked.failure.is_none(),
        "a genuine refinement of a nondeterministic abs init must not be rejected: {:?}",
        checked.failure
    );
}

/// Regression control: an ordinary refinement where both impl and abs `init`
/// are fully deterministic must still refine — the #493 fix (iterating over
/// every enumerated initial valuation instead of one) must not change the
/// single-candidate case.
#[test]
fn check_refinement_still_refines_with_ordinary_deterministic_init_on_both_sides() {
    let implementation = model(
        "spec MinImplDetBoth { type IQty = 0..1 state { choose: Bool, x: IQty } \
         init { choose = true x = 1 } action noop() { x = x } }",
    );
    let abstraction = model(
        "spec MinAbsDetBoth { type AQty = 0..1 state { choose: Bool, x: AQty } \
         init { choose = true x = 1 } action noop() { x = x } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl MinImplDetBoth abs MinAbsDetBoth maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");

    assert!(checked.impl_violation.is_none());
    assert!(checked.failure.is_none());
}

/// Negative control for #493's step-0 precondition ([`first_self_violation`]
/// shares the same enumeration): a self-violation reachable only from a
/// non-default nondeterministic initial branch (`choose = true` walks `n`
/// out of its declared bound immediately at init) must still be reported as
/// `impl_violation`, not missed because only the default branch was checked.
#[test]
fn check_refinement_finds_a_self_violation_reachable_only_from_a_nondeterministic_impl_branch() {
    let implementation = model(
        "spec MinImplNondetInitSelfViolation { type IQty = 0..1 state { choose: Bool, n: IQty } \
         init { if choose { n = 1 + 1 } else { n = 1 } } \
         action dec() { requires n > 0 n = n - 1 } }",
    );
    let abstraction = model(ABS);
    let mapping = parse_refinement(
        "refinement M { impl MinImplNondetInitSelfViolation abs MinAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");

    let (violation, trace) = checked
        .impl_violation
        .expect("the choose = true branch's own out-of-bounds init must be reported");
    assert_eq!(violation.kind, "type_bound");
    assert_eq!(
        trace[0].state["choose"],
        fsl_core::FslValue::Bool(true),
        "the reported counterexample must be the violating branch"
    );
    assert!(
        checked.failure.is_none(),
        "impl_violation and failure must be mutually exclusive: {:?}",
        checked.failure
    );
}
