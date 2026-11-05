// SPDX-License-Identifier: Apache-2.0

//! #1683: the carrier rule `check` and symbolic coercion both answer from. Every row
//! is written by hand and calls no other rule, because a test that derives its
//! expectation from the rule cannot detect a wrong rule.

use fsl_core::{
    CarrierCoercion, FsResolver, KernelModel, TypeRef, build_model, parse_kernel_source,
};

use CarrierCoercion::{MapMismatch, NotAssignable, RebuildInt, Retag};

fn model() -> KernelModel {
    let source = "spec T {
  type K = 0..1
  type K2 = 0..1
  type A = 0..1
  type A2 = 0..1
  type B = 0..2
  enum E { e0, e1 }
  enum F { f0, f1 }
  struct P { x: A }
  struct Q { x: A }
  state { x: A }
  init { x = 0 }
  action idle() { }
}";
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse")).expect("build")
}

fn named(name: &str) -> TypeRef {
    TypeRef::Named(name.to_owned())
}
fn set(item: TypeRef) -> TypeRef {
    TypeRef::Set(Box::new(item))
}
fn seq(item: TypeRef, capacity: usize) -> TypeRef {
    TypeRef::Seq(Box::new(item), capacity)
}
fn option(item: TypeRef) -> TypeRef {
    TypeRef::Option(Box::new(item))
}
fn map(key: TypeRef, value: TypeRef) -> TypeRef {
    TypeRef::Map(Box::new(key), Box::new(value))
}
fn relation(source: TypeRef, target: TypeRef) -> TypeRef {
    TypeRef::Relation(Box::new(source), Box::new(target))
}

