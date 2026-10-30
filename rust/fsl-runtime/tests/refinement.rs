// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::TraceStep;
use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};
use fsl_runtime::{RefinementCheck, RefinementFailure, RefinementVerdict, Violation};

#[path = "../../fslc/tests/support/identifiers.rs"]
mod identifiers;

// The outcome is read only through `RefinementCheck::verdict` (#1262). In
// debug builds `verdict()` also asserts that at most one outcome field is
// set, which is what the old "impl_violation and failure must be mutually
// exclusive" assertions here checked by hand.

fn impl_violation(checked: &RefinementCheck) -> (&Violation, &[TraceStep]) {
    match checked.verdict() {
        RefinementVerdict::ImplViolated { violation, trace } => (violation, trace),
        other => panic!("expected the impl's own violation, got {other:?}"),
    }
}

fn failure(checked: &RefinementCheck) -> &RefinementFailure {
    match checked.verdict() {
        RefinementVerdict::Failed(failure) => failure,
        other => panic!("expected a refinement failure, got {other:?}"),
    }
}

fn assert_refines(checked: &RefinementCheck) {
    assert_eq!(checked.verdict(), RefinementVerdict::Refines);
}

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

const ABS: &str = "spec MinAbs { type AQty = 0..1 state { n: AQty } init { n = 1 } \
     action dec() { requires n > 0 n = n - 1 } }";

#[test]
fn issue_1599_runtime_rejects_collisions_without_frontend_validation() {
    let implementation =
        model("spec Impl1599Spec { type Carrier1599 = 0..2 state { x: Int = 0 } }");
    let mut abstraction =
        model("spec Abs1599Spec { type Carrier1599 = 0..2 state { x: Int = 0 } }");
    let mapping = parse_refinement(
        "refinement R { impl Impl1599Spec abs Abs1599Spec maps auto }",
        &implementation,
        &abstraction,
    )
    .unwrap();
    assert_refines(
        &fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2).unwrap(),
    );
    // Inject the conflicting operand after parsing: no frontend collision gate can protect runtime.
    abstraction.types.insert(
        "Carrier1599".into(),
        fsl_core::TypeDef::Domain {
            lo: 0,
            hi: 1,
            symmetric: false,
        },
    );
    let observed = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2);
    println!("OBS runtime collision {observed:?}");
    let message = observed
        .expect_err("runtime must reject different domain definitions")
        .to_string();
    identifiers::assert_names(&message, &["Carrier1599", "Impl1599Spec", "Abs1599Spec"]);
}

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

    let (violation, trace) = impl_violation(&checked);
    assert_eq!(violation.kind, "type_bound");
    assert_eq!(violation.name, "_bounds_n");
    assert!(!trace.is_empty());
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

    assert_refines(&checked);
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

    let failure = failure(&checked);
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

    let failure = failure(&checked);
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

    assert_refines(&checked);
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

    assert_refines(&checked);
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

    let (violation, trace) = impl_violation(&checked);
    assert_eq!(violation.kind, "type_bound");
    assert_eq!(
        trace[0].state["choose"],
        fsl_core::FslValue::Bool(true),
        "the reported counterexample must be the violating branch"
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

    let (violation, trace) = impl_violation(&checked);
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
/// context per `docs/design/DESIGN-divmod.md` §2.2, not the read-only "mapping
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

    // Not an impl violation: the impl's own body never divides; this is a
    // mapping-argument defect. Nor silently ignored: it must be reported.
    let failure = failure(&checked);
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

    assert_refines(&checked);
}

const SEQ_ABS: &str = "spec SeqAbs { type V = 0..2 state { seq: Seq<V, 2> } init { seq = Seq {} } \
     action push(v: V) { requires seq.size() < 2 seq = seq.push(v) } }";

// Original complete fixtures remain inputs to the rejection/sweep controls.
const SEQ_WIDE: &str = "spec SeqWide { type V = -1..2 state { seq: Seq<V, 2> } init { seq = Seq {} } \
         action push(v: V) { requires seq.size() < 2 seq = seq.push(v) } }";
