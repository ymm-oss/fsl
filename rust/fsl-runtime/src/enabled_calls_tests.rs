// SPDX-License-Identifier: Apache-2.0

//! `Monitor::enabled_calls` returns the instances and the errors of
//! `Monitor::enabled_where` for a filter that accepts exactly the calls, and
//! evaluates only the named instances of an action whose guards cannot fail
//! (#1842). The counts below are produced by the work counters, so a step
//! that goes back to evaluating every instance fails without a deadline.

use super::evaluation_counts::{GUARD_EVALUATIONS, INIT_WRITE_COMPARISONS};
use super::*;
use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn model(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse")).expect("model")
}

/// Sixty-four instances, each with a stage, a day count and a flag, read
/// by guards the way a requirements entity lowers them.
const LEAVES: &str = "spec Leaves {
  type Leave = 0..63
  type Days = 0..9
  enum Stage { Draft, Submitted, Approved }
  const LIMIT = 1
  state { stage: Map<Leave, Stage>, days: Map<Leave, Days>, staffed: Map<Leave, Bool>, open: Bool }
  init { forall l: Leave { stage[l] = Draft  days[l] = 0  staffed[l] = false } open = true }
  action submit(l: Leave, d in 0..9) {
    requires stage[l] == Draft
    requires d > 0 and d <= 3
    stage[l] = Submitted
    days[l] = d
  }
  action approve(l: Leave) {
    requires stage[l] == Submitted
    requires open
    requires not (days[l] > LIMIT)
    stage[l] = Approved
  }
  action staff(l: Leave) {
    requires stage[l] == Approved and not staffed[l]
    staffed[l] = true
  }
}";

fn params(values: &[(&str, Value)]) -> Bindings {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn counted<T>(body: impl FnOnce() -> T) -> (T, usize) {
    GUARD_EVALUATIONS.with(|count| count.set(0));
    let result = body();
    (result, GUARD_EVALUATIONS.with(std::cell::Cell::get))
}

/// What `enabled_where` gives for a filter accepting exactly `calls`: the
/// trace gate's matching before #1842.
fn filtered(
    monitor: &Monitor,
    calls: &[(&str, &Bindings)],
) -> Result<Vec<EnabledAction>, RuntimeError> {
    monitor.enabled_where(|action, params| {
        calls
            .iter()
            .any(|(name, call)| *name == action && *call == params)
    })
}

/// Detector: on 64 instances a step evaluates the guards of its own call
/// once, where the full listing evaluates every instance of every action.
#[test]
fn a_call_evaluates_only_its_own_instance_when_no_guard_can_fail() {
    let mut monitor = Monitor::new(model(LEAVES)).expect("Monitor");
    let submit = params(&[("l", Value::Int(5)), ("d", Value::Int(1))]);
    let approve = params(&[("l", Value::Int(5))]);
    let calls = [("submit", &submit)];
    let (listed, listed_count) = counted(|| filtered(&monitor, &calls));
    let (named, named_count) = counted(|| monitor.enabled_calls(&calls));
    assert_eq!(listed_count, 64 * 10 + 64 + 64);
    assert_eq!(named_count, 1);
    let named = named.expect("submit(5, 1)");
    assert_eq!(named, listed.expect("submit(5, 1)"));
    assert_eq!(named.len(), 1, "{named:?}");
    let _ = monitor.step(&named[0]).expect("step submit(5, 1)");
    for (name, call, enabled) in [
        ("approve", &approve, 1),
        ("staff", &approve, 0),
        ("submit", &submit, 0),
    ] {
        let calls = [(name, call)];
        let (named, named_count) = counted(|| monitor.enabled_calls(&calls));
        assert_eq!(named_count, 1, "{name}");
        let named = named.expect(name);
        assert_eq!(named.len(), enabled, "{name}: {named:?}");
        assert_eq!(named, filtered(&monitor, &calls).expect(name), "{name}");
    }
}

/// Preservation: an action whose guard may fail still has every instance
/// evaluated, so the step keeps the error the full listing gives, whether
/// or not the call names that action.
#[test]
fn a_guard_that_may_fail_keeps_every_instance_and_its_error() {
    let monitor = Monitor::new(model(
        "spec Undefined { type X = 0..2 state { n: 0..3 } init { n = 0 }
         action a(x: X) { requires 10 / x > 0 n = 1 }
         action b() { requires n == 0 n = 2 } }",
    ))
    .expect("Monitor");
    let listed = monitor.enabled().expect_err("a(0) divides by zero");
    let none = Bindings::new();
    let two = params(&[("x", Value::Int(2))]);
    for calls in [
        vec![("b", &none)],
        vec![("a", &two)],
        vec![("a", &two), ("b", &none)],
    ] {
        let (named, count) = counted(|| monitor.enabled_calls(&calls));
        let named = named.expect_err("a(0) divides by zero");
        assert_eq!(
            named,
            filtered(&monitor, &calls).expect_err("a(0)"),
            "{calls:?}"
        );
        assert_eq!(named.to_string(), listed.to_string(), "{calls:?}");
        // a(0) fails first, so b is never reached.
        assert_eq!(count, 1, "{calls:?}");
    }
    let total = Monitor::new(model(
        "spec Total { type X = 0..2 state { n: 0..3 } init { n = 0 }
         action a(x: X) { requires 10 / (x + 1) > 0 n = 1 }
         action b() { requires n == 0 n = 2 } }",
    ))
    .expect("Monitor");
    let calls = [("b", &none)];
    let (named, count) = counted(|| total.enabled_calls(&calls));
    assert_eq!(named, filtered(&total, &calls));
    assert_eq!(count, 3 + 1, "a may fail, so all three of its instances");
}

