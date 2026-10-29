// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{
    FsResolver, KernelModel, Refinement, build_model, parse_kernel_source, parse_refinement,
};
use fsl_verifier::{ProgressCheck, VerifyError, check_refinement_progress};

#[path = "../../fslc/tests/support/identifiers.rs"]
mod identifiers;

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse model"))
        .expect("build model")
}

type ProgressResult = Result<ProgressCheck, VerifyError>;
fn mapping(imp: &KernelModel, abs: &KernelModel) -> Refinement {
    parse_refinement("refinement R { impl Impl1599Spec abs Abs1599Spec map y = x action go() -> step() preserve progress { respond Progress1599 by go } }", imp, abs).unwrap()
}
fn progress(imp: &KernelModel, abs: &KernelModel, mapping: &Refinement) -> ProgressResult {
    let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
    block_on(check_refinement_progress(imp, abs, mapping, &mut solver, 4))
}

fn pulled_safety(broken: bool, name: &str) -> ProgressResult {
    let implementation = model(&format!(
        "spec Impl1599Spec {{ state {{ x: Int = 0 }} fair action go() {{ requires x == 0 x = 1 }} invariant {name} {{ x <= {} }} }}",
        i32::from(!broken)
    ));
    let abstraction = model(
        "spec Abs1599Spec { state { y: Int = 0 } fair action step() { requires y == 0 y = 1 } leadsTo Progress1599 { y == 0 ~> y == 1 } }",
    );
    let observed = progress(
        &implementation,
        &abstraction,
        &mapping(&implementation, &abstraction),
    );
    println!("OBS pulled safety broken={broken} {observed:?}");
    observed
}

#[test]
fn issue_1599_pulled_safety_fails_closed() {
    assert_pulled_safety("Invariant1599Broken");
}

fn assert_pulled_safety(name: &str) {
    let error = pulled_safety(true, name).expect_err("pulled safety must not become refines");
    assert!(!error.is_solver_failure());
    let message = error.to_string();
    identifiers::assert_names(&message, &["pulled", name]);
}

#[test]
fn issue_1599_pulled_safety_renamed_invariant() {
    assert_pulled_safety("Invariant1599Renamed");
}

#[test]
fn issue_1599_pulled_safety_counterpart_keeps_progress_check() {
    let checked = pulled_safety(false, "Invariant1599Broken").unwrap();
    assert!(checked.violation.is_none(), "{checked:?}");
    assert_eq!(checked.checked["Progress1599"], vec!["go".to_owned()]);
}

fn merge_collision(member: bool) {
    let implementation = model(
        "spec Impl1599Spec { type Carrier1599 = 0..2 enum Source1599 { Member1599A, Member1599B } state { x: Int = 0 } fair action go() { requires x == 0 x = 1 } }",
    );
    let mut abstraction = model(
        "spec Abs1599Spec { type Carrier1599 = 0..2 enum Target1599 { Member1599Q } state { y: Int = 0 } fair action step() { requires y == 0 y = 1 } leadsTo Progress1599 { y == 0 ~> y == 1 } }",
    );
    let mapping = mapping(&implementation, &abstraction);
    assert!(
        progress(&implementation, &abstraction, &mapping)
            .unwrap()
            .violation
            .is_none()
    );
    let name = if member {
        abstraction.enum_members.remove("Member1599Q");
        abstraction.types.insert(
            "Target1599".into(),
            fsl_core::TypeDef::Enum {
                members: vec!["Member1599B".into()],
                symmetric: false,
            },
        );
        abstraction.enum_members.insert(
            "Member1599B".into(),
            fsl_core::FslValue::Enum {
                type_name: "Target1599".into(),
                member: "Member1599B".into(),
            },
        );
        "Member1599B"
    } else {
        abstraction.types.insert(
            "Carrier1599".into(),
            fsl_core::TypeDef::Domain {
                lo: 0,
                hi: 1,
                symmetric: false,
            },
        );
        "Carrier1599"
    };
    let observed = progress(&implementation, &abstraction, &mapping);
    println!("OBS verifier merge {name} {observed:?}");
    identifiers::assert_names(
        &observed
            .expect_err("direct verifier must reject merge collision")
            .to_string(),
        &[name, "Impl1599Spec", "Abs1599Spec"],
    );
}
#[test]
fn issue_1599_verifier_domain_collision() {
    merge_collision(false);
}
#[test]
fn issue_1599_verifier_member_collision() {
    merge_collision(true);
}