#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
fn rows() -> Vec<(TypeRef, TypeRef, CarrierCoercion)> {
    let (int, boolean) = (TypeRef::Int, TypeRef::Bool);
    let (k, k2, a, a2, b) = (named("K"), named("K2"), named("A"), named("A2"), named("B"));
    let (e, f, p, q) = (named("E"), named("F"), named("P"), named("Q"));
    vec![
        // Two scalars of one kind keep their one term, whatever carrier labels it.
        (int.clone(), int.clone(), Retag),
        (int.clone(), a.clone(), Retag),
        (a.clone(), int.clone(), Retag),
        (a.clone(), b.clone(), Retag),
        (a.clone(), TypeRef::Range(0, 1), Retag),
        (boolean.clone(), boolean.clone(), Retag),
        // Scalars of different kinds.
        (boolean.clone(), int.clone(), NotAssignable),
        (int.clone(), boolean.clone(), NotAssignable),
        (a.clone(), boolean.clone(), NotAssignable),
        (e.clone(), int.clone(), NotAssignable),
        (a.clone(), e.clone(), NotAssignable),
        // Names are nominal.
        (e.clone(), e.clone(), Retag),
        (e.clone(), f.clone(), NotAssignable),
        (p.clone(), p.clone(), Retag),
        (p.clone(), q.clone(), NotAssignable),
        (p.clone(), e.clone(), NotAssignable),
        // Shapes that differ.
        (set(a.clone()), seq(a.clone(), 2), NotAssignable),
        (seq(a.clone(), 2), option(a.clone()), NotAssignable),
        (option(a.clone()), a.clone(), NotAssignable),
        (a.clone(), option(a.clone()), NotAssignable),
        (map(k.clone(), a.clone()), set(a.clone()), NotAssignable),
        (
            relation(a.clone(), a.clone()),
            set(a.clone()),
            NotAssignable,
        ),
        // A Set of a finite carrier: the whole carrier must agree.
        (set(a.clone()), set(a.clone()), Retag),
        (set(a.clone()), set(a2.clone()), Retag),
        (set(a.clone()), set(b.clone()), NotAssignable),
        (set(e.clone()), set(e.clone()), Retag),
        (set(a.clone()), set(e.clone()), NotAssignable),
        (set(option(a.clone())), set(option(a2.clone())), Retag),
        (
            set(option(a.clone())),
            set(option(b.clone())),
            NotAssignable,
        ),
        // A Set holding `Int` rebuilds against a different carrier and retags against
        // its own.
        (set(int.clone()), set(int.clone()), Retag),
        (set(int.clone()), set(a.clone()), RebuildInt),
        (set(a.clone()), set(int.clone()), RebuildInt),
        (set(int.clone()), set(boolean.clone()), NotAssignable),
        (set(boolean.clone()), set(int.clone()), NotAssignable),
        (set(int.clone()), set(e.clone()), NotAssignable),
        (set(int.clone()), set(option(a.clone())), NotAssignable),
        (set(option(int.clone())), set(option(int.clone())), Retag),
        (set(option(int.clone())), set(option(a.clone())), RebuildInt),
        (set(set(int.clone())), set(set(a.clone())), RebuildInt),
        (set(set(a.clone())), set(set(int.clone())), RebuildInt),
        (set(set(int.clone())), set(set(int.clone())), Retag),
        (
            set(seq(int.clone(), 2)),
            set(seq(int.clone(), 3)),
            RebuildInt,
        ),
        (set(seq(int.clone(), 2)), set(seq(a.clone(), 3)), RebuildInt),
        // A Seq of a finite carrier: capacity is part of the carrier.
        (seq(a.clone(), 2), seq(a.clone(), 2), Retag),
        (seq(a.clone(), 2), seq(a2.clone(), 2), Retag),
        (seq(a.clone(), 2), seq(b.clone(), 2), NotAssignable),
        (seq(a.clone(), 2), seq(a.clone(), 3), NotAssignable),
        (seq(a.clone(), 2), seq(b.clone(), 3), NotAssignable),
        // A Seq holding `Int` ignores capacity on either side.
        (seq(int.clone(), 2), seq(int.clone(), 2), Retag),
        (seq(int.clone(), 2), seq(int.clone(), 3), RebuildInt),
        (seq(int.clone(), 2), seq(a.clone(), 2), RebuildInt),
        (seq(int.clone(), 2), seq(a.clone(), 3), RebuildInt),
        (seq(a.clone(), 2), seq(int.clone(), 3), RebuildInt),
        (seq(int.clone(), 2), seq(boolean.clone(), 2), NotAssignable),
        (
            seq(option(int.clone()), 2),
            seq(option(a.clone()), 3),
            RebuildInt,
        ),
        // An Option's value is always rebuilt, so different scalars below it are
        // assignable, unlike the same scalars below a Set or Seq.
        (option(a.clone()), option(a.clone()), Retag),
        (option(a.clone()), option(a2.clone()), Retag),
        (option(a.clone()), option(b.clone()), RebuildInt),
        (option(int.clone()), option(a.clone()), RebuildInt),
        (option(a.clone()), option(e.clone()), NotAssignable),
        (option(boolean.clone()), option(int.clone()), NotAssignable),
        (option(set(int.clone())), option(set(a.clone())), RebuildInt),
        (option(set(a.clone())), option(set(a.clone())), Retag),
        (
            option(set(a.clone())),
            option(set(b.clone())),
            NotAssignable,
        ),
        (
            option(seq(a.clone(), 2)),
            option(seq(a.clone(), 3)),
            NotAssignable,
        ),
        // A Map only retags. Different carriers are assignable through `Int` alone.
        (map(k.clone(), a.clone()), map(k.clone(), a.clone()), Retag),
        (
            map(k.clone(), a.clone()),
            map(k2.clone(), a2.clone()),
            Retag,
        ),
        (
            map(k.clone(), int.clone()),
            map(k.clone(), int.clone()),
            Retag,
        ),
        (
            map(k.clone(), int.clone()),
            map(k.clone(), a.clone()),
            MapMismatch,
        ),
        (
            map(k.clone(), a.clone()),
            map(k.clone(), int.clone()),
            MapMismatch,
        ),
        (
            map(k.clone(), option(int.clone())),
            map(k.clone(), option(a.clone())),
            MapMismatch,
        ),
        (
            map(k.clone(), a.clone()),
            map(k.clone(), b.clone()),
            NotAssignable,
        ),
        (
            map(k.clone(), a.clone()),
            map(b.clone(), a.clone()),
            NotAssignable,
        ),
        (
            map(k.clone(), option(a.clone())),
            map(k.clone(), option(b.clone())),
            NotAssignable,
        ),
        (
            map(k.clone(), boolean.clone()),
            map(k.clone(), int.clone()),
            NotAssignable,
        ),
        (
            map(int.clone(), a.clone()),
            map(k.clone(), a.clone()),
            MapMismatch,
        ),
        // The mismatch answers through anything that holds the Map.
        (
            option(map(k.clone(), int.clone())),
            option(map(k.clone(), a.clone())),
            MapMismatch,
        ),
        (
            set(map(k.clone(), int.clone())),
            set(map(k.clone(), a.clone())),
            MapMismatch,
        ),
        (
            seq(map(k.clone(), int.clone()), 2),
            seq(map(k.clone(), a.clone()), 3),
            MapMismatch,
        ),
        (
            set(map(k.clone(), int.clone())),
            set(map(k.clone(), int.clone())),
            Retag,
        ),
        (
            set(map(k.clone(), a.clone())),
            set(map(k.clone(), b.clone())),
            NotAssignable,
        ),
        // A relation's two carriers must each agree, `Int` or not.
        (
            relation(a.clone(), b.clone()),
            relation(a2.clone(), TypeRef::Range(0, 2)),
            Retag,
        ),
        (
            relation(a.clone(), a.clone()),
            relation(b.clone(), a.clone()),
            NotAssignable,
        ),
        (
            relation(a.clone(), a.clone()),
            relation(a.clone(), b.clone()),
            NotAssignable,
        ),
        (
            relation(int.clone(), a.clone()),
            relation(a.clone(), a.clone()),
            NotAssignable,
        ),
    ]
}

#[test]
fn every_pair_of_carriers_answers_as_written() {
    let model = model();
    let mut wrong = Vec::new();
    for (actual, expected, answer) in rows() {
        let observed = model
            .carrier_coercion(&actual, &expected)
            .unwrap_or_else(|error| panic!("{actual:?} to {expected:?}: {error:?}"));
        if observed != answer {
            wrong.push(format!(
                "{actual:?} to {expected:?}: wrote {answer:?}, got {observed:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn an_unknown_name_is_an_error_rather_than_an_answer() {
    let model = model();
    for (actual, expected) in [
        (named("Missing"), TypeRef::Int),
        (TypeRef::Int, named("Missing")),
        (set(named("Missing")), set(named("A"))),
        (set(named("A")), set(named("Missing"))),
        (
            map(named("K"), named("Missing")),
            map(named("K"), TypeRef::Int),
        ),
    ] {
        let error = model
            .carrier_coercion(&actual, &expected)
            .expect_err("an unknown type has no answer");
        assert!(error.message.contains("Missing"), "{error:?}");
    }
}
