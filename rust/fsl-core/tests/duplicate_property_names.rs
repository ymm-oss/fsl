// SPDX-License-Identifier: Apache-2.0

//! Regression for #1192: two properties with the same name were accepted, and
//! every result keyed by that name collapsed them — the induction engine
//! reported a false `leadsTo` as `proved`. Property names share one namespace
//! across `invariant`, `trans`, `unless`, `reachable`, `leadsTo`, and `until`
//! (including the `<name>_until_safety` trans it lowers to); a reuse is a
//! `semantics` error located at the later declaration.

use fsl_core::{FsResolver, build_model, parse_kernel_source};

const HEADER: &str = "spec Dup {
  state { x: 0..1, y: 0..5 }
  init { x = 0  y = 0 }
  action flip() { requires y == 5 x = 1 - x }
  action inc() { requires y < 5 y = y + 1 }
";

fn build(properties: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let source = format!("{HEADER}{properties}}}\n");
    let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("spec lowers");
    build_model(kernel)
}

/// The rejection of `properties`, whose second declaration is on line 7.
fn rejection(properties: &str) -> (String, (u32, u32)) {
    let error = build(properties).expect_err("a reused property name must be rejected");
    let span = error.span.expect("the error is located");
    (error.message, (span.start.line, span.start.column))
}

#[test]
fn duplicate_leadsto_is_rejected_at_the_second_declaration() {
    let (message, at) = rejection(
        "  leadsTo L { y < 5 ~> y == 5 decreases 5 - y }
  leadsTo L { x == 0 ~> x == 2 }
",
    );
    assert_eq!(
        message,
        "duplicate property name 'L': leadsTo reuses the name of the leadsTo declared at 6:3"
    );
    assert_eq!(at, (7, 3));
}

#[test]
fn duplicate_invariant_is_rejected() {
    let (message, at) = rejection(
        "  invariant I { x <= 1 }
  invariant I { x == 0 }
",
    );
    assert_eq!(
        message,
        "duplicate property name 'I': invariant reuses the name of the invariant declared at 6:3"
    );
    assert_eq!(at, (7, 3));
}

#[test]
fn duplicate_trans_and_reachable_are_rejected() {
    let (message, _) = rejection(
        "  trans T { x <= 1 }
  trans T { x == 0 }
",
    );
    assert!(message.starts_with("duplicate property name 'T': trans reuses"));
    let (message, _) = rejection(
        "  reachable R { x == 1 }
  reachable R { y == 9 }
",
    );
    assert!(message.starts_with("duplicate property name 'R': reachable reuses"));
}

#[test]
fn a_name_reused_across_property_kinds_is_rejected() {
    let (message, at) = rejection(
        "  invariant X { x <= 1 }
  reachable X { x == 1 }
",
    );
    assert_eq!(
        message,
        "duplicate property name 'X': reachable reuses the name of the invariant declared at 6:3"
    );
    assert_eq!(at, (7, 3));
    let (message, _) = rejection(
        "  trans U { x <= 1 }
  unless U { x == 0 unless x == 1 }
",
    );
    assert!(
        message.starts_with("duplicate property name 'U': unless reuses the name of the trans")
    );
}

#[test]
fn until_claims_its_leadsto_and_safety_names() {
    let (message, _) = rejection(
        "  until W { y < 5 until y == 5 }
  leadsTo W { y < 5 ~> y == 5 }
",
    );
    assert!(
        message.starts_with("duplicate property name 'W': leadsTo reuses the name of the until")
    );
    let (message, at) = rejection(
        "  trans W_until_safety { x <= 1 }
  until W { y < 5 until y == 5 }
",
    );
    assert_eq!(
        message,
        "duplicate property name 'W_until_safety': until reuses the name of the trans declared at 6:3"
    );
    assert_eq!(at, (7, 3));
}

/// Negative control: distinct names of every kind, including the names an
/// `until` lowers to, still build.
#[test]
fn distinct_property_names_are_accepted() {
    let model = build(
        "  invariant I { x <= 1 }
  trans T { x <= 1 }
  unless N { x == 0 unless x == 1 }
  reachable R { x == 1 }
  leadsTo L { y < 5 ~> y == 5 decreases 5 - y }
  until W { y < 5 until y == 5 }
  leadsTo L2 { x == 0 ~> x == 2 }
",
    )
    .expect("distinct property names build");
    assert_eq!(model.leadstos.len(), 3);
    assert_eq!(model.transitions.len(), 3);
}