/// The canonical indexed-map shape from DESIGN-refinement.md:20-40 (`map
/// stock[i: ItemId] = impl_stock[i] - reserved[i]` alongside `preserve
/// progress`): an indexed per-element map used by the pulled `leadsTo`.
///
/// DESIGN-divmod.md is unrelated; this exercises issue #483's
/// `rust/fsl-verifier/src/refinement.rs` early return, which used to reject
/// *any* mapping with an indexed `state_map.binder` before even checking
/// whether the pulled `leadsTo` reads it. Reverting the indexed-substitution
/// fix (`fsl_core::substitute_expr_indexed` / the `IndexedReplacements` plumbing
/// in `refinement.rs`) makes `check_refinement_progress` return
/// `Err("indexed progress map for 'a' is not implemented")` instead of a
/// verdict, so this test fails closed.
#[test]
fn indexed_progress_map_is_pulled_through_preserve_progress() {
    let implementation = model(
        "spec Impl { type Id = 0..2 type Lv = 0..1 state { b: Map<Id, Lv> } \
         init { forall i: Id { b[i] = 0 } } \
         fair action bstep(i: Id) { b[i] = 1 - b[i] } }",
    );
    let abstraction = model(
        "spec Abs { type Id = 0..2 type Lv = 0..1 state { a: Map<Id, Lv> } \
         init { forall i: Id { a[i] = 0 } } \
         fair action step(i: Id) { a[i] = 1 - a[i] } \
         leadsTo Done { forall i: Id { a[i] == 1 ~> a[i] == 0 } } }",
    );
    let mapping = parse_refinement(
        "refinement R { impl Impl abs Abs map a[i: Id] = b[i] action bstep(i) -> step(i) \
         preserve progress { respond Done by bstep } }",
        &implementation,
        &abstraction,
    )
    .expect("build indexed mapping");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let progress = block_on(fsl_verifier::check_refinement_progress(
        &implementation,
        &abstraction,
        &mapping,
        &mut solver,
        3,
    ))
    .expect("indexed progress map must be checked, not rejected as unimplemented");
    assert!(progress.violation.is_none(), "{progress:?}");
    assert_eq!(progress.checked["Done"], vec!["bstep".to_owned()]);
}

/// Same indexed-map shape as above, but the implementation never resets
/// `b[i]` back to `0`, so the pulled-back `Done` genuinely stalls. This is
/// the negative half of the indexed-map fix: it proves the pulled-back check
/// still *detects* a real progress failure, not merely that it stopped
/// erroring. Reverting the fix makes this `Err("indexed progress map for 'a'
/// is not implemented")` too (masking the real defect); a correct but
/// over-permissive fix would wrongly report `violation.is_none()` here.
#[test]
fn indexed_progress_map_detects_a_genuine_stall() {
    let implementation = model(
        "spec Impl { type Id = 0..2 type Lv = 0..1 state { b: Map<Id, Lv> } \
         init { forall i: Id { b[i] = 0 } } \
         fair action bstep(i: Id) { requires b[i] == 0 b[i] = 1 } }",
    );
    let abstraction = model(
        "spec Abs { type Id = 0..2 type Lv = 0..1 state { a: Map<Id, Lv> } \
         init { forall i: Id { a[i] = 0 } } \
         fair action step(i: Id) { a[i] = 1 - a[i] } \
         leadsTo Done { forall i: Id { a[i] == 1 ~> a[i] == 0 } } }",
    );
    let mapping = parse_refinement(
        "refinement R { impl Impl abs Abs map a[i: Id] = b[i] action bstep(i) -> step(i) \
         preserve progress { respond Done by bstep } }",
        &implementation,
        &abstraction,
    )
    .expect("build indexed mapping");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let progress = block_on(fsl_verifier::check_refinement_progress(
        &implementation,
        &abstraction,
        &mapping,
        &mut solver,
        4,
    ))
    .expect("indexed progress map must be checked, not rejected as unimplemented");
    let violation = progress
        .violation
        .expect("b[i] never resets to 0, so Done must stall");
    assert_eq!(violation.name, "Done");
}

/// CONTROL C from issue #483: an indexed state map (`map a[i: Id] = b[i]`)
/// coexists with a `preserve progress` that only reads an unrelated scalar
/// map (`map g = h`). The pre-fix code aborted on the mere *presence* of any
/// indexed `state_map.binder` in `mapping.state_maps`, regardless of whether
/// the pulled `leadsTo` referenced it — so this must not regress back to
/// that early return.
#[test]
fn indexed_state_map_unread_by_pulled_leadsto_does_not_block_progress() {
    let implementation = model(
        "spec Impl { type Id = 0..2 type Lv = 0..1 state { b: Map<Id, Lv>, h: Lv } \
         init { forall i: Id { b[i] = 0 } h = 0 } \
         fair action bstep(i: Id) { b[i] = 1 - b[i] } \
         fair action hstep() { h = 1 - h } }",
    );
    let abstraction = model(
        "spec Abs { type Id = 0..2 type Lv = 0..1 state { a: Map<Id, Lv>, g: Lv } \
         init { forall i: Id { a[i] = 0 } g = 0 } \
         fair action step(i: Id) { a[i] = 1 - a[i] } \
         fair action gstep() { g = 1 - g } \
         leadsTo Done { g == 1 ~> g == 0 } }",
    );
    let mapping = parse_refinement(
        "refinement R { impl Impl abs Abs map a[i: Id] = b[i] map g = h \
         action bstep(i) -> step(i) action hstep() -> gstep() \
         preserve progress { respond Done by hstep } }",
        &implementation,
        &abstraction,
    )
    .expect("build mixed scalar/indexed mapping");

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let progress = block_on(fsl_verifier::check_refinement_progress(
        &implementation,
        &abstraction,
        &mapping,
        &mut solver,
        3,
    ))
    .expect("an unrelated indexed map must not block a scalar-only pulled leadsTo");
    assert!(progress.violation.is_none(), "{progress:?}");
}