const SEQ_WIDER: &str = "spec SeqWider { type V = 0..3 state { seq: Seq<V, 2> } init { seq = Seq {} } \
         action push(v: V) { requires seq.size() < 2 seq = seq.push(v) } }";
const SEQ_GUARDED: &str = "spec SeqGuarded { type V = 0..2 state { seq: Seq<V, 2> } init { seq = Seq {} } \
         action push(v: V) { requires v <= 2 requires seq.size() < 2 seq = seq.push(v) } }";
const SEQ_WIDE_MAP: &str =
    "refinement M { impl SeqWide abs SeqAbs map seq = seq action push(v) -> push(v) }";
const SEQ_WIDER_MAP: &str =
    "refinement M { impl SeqWider abs SeqGuarded map seq = seq action push(v) -> push(v) }";

fn argument_model(source: &str) -> fsl_core::KernelModel {
    // The parameter can escape without either whole-carrier mismatch or an
    // implementation storage violation masking the argument/guard finding.
    model(
        &source
            .replace("state {", "type S = 0..2 state {")
            .replace("Seq<V, 2>", "Seq<S, 2>")
            .replace("Seq<AV, 2>", "Seq<S, 2>")
            .replace("seq.push(v)", "seq.push(0)"),
    )
}

fn reject_old_seq_map(implementation: &str, abstraction: &str, source: &str, column: u32) {
    let start = usize::try_from(column - 1).unwrap();
    assert_eq!(&source[start..start + 3], "map");
    assert!(!source[..start].contains('\n'));
    let error = parse_refinement(source, &model(implementation), &model(abstraction))
        .expect_err("different model-local Seq carriers must be rejected");
    assert_eq!(error.kind, fsl_core::RefinementErrorKind::Type);
    assert_eq!(
        error.message,
        "invalid map expression for 'seq': expression of type Seq(Named(\"V\"), 2) is not assignable to Seq(Named(\"V\"), 2)"
    );
    let span = error.span.expect("located map rejection");
    assert_eq!((span.start.line, span.start.column), (1, column));
    assert_eq!((span.start.offset, span.end.offset), (start, start + 3));
    assert_eq!((span.end.line, span.end.column), (1, column + 3));
}

#[test]
fn old_argument_fixture_whole_seq_map_is_rejected() {
    reject_old_seq_map(SEQ_WIDE, SEQ_ABS, SEQ_WIDE_MAP, 40);
}

#[test]
fn old_guard_fixture_whole_seq_map_is_rejected() {
    reject_old_seq_map(SEQ_WIDER, SEQ_GUARDED, SEQ_WIDER_MAP, 45);
}

const MAP_ABS: &str = "spec MapAbs { type K = 0..2 state { m: Map<K, Bool> } \
     init { forall k: K { m[k] = false } } action set(k: K) { m[k] = true } \
     invariant Total { forall k: K { m[k] or not m[k] } } }";

/// An impl whose parameter domain is wider than the abstract action's maps a
/// step to an abstract call with an argument outside the abstract parameter
/// domain. That is a range escape of the mapping (`map_out_of_bounds`), not a
/// runtime error from stepping the abstraction with an ill-typed argument.
#[test]
fn an_abstract_argument_outside_its_parameter_domain_is_map_out_of_bounds() {
    let implementation = argument_model(SEQ_WIDE);
    let abstraction = argument_model(&SEQ_ABS.replace('V', "AV"));
    let mapping =
        parse_refinement(SEQ_WIDE_MAP, &implementation, &abstraction).expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("an out-of-domain abstract argument is a finding, not an error");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "map_out_of_bounds");
    assert_eq!(failure.at.as_deref(), Some("step"));
    assert_eq!(failure.step, 1);
}

