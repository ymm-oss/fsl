// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FsResolver, FslValue, KernelBinder, KernelExpr, build_model, parse_kernel_source};
use fsl_runtime::{BoundedLivenessMonitor, Monitor};

fn checked_model(source: &str) -> fsl_core::KernelModel {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse kernel");
    build_model(kernel).expect("build model")
}

fn step(monitor: &mut Monitor, action: &str, params: &BTreeMap<String, FslValue>) {
    let result = monitor.attempt(action, params).expect("attempt action");
    assert!(result.violation.is_none(), "{result:?}");
}

const LATE_Q: &str = r"
spec LateQ {
  type Count = 0..3
  state { x: Count }
  init { x = 0 }
  action step() {
    requires x < 3
    x = x + 1
  }
  leadsTo Progress { x == 1 ~> within 1 x == 3 }
}
";

#[test]
fn deadline_is_inclusive_and_a_late_response_is_reported_at_the_deadline() {
    let model = checked_model(LATE_Q);
    let mut state = Monitor::new(model.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(model).expect("liveness monitor");
    assert!(
        liveness
            .observe(&state.state, 0)
            .expect("initial")
            .is_none()
    );

    step(&mut state, "step", &BTreeMap::new());
    assert!(
        liveness
            .observe(&state.state, 1)
            .expect("trigger")
            .is_none()
    );
    let pending = liveness.status().pending;
    assert_eq!(pending[0].pending_since, 1);
    assert_eq!(pending[0].deadline, 2);

    step(&mut state, "step", &BTreeMap::new());
    let violation = liveness
        .observe(&state.state, 2)
        .expect("deadline")
        .expect("missed deadline");
    assert_eq!(violation.property, "Progress");
    assert_eq!(violation.pending_since, 1);
    assert_eq!(violation.deadline, 2);
    assert_eq!(violation.within, 1);
}

#[test]
fn response_on_the_deadline_and_simultaneous_zero_window_response_satisfy() {
    let model = checked_model(&LATE_Q.replace("within 1", "within 2"));
    let mut state = Monitor::new(model.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(model).expect("liveness monitor");
    liveness.observe(&state.state, 0).expect("initial");
    for observation in 1..=3 {
        step(&mut state, "step", &BTreeMap::new());
        assert!(
            liveness
                .observe(&state.state, observation)
                .expect("observation")
                .is_none()
        );
    }
    assert!(liveness.status().pending.is_empty());

    let simultaneous = checked_model(
        r"
spec Simultaneous {
  state { ready: Bool }
  init { ready = true }
  leadsTo Already { ready ~> within 0 ready }
}
",
    );
    let state = Monitor::new(simultaneous.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(simultaneous).expect("liveness monitor");
    assert!(
        liveness
            .observe(&state.state, 0)
            .expect("initial")
            .is_none()
    );
}

#[test]
fn zero_window_failure_and_each_static_binding_are_checked() {
    let zero = checked_model(
        r"
spec ZeroWindow {
  state { ready: Bool }
  init { ready = false }
  leadsTo Immediate { not ready ~> within 0 ready }
}
",
    );
    let state = Monitor::new(zero.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(zero).expect("liveness monitor");
    let violation = liveness
        .observe(&state.state, 0)
        .expect("initial")
        .expect("zero-window failure");
    assert_eq!(violation.deadline, 0);

    let bound = checked_model(
        r"
spec Bound {
  type Id = 0..1
  state { done: Map<Id, Bool> }
  init { forall i: Id { done[i] = false } }
  action complete(i: Id) { done[i] = true }
  leadsTo Each { forall i: Id { not done[i] ~> within 1 done[i] } }
}
",
    );
    let mut state = Monitor::new(bound.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(bound).expect("liveness monitor");
    liveness.observe(&state.state, 0).expect("initial");
    step(
        &mut state,
        "complete",
        &BTreeMap::from([("i".to_owned(), FslValue::Int(0))]),
    );
    let violation = liveness
        .observe(&state.state, 1)
        .expect("deadline")
        .expect("binding failure");
    assert_eq!(violation.bindings["i"], FslValue::Int(1));
}

#[test]
fn finite_prefix_reports_pending_and_unbounded_properties_separately() {
    let model = checked_model(&LATE_Q.replace(
        "leadsTo Progress { x == 1 ~> within 1 x == 3 }",
        "leadsTo Progress { x == 1 ~> within 3 x == 3 }\n  leadsTo Eventually { x == 0 ~> x == 3 }",
    ));
    let mut state = Monitor::new(model.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(model).expect("liveness monitor");
    liveness.observe(&state.state, 0).expect("initial");
    step(&mut state, "step", &BTreeMap::new());
    liveness.observe(&state.state, 1).expect("trigger");
    let status = liveness.status();
    assert_eq!(status.checked_properties, ["Progress"]);
    assert_eq!(status.unbounded_properties, ["Eventually"]);
    assert_eq!(status.pending[0].deadline, 4);
}

#[test]
fn static_range_binders_use_the_shared_concrete_expansion() {
    let mut model = checked_model(LATE_Q);
    model.leadstos[0].binders = vec![KernelBinder::Range {
        name: "i".to_owned(),
        lo: Box::new(KernelExpr::Num(0)),
        hi: Box::new(KernelExpr::Num(1)),
        where_expr: None,
    }];
    let mut state = Monitor::new(model.clone()).expect("state monitor");
    let mut liveness = BoundedLivenessMonitor::new(model).expect("liveness monitor");
    liveness.observe(&state.state, 0).expect("initial");
    step(&mut state, "step", &BTreeMap::new());
    liveness.observe(&state.state, 1).expect("trigger");
    step(&mut state, "step", &BTreeMap::new());
    let violation = liveness
        .observe(&state.state, 2)
        .expect("deadline")
        .expect("binding failure");
    assert_eq!(violation.bindings["i"], FslValue::Int(0));
}

#[test]
fn unsupported_deadline_shapes_fail_closed() {
    let mut negative = checked_model(LATE_Q);
    negative.leadstos[0].within = Some(-1);
    assert!(BoundedLivenessMonitor::new(negative).is_err());

    let mut collection = checked_model(LATE_Q);
    collection.leadstos[0].binders = vec![KernelBinder::Collection {
        name: "item".to_owned(),
        collection: Box::new(KernelExpr::Var("x".to_owned())),
        where_expr: None,
    }];
    assert!(BoundedLivenessMonitor::new(collection).is_err());

    let mut filtered = checked_model(LATE_Q);
    filtered.leadstos[0].binders = vec![KernelBinder::Range {
        name: "i".to_owned(),
        lo: Box::new(KernelExpr::Num(0)),
        hi: Box::new(KernelExpr::Num(1)),
        where_expr: Some(Box::new(KernelExpr::Bool(true))),
    }];
    assert!(BoundedLivenessMonitor::new(filtered).is_err());
}
