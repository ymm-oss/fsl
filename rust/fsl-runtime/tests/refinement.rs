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

/// Coverage for issue #730's `trace::reconstruct_trace` generalization: the
/// two tests above both violate at step 0 (inside the root-immediate-
/// violation check, before any `ParentLink` is ever recorded), so neither
/// exercises the walk-back-to-root path a violation found *after* BFS
/// exploration starts takes. This impl has two nondeterministic-init roots
/// (`choose = true`/`false`) that only diverge once exploration begins:
/// `choose = true` can only call `inc_fast` (unconditionally enabled, `+2`
/// per step) and walks `n` out of its declared `0..2` bound two steps in;
/// `choose = false` can only call `inc_slow` (`+1`, guarded to at most one
/// call), which reaches `n = 1` and then deadlocks without ever violating.
/// `first_self_violation`'s shared `visited`/`queue`/`parents` therefore
/// hold live entries from both roots at once when the violation is found,
/// and reconstructing its trace must walk back through `inc_fast`'s own
/// `ParentLink` to the `choose = true` root -- not the `choose = false`
/// root, and not a caller-assumed single initial state.
#[test]
fn check_refinement_finds_a_self_violation_two_steps_into_one_of_two_nondeterministic_roots() {
    let implementation = model(
        "spec MRImplDeep { type IQty = 0..2 state { choose: Bool, n: IQty } \
         init { if choose { n = 0 } else { n = 0 } } \
         action inc_fast() { requires choose  n = n + 2 } \
         action inc_slow() { requires not choose  requires n < 1  n = n + 1 } }",
    );
    let abstraction = model(
        "spec MRAbsDeep { type AQty = 0..4 state { n: AQty } init { n = 0 } \
         action inc_fast() { n = n + 2 } action inc_slow() { n = n + 1 } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl MRImplDeep abs MRAbsDeep maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 4)
        .expect("check_refinement runs");

    let (violation, trace) = checked
        .impl_violation
        .expect("the choose = true root's two-step-deep out-of-bounds walk must be reported");
    assert_eq!(violation.kind, "type_bound");
    assert_eq!(
        trace.len(),
        3,
        "expected init + two inc_fast steps, not a step-0 shortcut: {trace:?}"
    );
    assert_eq!(
        trace[0].state["choose"],
        fsl_core::FslValue::Bool(true),
        "the reconstructed trace's root must be the violating (choose = true) branch, not the \
         other live root or a default initial state: {trace:?}"
    );
    assert_eq!(trace[0].state["n"], fsl_core::FslValue::Int(0));
    assert_eq!(trace[1].state["n"], fsl_core::FslValue::Int(2));
    assert_eq!(trace[2].state["n"], fsl_core::FslValue::Int(4));
    assert!(
        checked.failure.is_none(),
        "impl_violation and failure must be mutually exclusive: {:?}",
        checked.failure
    );
}

const DIVMAP_ABS: &str = "spec DivMapAbs { state { q: 0..10 } init { q = 0 } \
     action go(v in 0..10) { q = v } }";

/// Negative control for #512: an action-correspondence *argument* expression
/// (`go2(a) -> go(a / c)`) that divides by a state variable that can be zero
/// -- distinct from the impl action's own body, which has no division at all
/// here -- must be reported as a located `refinement_failed` /
/// `kind:"map_partial_op"` finding. Before the fix, the divisor's own
/// `eval()` call inside `check_refinement`'s action-correspondence handling
/// was not wrapped in `with_total_division` (correctly so: this is action
/// context per `docs/DESIGN-divmod.md` §2.2, not the read-only "mapping
/// expression" §2.3 exempts) but its `RuntimeError` was propagated raw via
/// `?` instead of being classified, so it surfaced as an unclassified
/// internal error the CLI stamps `kind:"type"` -- neither of the two
/// documented divide-by-zero treatments.
#[test]
fn check_refinement_reports_map_partial_op_for_a_zero_divisor_in_a_correspondence_argument() {
    let implementation = model(
        "spec DivMapImpl { state { q: 0..10, c: 0..5 } init { q = 0  c = 1 } \
         action set_c(v in 0..5) { c = v } \
         action go2(a in 0..10) { q = a } }",
    );
    let abstraction = model(DIVMAP_ABS);
    let mapping = parse_refinement(
        "refinement M { impl DivMapImpl abs DivMapAbs map q = q \
         action set_c(v) -> stutter action go2(a) -> go(a / c) }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("check_refinement runs");

    assert!(
        checked.impl_violation.is_none(),
        "the impl's own body never divides; this is a mapping-argument defect, not a self-violation: {:?}",
        checked.impl_violation
    );
    let failure = checked
        .failure
        .expect("the zero-divisor correspondence argument must be reported, not silently ignored");
    assert_eq!(failure.kind, "map_partial_op");
}

/// Regression control: the same action-correspondence argument division,
/// but guarded so the divisor is never zero on any reachable impl step, must
/// still `refines` -- the fix must not turn a legitimately total mapping
/// into a false `map_partial_op`.
#[test]
fn check_refinement_still_refines_when_the_correspondence_divisor_is_always_guarded() {
    let implementation = model(
        "spec DivMapImplGuarded { state { q: 0..10, c: 0..5 } init { q = 0  c = 1 } \
         action set_c(v in 0..5) { requires v != 0  c = v } \
         action go2(a in 0..10) { q = a / c } }",
    );
    let abstraction = model(DIVMAP_ABS);
    let mapping = parse_refinement(
        "refinement M { impl DivMapImplGuarded abs DivMapAbs map q = q \
         action set_c(v) -> stutter action go2(a) -> go(a / c) }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");

    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("check_refinement runs");

    assert!(checked.impl_violation.is_none());
    assert!(checked.failure.is_none(), "failure: {:?}", checked.failure);
}
