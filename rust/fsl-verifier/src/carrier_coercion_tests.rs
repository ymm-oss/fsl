// SPDX-License-Identifier: Apache-2.0

//! #1683: `coerce` asked directly, past `check`, so a fault in the shared carrier rule
//! shows on this side without `check` refusing the form first. The value a pair coerces
//! to is compared with the same value built at the target type, by the solver and not
//! by the rule.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{
    FsResolver, FslValue, KernelModel, TypeDef, TypeRef, build_model, parse_kernel_source,
};
use fsl_solver::{SatResult, SmtSolver};

use crate::value::{SymbolicValue, coerce, concrete_value, logical_equal};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model() -> KernelModel {
    let source = "spec T {
  type K = 0..1
  type A = 0..1
  type A2 = 0..1
  type B = 0..2
  enum E { e0, e1 }
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

/// One present element at every level, so that a rebuild that drops or changes a member
/// differs from the value built at the target type.
fn sample(ty: &TypeRef, model: &KernelModel) -> FslValue {
    match ty {
        TypeRef::Option(item) => FslValue::Some(Box::new(sample(item, model))),
        TypeRef::Set(item) => FslValue::Set([sample(item, model)].into()),
        TypeRef::Seq(item, _) => FslValue::Seq(vec![sample(item, model)]),
        TypeRef::Map(key, value) => FslValue::Map(
            model
                .map_key_values(key)
                .expect("finite key")
                .into_iter()
                .map(|key| (key, sample(value, model)))
                .collect(),
        ),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Enum { .. }) => FslValue::Enum {
                type_name: name.clone(),
                member: "e1".to_owned(),
            },
            _ => FslValue::Int(1),
        },
        TypeRef::Bool => FslValue::Bool(true),
        _ => FslValue::Int(1),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shape {
    Set,
    Collection,
    Seq,
    Option,
    Map,
    Scalar,
    Other,
}

fn shape<T>(value: &SymbolicValue<T>) -> Shape {
    match value {
        SymbolicValue::Set { .. } => Shape::Set,
        SymbolicValue::Collection { .. } => Shape::Collection,
        SymbolicValue::Seq { .. } => Shape::Seq,
        SymbolicValue::Option { .. } => Shape::Option,
        SymbolicValue::Map { .. } => Shape::Map,
        SymbolicValue::Scalar { .. } => Shape::Scalar,
        _ => Shape::Other,
    }
}

/// The value carries the target's type, and every element it holds that carrier: a
/// value that is only retagged at the top keeps its elements' `Int`.
fn assert_typed_throughout<T>(
    value: &SymbolicValue<T>,
    ty: &TypeRef,
    model: &KernelModel,
    label: &str,
) {
    let actual = value.ty().unwrap_or_else(|| panic!("{label}: untyped"));
    assert!(
        model.same_carrier(actual, ty).expect("known types"),
        "{label}: {actual:?} holds no {ty:?}"
    );
    match (value, ty) {
        (SymbolicValue::Collection { entries, .. }, TypeRef::Set(item)) => {
            for (key, _) in entries {
                assert_typed_throughout(&key[0], item, model, label);
            }
        }
        (SymbolicValue::Seq { slots, .. }, TypeRef::Seq(item, _)) => {
            for slot in slots {
                assert_typed_throughout(slot, item, model, label);
            }
        }
        (SymbolicValue::Option { value, .. }, TypeRef::Option(item)) => {
            assert_typed_throughout(value, item, model, label);
        }
        _ => {}
    }
}

/// `coerce` keeps this value's representation when the carriers agree, and otherwise
/// rebuilds it into the target's, with the same contents.
#[test]
fn coercible_pairs_keep_their_contents_and_carry_the_target_type() {
    let (int, a, a2, b) = (TypeRef::Int, named("A"), named("A2"), named("B"));
    let rows = [
        // The same carrier keeps its representation.
        (set(a.clone()), set(a.clone()), Shape::Set),
        (set(a.clone()), set(a2.clone()), Shape::Set),
        (set(int.clone()), set(int.clone()), Shape::Collection),
        (seq(a.clone(), 2), seq(a2.clone(), 2), Shape::Seq),
        (seq(int.clone(), 2), seq(int.clone(), 2), Shape::Seq),
        (
            map(named("K"), a.clone()),
            map(named("K"), a2.clone()),
            Shape::Map,
        ),
        (
            map(named("K"), int.clone()),
            map(named("K"), int.clone()),
            Shape::Map,
        ),
        (named("E"), named("E"), Shape::Scalar),
        // Scalars and Options answer before the rule, and an Option's value through it.
        (a.clone(), b.clone(), Shape::Scalar),
        (TypeRef::Int, a.clone(), Shape::Scalar),
        (option(a.clone()), option(b.clone()), Shape::Option),
        (option(int.clone()), option(a.clone()), Shape::Option),
        (
            option(set(int.clone())),
            option(set(a.clone())),
            Shape::Option,
        ),
        // `Int` below a Set or Seq rebuilds.
        (set(int.clone()), set(a.clone()), Shape::Collection),
        (set(a.clone()), set(int.clone()), Shape::Collection),
        (
            set(option(int.clone())),
            set(option(a.clone())),
            Shape::Collection,
        ),
        (
            set(set(int.clone())),
            set(set(a.clone())),
            Shape::Collection,
        ),
        (seq(int.clone(), 2), seq(a.clone(), 2), Shape::Seq),
        (seq(int.clone(), 2), seq(int.clone(), 3), Shape::Seq),
        (seq(int.clone(), 2), seq(a.clone(), 3), Shape::Seq),
        (seq(a.clone(), 2), seq(int.clone(), 3), Shape::Seq),
        (
            seq(option(int.clone()), 2),
            seq(option(a.clone()), 2),
            Shape::Seq,
        ),
    ];
    let model = model();
    for (actual, target, expected_shape) in rows {
        let label = format!("{actual:?} to {target:?}");
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("solver");
        let value = concrete_value(&solver, &model, &actual, &sample(&actual, &model))
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        let coerced = coerce(&solver, &model, value, &target)
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(shape(&coerced), expected_shape, "{label}");
        assert_eq!(coerced.ty(), Some(&target), "{label}");
        assert_typed_throughout(&coerced, &target, &model, &label);
        let at_target = concrete_value(&solver, &model, &target, &sample(&target, &model))
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        let equal = logical_equal(&solver, &model, &coerced, &at_target)
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        solver
            .assert(&solver.not(&equal).expect("not"))
            .expect("assert");
        assert_eq!(
            block_on(solver.check()).expect("check"),
            SatResult::Unsat,
            "{label}: coercion changed the contents"
        );
    }
}

/// These report `cannot coerce`, naming the value's own type. `check` refuses each
/// outside a position it leaves to the prior rule, so `coerce` is the only thing that
/// can be asked about them here.
#[test]
fn pairs_the_rule_refuses_report_cannot_coerce() {
    let (int, a, b, e) = (TypeRef::Int, named("A"), named("B"), named("E"));
    let key = named("K");
    let rows = [
        (set(a.clone()), set(b.clone())),
        (set(a.clone()), seq(a.clone(), 2)),
        (seq(a.clone(), 2), seq(b.clone(), 2)),
        (seq(a.clone(), 1), seq(a.clone(), 2)),
        (set(option(a.clone())), set(option(b.clone()))),
        (option(set(a.clone())), option(set(b.clone()))),
        (map(key.clone(), int.clone()), map(key.clone(), a.clone())),
        (map(key.clone(), a.clone()), map(key.clone(), int.clone())),
        (map(key.clone(), a.clone()), map(key.clone(), b.clone())),
        (set(int.clone()), set(e.clone())),
        (set(int.clone()), set(TypeRef::Bool)),
        (set(e.clone()), set(int.clone())),
        (seq(int.clone(), 2), seq(TypeRef::Bool, 2)),
        (a.clone(), set(a.clone())),
        (set(a.clone()), a.clone()),
    ];
    let model = model();
    for (actual, target) in rows {
        let label = format!("{actual:?} to {target:?}");
        let solver = fsl_solver_z3::Z3Solver::new().expect("solver");
        let value = concrete_value(&solver, &model, &actual, &sample(&actual, &model))
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        let error = coerce(&solver, &model, value, &target)
            .map(|_| ())
            .expect_err(&label);
        assert!(
            error
                .to_string()
                .starts_with("cannot coerce symbolic value"),
            "{label}: {error:?}"
        );
    }
}