/// A narrower impl key domain under an indexed map: the mapping reads impl
/// keys the impl does not have.
#[test]
fn an_indexed_map_over_a_narrower_impl_key_domain_is_map_out_of_bounds() {
    let implementation = model(
        "spec MapNarrow { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall k: K { m[k] = false } } action set(k: K) { m[k] = true } }",
    );
    let abstraction = model(&MAP_ABS.replace('K', "AK"));
    let mapping = parse_refinement(
        "refinement M { impl MapNarrow abs MapAbs maps auto }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("a missing abstract key is a finding, not an error");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "map_out_of_bounds");
    assert_eq!(failure.at.as_deref(), Some("init"));
}

/// DESIGN-refinement §2.4 checks the transition before bounds: with an
/// out-of-domain argument the abstract guard is still evaluated first, and a
/// guard it fails is `abs_requires_failed`, exactly as before the
/// out-of-domain check existed. Only a guard that holds leaves the range escape
/// to report as `map_out_of_bounds`.
#[test]
fn a_false_abstract_guard_wins_over_an_out_of_domain_argument() {
    let implementation = argument_model(SEQ_WIDER);
    let abstraction = argument_model(&SEQ_GUARDED.replace('V', "AV"));
    let mapping =
        parse_refinement(SEQ_WIDER_MAP, &implementation, &abstraction).expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "abs_requires_failed");
    assert_eq!(failure.step, 1);
}

fn colliding_used_domain(body: &str) {
    let implementation = model(&format!("spec Impl1599Spec {{ type T = 0..1 {body} }}"));
    let abstraction = model(&format!("spec Abs1599Spec {{ type T = 0..2 {body} }}"));
    let error = parse_refinement(
        "refinement R { impl Impl1599Spec abs Abs1599Spec maps auto }",
        &implementation,
        &abstraction,
    )
    .expect_err("used domains must obey the same declaration rule");
    let span = error.span.expect("located type error");
    assert_eq!((span.start.line, span.start.column), (1, 1));
    identifiers::assert_names(&error.message, &["T", "Impl1599Spec", "Abs1599Spec"]);
}

#[test]
fn issue_1599_rejects_colliding_scalar_parameter_domains() {
    colliding_used_domain(
        "state { seq: Seq<T, 2> } init { seq = Seq {} } action push(v: T) { requires seq.size() < 2 seq = seq.push(v) }",
    );
}

#[test]
fn issue_1599_rejects_colliding_map_key_domains() {
    colliding_used_domain(
        "state { m: Map<T, Bool> } init { forall k: T { m[k] = false } } action set(k: T) { m[k] = true }",
    );
}

const BOOL_ABS: &str =
    "spec BoolAbs { state { x: Bool } init { x = false } action go() { x = true } }";

/// The step-level arm: a mapping expression that reads an impl `Map` at a key
/// outside its domain only after some steps (`idx` reaches 2, `K` is 0..1).
#[test]
fn a_mapping_read_outside_an_impl_map_domain_mid_walk_is_map_out_of_bounds() {
    let implementation = model(
        "spec MapIdx { type K = 0..1 type J = 0..2 state { m: Map<K, Bool>, idx: J } \
         init { forall k: K { m[k] = false } idx = 0 } \
         action go() { requires idx < 2 idx = idx + 1 } }",
    );
    let abstraction = model(BOOL_ABS);
    let mapping = parse_refinement(
        "refinement M { impl MapIdx abs BoolAbs map x = m[idx] action go() -> stutter }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 4)
        .expect("an out-of-domain map read is a finding, not an error");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "map_out_of_bounds");
    assert_eq!(failure.at.as_deref(), Some("step"));
    assert_eq!(failure.step, 2);
}

