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