/// Every call shape the trace gate can pass gives what `enabled_where`
/// gives: inside and outside the domain, of another kind, with a missing or
/// extra parameter, an unknown action, a repeated call, and several actions.
#[test]
fn enabled_calls_agrees_with_enabled_where() {
    let mut monitor = Monitor::new(model(LEAVES)).expect("Monitor");
    let int = Value::Int;
    let bool_value = Value::Bool;
    let draft = |l| params(&[("l", int(l)), ("d", int(2))]);
    let shapes = [
        params(&[("l", int(0)), ("d", int(0))]),
        params(&[("l", int(63)), ("d", int(3))]),
        params(&[("l", int(64)), ("d", int(1))]),
        params(&[("l", int(-1)), ("d", int(1))]),
        params(&[("l", int(1)), ("d", int(10))]),
        params(&[("l", bool_value(true)), ("d", int(1))]),
        params(&[("l", int(1))]),
        params(&[("l", int(1)), ("d", int(1)), ("e", int(1))]),
        params(&[("m", int(1)), ("d", int(1))]),
        Bindings::new(),
    ];
    let states = 4;
    for state in 0..states {
        for name in ["submit", "approve", "staff", "missing"] {
            for call in &shapes {
                let calls = [(name, call)];
                assert_eq!(
                    monitor.enabled_calls(&calls),
                    filtered(&monitor, &calls),
                    "state {state}: {name}({call:?})"
                );
            }
        }
        let first = draft(9);
        let second = draft(4);
        let approve = params(&[("l", int(4))]);
        for calls in [
            vec![("submit", &first), ("submit", &second)],
            vec![("submit", &second), ("submit", &first)],
            vec![("submit", &first), ("submit", &first)],
            vec![
                ("approve", &approve),
                ("submit", &first),
                ("staff", &approve),
            ],
        ] {
            assert_eq!(
                monitor.enabled_calls(&calls),
                filtered(&monitor, &calls),
                "state {state}: {calls:?}"
            );
        }
        let step = match state {
            0 => ("submit", draft(4)),
            1 => ("submit", params(&[("l", int(9)), ("d", int(1))])),
            _ => ("approve", params(&[("l", int(9))])),
        };
        let enabled = monitor.enabled_calls(&[(step.0, &step.1)]).expect("step");
        if let Some(instance) = enabled.first() {
            let _ = monitor.step(instance).expect("step");
        }
    }
}