/// Negative control: only the map-key marker becomes `map_out_of_bounds`. Any
/// other mapping evaluation error (here a `Seq` index out of range, a separate
/// follow-up) stays an error.
#[test]
fn a_mapping_error_without_the_map_key_marker_stays_an_error() {
    let implementation = model(
        "spec SeqIdx { type J = 0..3 state { s: Seq<Bool, 2>, idx: J } \
         init { s = Seq {} idx = 0 } action go() { requires idx < 3 idx = idx + 1 } }",
    );
    let abstraction = model(BOOL_ABS);
    let mapping = parse_refinement(
        "refinement M { impl SeqIdx abs BoolAbs map x = s[idx] action go() -> stutter }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    let error = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 4)
        .expect_err("a non-marker mapping error must not become a verdict");
    assert!(!error.map_key_outside_domain, "{error:?}");
}

// Issue #1296: the abstract action's own body hits a partial operation at
// the mapped state. The impl `go()` corresponds to the abstract `go()`, but
// the abstraction has no defined successor there, so the step cannot
// correspond. `Monitor::step` reports the body's `partial_op`; the
// correspondence walk used to ignore it and accept the step.

const DIV_IMPL: &str =
    "spec I { type N = 0..4 state { z: N } init { z = 0 } action go() { z = 0 } }";
const DIV_MAP: &str = "refinement M { impl I abs A map y = z action go() -> go() }";

fn div_check(abs: &str) -> RefinementCheck {
    let implementation = model(DIV_IMPL);
    let abstraction = model(abs);
    let mapping = parse_refinement(DIV_MAP, &implementation, &abstraction).expect("parse mapping");
    fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("an abstract-body partial operation is a finding, not an error")
}

/// The issue's repro: `y = 4 / y` from `y = 0`. Under the total `/` the
/// update would be `y = 0`, equal to the mapped impl state, so this is not a
/// state mismatch: it is the action-context `partial_op` rule applied to the
/// abstract action the impl step invokes (DESIGN-divmod §2.2).
#[test]
fn an_abstract_body_division_by_zero_is_abs_partial_op() {
    let checked = div_check(
        "spec A { type N = 0..4 state { y: N } init { y = 0 } action go() { y = 4 / y } }",
    );
    let failure = failure(&checked);
    assert_eq!(failure.kind, "abs_partial_op");
    assert_eq!(failure.at.as_deref(), Some("step"));
    assert_eq!(failure.step, 1);
    assert_eq!(
        failure
            .impl_action
            .as_ref()
            .map(|action| action.name.as_str()),
        Some("go")
    );
    // No abstract successor exists, so there is no expected state.
    assert_eq!(failure.alpha_after_expected, None);
    assert!(failure.alpha_before.is_some() && failure.alpha_after_actual.is_some());
}

/// §2.4 order: the abstract guard is evaluated before the body, so a false
/// guard is still `abs_requires_failed` even though the body would divide by
/// zero at the same state.
#[test]
fn a_false_abstract_guard_still_wins_over_a_partial_abstract_body() {
    let checked = div_check(
        "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { requires y > 0 y = 4 / y } }",
    );
    assert_eq!(failure(&checked).kind, "abs_requires_failed");
}

/// The guarded variant whose guard holds at the mapped state: the body is
/// reached and divides by zero.
#[test]
fn a_true_abstract_guard_reaches_the_partial_body_as_abs_partial_op() {
    let checked = div_check(
        "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { requires y < 4 y = 4 / y } }",
    );
    let failure = failure(&checked);
    assert_eq!(failure.kind, "abs_partial_op");
    assert_eq!(failure.step, 1);
}

/// Not only division: a `Seq` `pop()` on the empty abstract sequence has no
/// value at all (no total fallback), and is the same finding.
#[test]
fn an_abstract_body_pop_on_an_empty_seq_is_abs_partial_op() {
    let implementation = model(
        "spec SI { type V = 0..1 state { t: Seq<V, 2> } init { t = Seq {} } \
         action take() { t = t } }",
    );
    let abstraction = model(
        "spec SA { type V = 0..1 state { s: Seq<V, 2> } init { s = Seq {} } \
         action take() { s = s.pop() } }",
    );
    let mapping = parse_refinement(
        "refinement M { impl SI abs SA map s = t action take() -> take() }",
        &implementation,
        &abstraction,
    )
    .expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 2)
        .expect("check_refinement runs");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "abs_partial_op");
    assert_eq!(failure.step, 1);
}

