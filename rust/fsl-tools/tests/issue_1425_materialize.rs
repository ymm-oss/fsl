// SPDX-License-Identifier: Apache-2.0

//! API pins for #1425's private `Item`, `Part` and `RemovePart` edit variants.
//! Expected ASTs are hand-edited clones, retaining every unaffected span and
//! field. These tests intentionally do not compile before materialize exists.

use fsl_syntax::{ActionItem, Expr, SpecItem, SurfaceSpec, parse_surface_spec};
use fsl_tools::enumerate_builtin_mutants;

fn base() -> SurfaceSpec {
    parse_surface_spec(
        r"
spec Materialize {
  const FIRST = 10
  const SECOND = 20
  state { x: Bool }
  init { x = false }
  fair action stay() {
    let limit = 10
    requires limit == 10
    requires not x
    x = x
  }
  invariant Stable { not x }
}
",
    )
    .expect("parse base")
}

fn assert_materializes(base: &SurfaceSpec, expected: &SurfaceSpec, op: &str, target: &str) {
    let unchanged = base.clone();
    let mutants = enumerate_builtin_mutants(base);
    let matching = mutants
        .iter()
        .filter(|mutant| mutant.op == op && mutant.target == target)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "select exactly one {op} at {target}");
    assert_ne!(expected, base, "hand edit must change the base");
    let mutant = matching[0];
    assert_eq!(&mutant.materialize(base), expected);
    assert_eq!(
        base, &unchanged,
        "materialize must leave its base unchanged"
    );
    // The descriptor is reusable, not a consumable or stateful edit.
    assert_eq!(&mutant.materialize(base), expected);
    assert_eq!(base, &unchanged);
}

#[test]
fn item_replaces_only_the_selected_top_level_item() {
    let base = base();
    let mut expected = base.clone();
    let SpecItem::Const { value, .. } = &mut expected.items[1] else {
        panic!("second const");
    };
    **value = Expr::Num(21);
    assert_materializes(&base, &expected, "integer_literal_plus1", "const SECOND");
}

#[test]
fn part_replaces_only_the_selected_action_part() {
    let base = base();
    let mut expected = base.clone();
    let SpecItem::Action { items, .. } = &mut expected.items[4] else {
        panic!("stay action");
    };
    let ActionItem::Requires(expr, _) = &mut items[2] else {
        panic!("second guard after a let binding");
    };
    *expr = Expr::Not(Box::new(expr.clone()));
    assert_materializes(&base, &expected, "requires_negate", "stay requires #2");
}

#[test]
fn remove_part_removes_only_the_selected_action_part() {
    let base = base();
    let mut expected = base.clone();
    let SpecItem::Action { items, .. } = &mut expected.items[4] else {
        panic!("stay action");
    };
    items.remove(2);
    assert_materializes(&base, &expected, "requires_remove", "stay requires #2");
}

#[test]
fn fair_remove_preserves_the_action_except_for_its_fairness() {
    let base = base();
    let mut expected = base.clone();
    let SpecItem::Action { fair, .. } = &mut expected.items[4] else {
        panic!("stay action");
    };
    assert!(*fair, "base action must be fair");
    *fair = false;
    assert_materializes(&base, &expected, "fair_remove", "stay fair");
}

#[test]
fn let_mutation_preserves_the_binding_name_span_and_other_parts() {
    let base = base();
    let mut expected = base.clone();
    let SpecItem::Action { items, .. } = &mut expected.items[4] else {
        panic!("stay action");
    };
    let ActionItem::Let(_, expr, _) = &mut items[0] else {
        panic!("let binding");
    };
    *expr = Expr::Num(11);
    assert_materializes(&base, &expected, "integer_literal_plus1", "stay let limit");
}