/// Negative controls: each fact `guards_cannot_fail` reads from the state is
/// broken in a state the Monitor is handed, and the step falls back to
/// every instance and keeps the full listing's error.
#[test]
fn a_state_the_guards_cannot_read_keeps_every_instance_and_its_error() {
    let leaves = model(LEAVES);
    let initial = Monitor::new(leaves.clone()).expect("Monitor").state;
    let submit = params(&[("l", Value::Int(5)), ("d", Value::Int(1))]);
    let staff = params(&[("l", Value::Int(5))]);
    let approved = Value::Enum {
        type_name: "Stage".to_owned(),
        member: "Approved".to_owned(),
    };
    let mut missing_key = initial.clone();
    let Some(Value::Map(stage)) = missing_key.get_mut("stage") else {
        panic!("stage")
    };
    stage.remove(&Value::Int(7));
    let mut extra_key = initial.clone();
    let Some(Value::Map(stage)) = extra_key.get_mut("stage") else {
        panic!("stage")
    };
    stage.remove(&Value::Int(7));
    stage.insert(Value::Int(64), approved.clone());
    let mut not_boolean = initial.clone();
    let Some(Value::Map(stage)) = not_boolean.get_mut("stage") else {
        panic!("stage")
    };
    stage.insert(Value::Int(9), approved);
    let mut unknown_map = not_boolean.clone();
    unknown_map.remove("staffed");
    let mut unknown_name = unknown_map.clone();
    unknown_name.insert("staffed".to_owned(), initial["staffed"].clone());
    unknown_name.remove("open");
    let Some(Value::Map(stage)) = unknown_name.get_mut("stage") else {
        panic!("stage")
    };
    stage.insert(
        Value::Int(9),
        Value::Enum {
            type_name: "Stage".to_owned(),
            member: "Submitted".to_owned(),
        },
    );
    let Some(Value::Map(staffed)) = not_boolean.get_mut("staffed") else {
        panic!("staffed")
    };
    staffed.insert(Value::Int(9), Value::Int(1));
    let mut not_a_map = initial;
    not_a_map.insert("stage".to_owned(), Value::Int(0));
    for (label, state, name, call, message) in [
        (
            "missing key",
            missing_key,
            "submit",
            &submit,
            "map index outside finite key domain",
        ),
        (
            "key outside the domain",
            extra_key,
            "submit",
            &submit,
            "map index outside finite key domain",
        ),
        ("not a Bool", not_boolean, "staff", &staff, "expected Bool"),
        (
            "not a map",
            not_a_map,
            "submit",
            &submit,
            "indexing requires a map or sequence",
        ),
        (
            "unknown map",
            unknown_map,
            "staff",
            &staff,
            "unknown identifier 'staffed'",
        ),
        (
            "unknown name",
            unknown_name,
            "approve",
            &staff,
            "unknown identifier 'open'",
        ),
    ] {
        let monitor = Monitor::from_state(leaves.clone(), state);
        let calls = [(name, call)];
        let listed = filtered(&monitor, &calls).expect_err(label);
        let named = monitor.enabled_calls(&calls).expect_err(label);
        assert_eq!(named, listed, "{label}");
        assert!(named.to_string().contains(message), "{label}: {named}");
    }
}

/// Negative control: a `let` that shadows a state map is what an index of
/// that name reads, so the state map's coverage says nothing about it.
#[test]
fn a_let_shadowing_a_state_map_keeps_every_instance_and_its_error() {
    let shadowing = model(
        "spec Shadowing { type L = 0..3 type D = 0..9
         state { stage: Map<L, Bool>, days: Map<L, D> }
         init { forall l: L { stage[l] = true  days[l] = 0 } }
         action a(l: L) { let stage = days  requires stage[l] == 0  days[l] = 1 } }",
    );
    let mut short_days = Monitor::new(shadowing.clone()).expect("Monitor").state;
    let Some(Value::Map(days)) = short_days.get_mut("days") else {
        panic!("days")
    };
    days.remove(&Value::Int(2));
    let monitor = Monitor::from_state(shadowing, short_days);
    let first = params(&[("l", Value::Int(0))]);
    let calls = [("a", &first)];
    let named = monitor
        .enabled_calls(&calls)
        .expect_err("a(2) reads the let, not the state map");
    assert_eq!(named, filtered(&monitor, &calls).expect_err("a(2)"));
    assert!(
        named
            .to_string()
            .contains("map index outside finite key domain"),
        "{named}"
    );
}

/// Detector: the init duplicate-write check compares a key only with the
/// keys written before it to the same state variable, so three maps over 64
/// keys need no comparison at all, and a key written twice is still found.
#[test]
fn init_duplicate_writes_are_compared_within_one_state_variable() {
    INIT_WRITE_COMPARISONS.with(|count| count.set(0));
    let _ = Monitor::new(model(LEAVES)).expect("Monitor");
    assert_eq!(INIT_WRITE_COMPARISONS.with(std::cell::Cell::get), 0);
    let twice = LEAVES.replace("open = true }", "open = true  stage[3] = Submitted }");
    assert_ne!(twice, LEAVES);
    INIT_WRITE_COMPARISONS.with(|count| count.set(0));
    let error = Monitor::new(model(&twice)).expect_err("stage[3] is written twice");
    assert!(
        error
            .to_string()
            .starts_with("state variable 'stage' assigned more than once in init"),
        "{error}"
    );
    assert!(INIT_WRITE_COMPARISONS.with(std::cell::Cell::get) > 0);
}