/// Negative control: an abstract body whose division never reaches a zero
/// divisor on a corresponded step still `refines`.
#[test]
fn a_total_abstract_body_division_still_refines() {
    let implementation = model(
        "spec I { type N = 0..4 state { z: N } init { z = 0 } action go() { z = 4 / (z + 1) } }",
    );
    let abstraction = model(
        "spec A { type N = 0..4 state { y: N } init { y = 0 } action go() { y = 4 / (y + 1) } }",
    );
    let mapping = parse_refinement(DIV_MAP, &implementation, &abstraction).expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("check_refinement runs");
    assert_refines(&checked);
}

/// The body partial-op check runs before the state comparison. Here the impl
/// step moves the mapped state to `y = 3` while the abstract `go()` divides by
/// zero at `y = 0`: the state `step` returns (the unchanged `y = 0`) also
/// differs from `y = 3`, so a check placed after the comparison would report
/// `abs_state_mismatch` against a successor the abstraction does not have.
#[test]
fn an_abstract_body_partial_op_is_reported_before_the_state_comparison() {
    let implementation = model(
        "spec I { type N = 0..4 state { z: N } init { z = 0 } \
         action go() { requires z == 0 z = 3 } }",
    );
    let abstraction =
        model("spec A { type N = 0..4 state { y: N } init { y = 0 } action go() { y = 4 / y } }");
    let mapping = parse_refinement(DIV_MAP, &implementation, &abstraction).expect("parse mapping");
    let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("check_refinement runs");
    let failure = failure(&checked);
    assert_eq!(failure.kind, "abs_partial_op");
    assert_eq!(failure.alpha_after_expected, None);
}

// Issue #1302: the corresponded abstract step itself violates something other
// than a body partial operation -- a post-state type bound, an abstract
// invariant, a false `ensures`, or a partial operation inside `ensures`.
// `Monitor::step` reports it with the would-be successor in
// `attempted_state` and returns the rolled-back α(s) as `state`. The walk
// used to compare only that rolled-back state with α(s'): a false `refines`
// when the impl step left the mapped state unchanged, and an
// `abs_state_mismatch` whose `abs_after_expected` was the rolled-back α(s)
// when it changed it. Each row below is checked both ways.

const STEP_IMPL_SAME: &str =
    "spec I { type N = 0..4 state { z: N } init { z = 0 } action go() { z = 0 } }";
const STEP_IMPL_CHANGED: &str = "spec I { type N = 0..4 state { z: N } init { z = 0 } \
     action go() { requires z == 0 z = 1 } }";
const STEP_MAP: &str = "refinement M { impl I abs A map y = z action go() -> go() }";
const STEP_MAP_W: &str = "refinement M { impl I abs A map y = z map w = 0 action go() -> go() }";

fn step_check(implementation: &str, abs: &str, map: &str) -> RefinementCheck {
    let implementation = model(implementation);
    let abstraction = model(abs);
    let mapping = parse_refinement(map, &implementation, &abstraction).expect("parse mapping");
    fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 3)
        .expect("an abstract step violation is a finding, not an error")
}

/// The finding every row shares: the step fails at step 1 on the impl's
/// `go`, no abstract successor is reported as expected (the abstraction has
/// none; the rolled-back α(s) is not one), and the abstract step's own
/// violation is carried as `abs_violation`.
fn assert_abs_step_violation<'a>(
    checked: &'a RefinementCheck,
    abs_kind: &str,
    abs_name: &str,
) -> &'a RefinementFailure {
    let failure = failure(checked);
    assert_eq!(failure.kind, "abs_step_violation", "{failure:?}");
    assert_eq!(failure.at.as_deref(), Some("step"));
    assert_eq!(failure.step, 1);
    assert_eq!(
        failure
            .impl_action
            .as_ref()
            .map(|action| action.name.as_str()),
        Some("go")
    );
    assert_eq!(failure.alpha_after_expected, None, "{failure:?}");
    assert!(failure.alpha_before.is_some() && failure.alpha_after_actual.is_some());
    let violation = failure
        .abs_violation
        .as_ref()
        .expect("abs_step_violation carries the abstract violation");
    assert_eq!(
        (violation.kind.as_str(), violation.name.as_str()),
        (abs_kind, abs_name)
    );
    assert!(
        failure.alpha_after_attempted.is_some(),
        "abs_step_violation carries the attempted abstract successor: {failure:?}"
    );
    failure
}

