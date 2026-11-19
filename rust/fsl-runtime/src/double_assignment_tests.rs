// SPDX-License-Identifier: Apache-2.0

//! `check` rejects an action that assigns one state location twice, so a
//! model reaches the runtime's own guard only when its statements are built
//! by hand. These tests duplicate an assignment after the model is built and
//! pin which second writes the guard refuses and the message it gives.

use super::*;
use fsl_core::{FsResolver, build_model, parse_kernel_source};

const SOURCE: &str = "spec Writes {
  type K = 0..1
  struct P { f: K, g: K }
  state { x: K, m: Map<K, K>, p: P, ps: Map<K, P> }
  init {
    x = 0
    forall k: K { m[k] = 0 }
    p = P { f: 0, g: 0 }
    forall k: K { ps[k] = P { f: 0, g: 0 } }
  }
  action var(i: K, j: K) { x = 1 }
  action index(i: K, j: K) { m[i] = 1 }
  action field(i: K, j: K) { p.f = 1 }
  action index_field(i: K, j: K) { ps[i].f = 1 }
  action fields(i: K, j: K) { p.f = 1 p.g = 1 }
  action index_fields(i: K, j: K) { ps[i].f = 1 ps[i].g = 1 }
}";

/// Step `action` with `i` and `j` after appending a second assignment, made
/// from its first by writing `j` where it wrote `i`.
fn step_twice(action: &str, i: i64, j: i64) -> Result<StepResult, RuntimeError> {
    let mut model = model();
    let definition = model
        .actions
        .iter_mut()
        .find(|definition| definition.name == action)
        .expect("action");
    let mut second = definition.statements[0].clone();
    if let Statement::Assign { target, .. } = &mut second {
        let index = match target {
            LValue::Index(_, index) => Some(index),
            LValue::Field(base, _) => match base.as_mut() {
                LValue::Index(_, index) => Some(index),
                _ => None,
            },
            LValue::Var(_) => None,
        };
        if let Some(index) = index {
            *index = Expr::Var("j".to_owned());
        }
    }
    definition.statements.push(second);
    step(model, action, i, j)
}

fn model() -> KernelModel {
    build_model(parse_kernel_source(SOURCE, &FsResolver::new(".")).expect("parse")).expect("model")
}

/// Step `action` of `model` with `i` and `j`.
fn step(model: KernelModel, action: &str, i: i64, j: i64) -> Result<StepResult, RuntimeError> {
    let mut monitor = Monitor::new(model).expect("monitor");
    let enabled = monitor
        .enabled()
        .expect("enabled")
        .into_iter()
        .find(|enabled| {
            enabled.action == action
                && enabled.params.get("i") == Some(&Value::Int(i))
                && enabled.params.get("j") == Some(&Value::Int(j))
        })
        .expect("instance");
    monitor.step(&enabled)
}

#[test]
fn a_second_write_to_one_location_names_it() {
    for (action, location) in [
        ("var", "x"),
        ("index", "m[Int(1)]"),
        ("field", "p.f"),
        ("index_field", "ps[Int(1)].f"),
    ] {
        let error = step_twice(action, 1, 1).expect_err(action);
        assert_eq!(
            error.message,
            format!("double assignment to '{location}' on the same execution path"),
            "{action}"
        );
    }
}

#[test]
fn writes_to_two_indices_are_two_locations() {
    for action in ["index", "index_field"] {
        let stepped = step_twice(action, 0, 1).expect(action);
        assert_eq!(stepped.violation, None, "{action}");
    }
}

#[test]
fn writes_to_two_fields_are_two_locations() {
    for action in ["fields", "index_fields"] {
        let stepped = step(model(), action, 0, 0).expect(action);
        assert_eq!(stepped.violation, None, "{action}");
    }
}

/// The grammar has no `p.f.g` target; one built by hand is refused with
/// this message.
#[test]
fn a_nested_field_target_is_refused() {
    let mut model = model();
    let definition = model
        .actions
        .iter_mut()
        .find(|definition| definition.name == "field")
        .expect("action");
    let Statement::Assign { target, .. } = &mut definition.statements[0] else {
        panic!("assignment");
    };
    *target = LValue::Field(Box::new(target.clone()), "g".to_owned());
    let error = step(model, "field", 0, 0).expect_err("nested field target");
    assert_eq!(error.message, "nested field lvalue");
}