const TYPE_ABS: &str =
    "spec A { type N = 0..4 state { y: N } init { y = 0 } action go() { y = y + 5 } }";

/// Row 1, impl leaves the mapped state unchanged (was `refines`): the
/// abstract successor `y = 5` escapes `N = 0..4`.
#[test]
fn an_abstract_post_state_type_bound_violation_is_not_refines() {
    let checked = step_check(STEP_IMPL_SAME, TYPE_ABS, STEP_MAP);
    let failure = assert_abs_step_violation(&checked, "type_bound", "_bounds_y");
    // The attempted successor is `y = 5`, the out-of-type state the
    // violation is about -- not the rolled-back α(s) `y = 0`.
    assert_eq!(
        failure
            .alpha_after_attempted
            .as_ref()
            .and_then(|state| state.get("y")),
        Some(&fsl_core::FslValue::Int(5)),
        "{failure:?}"
    );
}

/// Row 1, impl changes the mapped state (was `abs_state_mismatch` with the
/// rolled-back α(s) as `abs_after_expected`).
#[test]
fn an_abstract_post_state_type_bound_violation_is_not_a_state_mismatch() {
    assert_abs_step_violation(
        &step_check(STEP_IMPL_CHANGED, TYPE_ABS, STEP_MAP),
        "type_bound",
        "_bounds_y",
    );
}

/// Row 2, unchanged: `ensures y == 3` is false at the abstract successor.
#[test]
fn a_false_abstract_ensures_is_not_refines() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_SAME,
            "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y ensures y == 3 } }",
            STEP_MAP,
        ),
        "ensures",
        "go",
    );
}

/// Row 2, changed: the update matches the impl (`y = 1`), only `ensures`
/// is false.
#[test]
fn a_false_abstract_ensures_is_not_a_state_mismatch() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_CHANGED,
            "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y + 1 ensures y == 3 } }",
            STEP_MAP,
        ),
        "ensures",
        "go",
    );
}

/// Row 3, unchanged: `ensures 4 / y == 0` divides by zero at `y = 0`.
/// Unlike #1296's body partial op, `step` reports this one with an
/// `attempted_state`.
#[test]
fn a_partial_op_in_abstract_ensures_is_not_refines() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_SAME,
            "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y ensures 4 / y == 0 } }",
            STEP_MAP,
        ),
        "partial_op",
        "_partial_go",
    );
}

/// Row 3, changed: the update matches the impl, `ensures` divides by zero.
#[test]
fn a_partial_op_in_abstract_ensures_is_not_a_state_mismatch() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_CHANGED,
            "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y + 1 ensures 4 / (y - 1) == 0 } }",
            STEP_MAP,
        ),
        "partial_op",
        "_partial_go",
    );
}

/// Row 4, unchanged: the abstract successor `w = 1` violates `W`, while
/// α(s') (`w = 0` by the mapping) does not, so the existing α(s') check
/// does not see it either.
#[test]
fn an_abstract_invariant_violated_by_the_abstract_successor_is_not_refines() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_SAME,
            "spec A { type N = 0..4 state { y: N, w: N } init { y = 0 w = 0 } \
         action go() { w = 1 } invariant W { w == 0 } }",
            STEP_MAP_W,
        ),
        "invariant",
        "W",
    );
}

/// Row 4, changed.
#[test]
fn an_abstract_invariant_violated_by_the_abstract_successor_is_not_a_state_mismatch() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_CHANGED,
            "spec A { type N = 0..4 state { y: N, w: N } init { y = 0 w = 0 } \
         action go() { y = y + 1 w = 1 } invariant W { w == 0 } }",
            STEP_MAP_W,
        ),
        "invariant",
        "W",
    );
}

/// Negative controls: an abstract `ensures` that holds still `refines`,
/// whether the corresponded step leaves the mapped state unchanged or not.
#[test]
fn an_abstract_ensures_that_holds_still_refines() {
    assert_refines(&step_check(
        STEP_IMPL_SAME,
        "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y ensures y == 0 } }",
        STEP_MAP,
    ));
    assert_refines(&step_check(
        STEP_IMPL_CHANGED,
        "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y + 1 ensures y == 1 } }",
        STEP_MAP,
    ));
}

/// Negative control: an abstract invariant the abstract successor keeps
/// still `refines`.
#[test]
fn an_abstract_invariant_the_successor_keeps_still_refines() {
    assert_refines(&step_check(
        STEP_IMPL_CHANGED,
        "spec A { type N = 0..4 state { y: N, w: N } init { y = 0 w = 0 } \
         action go() { y = y + 1 } invariant W { w == 0 } }",
        STEP_MAP_W,
    ));
}

/// A `trans` the abstract step breaks: `Grow` needs `y` to increase, the
/// abstract update keeps it. α(s') equals the attempted successor, so only
/// the abstract step's own `trans` check sees it.
#[test]
fn an_abstract_trans_violated_by_the_abstract_step_is_not_refines() {
    assert_abs_step_violation(
        &step_check(
            STEP_IMPL_SAME,
            "spec A { type N = 0..4 state { y: N } init { y = 0 } \
             action go() { y = y } trans Grow { y > old(y) } }",
            STEP_MAP,
        ),
        "trans",
        "Grow",
    );
}

/// A partial operation inside an abstract invariant at the attempted
/// successor: `q.pop()` empties `q`, and `P`'s `q.head()` has no value there
/// (a `Seq` operation; `/` and `%` are total in property context).
#[test]
fn a_partial_op_in_an_abstract_invariant_at_the_successor_is_not_refines() {
    let checked = step_check(
        STEP_IMPL_SAME,
        "spec A { type N = 0..4 state { y: N, q: Seq<N, 2> } \
         init { y = 0 q = Seq { 1 } } action go() { q = q.pop() } \
         invariant P { q.head() >= 0 } }",
        "refinement M { impl I abs A map y = z map q = Seq { 1 } action go() -> go() }",
    );
    let failure = assert_abs_step_violation(&checked, "partial_op", "_partial_property_P");
    assert!(failure.alpha_after_attempted.is_some(), "{failure:?}");
}

/// The walk steps the abstraction on one re-pointed monitor instead of a
/// fresh `Monitor::from_state(abstraction.clone(), ..)` per transition
/// (issues #1174, #1247). The abstract step's own violation must still read
/// as that of a fresh monitor: here the abstract `ensures` first fails on
/// the third mapped transition, after two abstract steps on the same
/// monitor, and its `step` is still 1, not 3.
#[test]
fn an_abstract_step_violation_after_earlier_transitions_reads_as_a_fresh_step() {
    let checked = step_check(
        "spec I { type N = 0..4 state { z: N } init { z = 0 } \
         action go() { requires z < 3 z = z + 1 } }",
        "spec A { type N = 0..4 state { y: N } init { y = 0 } \
         action go() { y = y + 1 ensures y <= 2 } }",
        STEP_MAP,
    );
    let failure = failure(&checked);
    assert_eq!(
        (failure.kind.as_str(), failure.step),
        ("abs_step_violation", 3)
    );
    let violation = failure.abs_violation.as_ref().expect("abs_violation");
    assert_eq!(
        (
            violation.kind.as_str(),
            violation.name.as_str(),
            violation.step
        ),
        ("ensures", "go", 1),
        "{failure:?}"
    );
}
