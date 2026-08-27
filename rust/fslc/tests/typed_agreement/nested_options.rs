// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::BTreeMap;

use fsl_core::FslValue;
use fsl_runtime::{Monitor, State};

const CLEAN: &str = r"
spec NestedOptionAssignment {
  type Bit = 0..1
  state { x: Option<Option<Bit>> }
  init { x = none }
  action wrap() { requires x == none x = some(none) }
  action fill() { requires x == some(none) x = some(some(1)) }
  action clear() { requires x == some(some(1)) x = none }
  invariant Shape {
    x == none or x == some(none) or x == some(some(0)) or x == some(some(1))
  }
  reachable Wrapped { x == some(none) }
  reachable Filled { x == some(some(1)) }
}
";

const VIOLATING: &str = r"
spec NestedOptionViolation {
  type Bit = 0..1
  state { x: Option<Option<Bit>> }
  init { x = none }
  action wrap() { requires x == none x = some(none) }
  action fill() { requires x == some(none) x = some(some(1)) }
  action clear() { requires x == some(some(1)) x = none }
  invariant NotFilled { x != some(some(1)) }
  reachable Wrapped { x == some(none) }
  reachable Filled { x == some(some(1)) }
}
";

const PLACEMENTS: &str = r"
spec NestedOptionPlacements {
  type Bit = 0..1
  type Id = 0..1
  struct Record { value: Option<Option<Bit>> }
  state {
    root: Option<Option<Bit>>,
    inline: Option<Option<Bit>> = some(none),
    depth_three: Option<Option<Option<Bit>>>,
    indexed: Map<Id, Option<Option<Bit>>>,
    field: Record,
    constructed: Record,
    records: Map<Id, Record>,
    flag: Bool
  }
  init {
    root = none
    depth_three = none
    forall id: Id { indexed[id] = none records[id] = Record { value: none } }
    field = Record { value: none }
    constructed = Record { value: none }
    flag = true
  }
  action root_assignment() { root = some(none) }
  action index_assignment(id: Id) { indexed[id] = some(none) }
  action struct_field_assignment() { field.value = some(none) }
  action whole_struct_construction() { constructed = Record { value: some(none) } }
  action map_value_struct_field(id: Id) { records[id].value = some(none) }
  action conditional_branch() { root = if flag then some(none) else some(some(1)) }
  action depth_three() { depth_three = some(some(none)) }
  invariant Bounded { root == none or root == some(none) or root == some(some(0)) or root == some(some(1)) }
}
";

fn nested_none() -> FslValue {
    FslValue::Some(Box::new(FslValue::None))
}

fn nested_one() -> FslValue {
    FslValue::Some(Box::new(FslValue::Some(Box::new(FslValue::Int(1)))))
}

fn reachables(
    values: &BTreeMap<String, Option<impl ReachableStep>>,
) -> BTreeMap<String, Option<usize>> {
    values
        .iter()
        .map(|(name, witness)| (name.clone(), witness.as_ref().map(ReachableStep::step)))
        .collect()
}

trait ReachableStep {
    fn step(&self) -> usize;
}

impl ReachableStep for fsl_runtime::ExplicitReachableWitness {
    fn step(&self) -> usize {
        self.step
    }
}

impl ReachableStep for fsl_verifier::ReachableWitness {
    fn step(&self) -> usize {
        self.step
    }
}

fn direct_states(model: &fsl_core::KernelModel) -> BTreeSet<State> {
    let initial = Monitor::new(model.clone()).expect("create nested-option monitor");
    let mut seen = BTreeSet::from([initial.state.clone()]);
    let mut frontier = vec![initial];
    for _ in 0..3 {
        let mut next = Vec::new();
        for monitor in &frontier {
            for enabled in monitor.enabled().expect("enumerate actions") {
                let mut child = monitor.clone();
                let stepped = child.step(&enabled).expect("step monitor");
                assert!(stepped.violation.is_none(), "{stepped:?}");
                if seen.insert(child.state.clone()) {
                    next.push(child);
                }
            }
        }
        frontier = next;
    }
    seen
}

fn exact_successor(
    model: &fsl_core::KernelModel,
    monitor: &mut Monitor,
    action: &str,
) -> State {
    let current = monitor.state.clone();
    let enabled = monitor
        .enabled()
        .expect("enumerate actions")
        .into_iter()
        .find(|candidate| candidate.action == action)
        .expect("expected enabled action");
    let stepped = monitor.step(&enabled).expect("step monitor");
    assert!(stepped.violation.is_none(), "{stepped:?}");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    assert!(
        engines::block_on(fsl_verifier::transition_matches_step(
            model,
            &mut solver,
            &current,
            &stepped.action,
            &stepped.params,
            &stepped.state,
        ))
        .expect("symbolic transition evaluates")
    );
    stepped.state
}

#[test]
#[allow(clippy::too_many_lines)]
fn nested_option_assignment_agrees_across_all_engines() {
    let clean = engines::build("nested_option_clean", CLEAN);
    assert_eq!(engines::run_agreement("nested_option_clean", &clean, 3), engines::Verdict::Clean);

    let mut monitor = Monitor::new(clean.clone()).expect("create clean monitor");
    assert_eq!(monitor.state["x"], FslValue::None);
    assert_eq!(exact_successor(&clean, &mut monitor, "wrap")["x"], nested_none());
    assert_eq!(exact_successor(&clean, &mut monitor, "fill")["x"], nested_one());
    assert_eq!(exact_successor(&clean, &mut monitor, "clear")["x"], FslValue::None);

    let expected_states = BTreeSet::from([
        BTreeMap::from([("x".to_owned(), FslValue::None)]),
        BTreeMap::from([("x".to_owned(), nested_none())]),
        BTreeMap::from([("x".to_owned(), nested_one())]),
    ]);
    assert_eq!(direct_states(&clean), expected_states);

    let explicit = fsl_runtime::verify_explicit(clean.clone(), 3, 100).expect("explicit verdict");
    assert!(explicit.violation.is_none(), "{explicit:?}");
    assert!(explicit.closure, "{explicit:?}");
    assert_eq!(reachables(&explicit.reachables), BTreeMap::from([
        ("Filled".to_owned(), Some(2)),
        ("Wrapped".to_owned(), Some(1)),
    ]));

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create BMC solver");
    let bmc = engines::block_on(fsl_verifier::verify_bounded(&clean, &mut solver, 3))
        .expect("BMC clean verdict");
    assert!(bmc.violation.is_none(), "{bmc:?}");
    assert_eq!(reachables(&bmc.reachables), reachables(&explicit.reachables));

    assert_eq!(fsl_core::fsl_value_json(&FslValue::None), serde_json::Value::Null);
    assert_eq!(
        fsl_core::fsl_value_json(&nested_none()),
        serde_json::json!({"kind":"some","value":null})
    );

    let placements = engines::build("nested_option_placements", PLACEMENTS);
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create placement BMC solver");
    let placement_bmc = engines::block_on(fsl_verifier::verify_bounded(&placements, &mut solver, 3))
        .expect("all nested Option placement forms are context typed");
    assert!(placement_bmc.violation.is_none(), "{placement_bmc:?}");
    for action in [
        "root_assignment",
        "index_assignment",
        "struct_field_assignment",
        "whole_struct_construction",
        "map_value_struct_field",
        "conditional_branch",
        "depth_three",
    ] {
        assert!(placement_bmc.action_coverage[action], "{action}");
    }
    let mut induction_solver = fsl_solver_z3::Z3Solver::new().expect("create induction solver");
    assert!(
        engines::block_on(fsl_verifier::prove_induction(&placements, &mut induction_solver, 1))
            .expect("bounded nested Option induction")
            .cti
            .is_none()
    );

    let violating = engines::build("nested_option_violating", VIOLATING);
    let explicit = fsl_runtime::verify_explicit(violating.clone(), 3, 100)
        .expect("explicit violation verdict");
    let explicit_violation = explicit.violation.expect("explicit detects filled state");
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create violating BMC solver");
    let bmc = engines::block_on(fsl_verifier::verify_bounded(&violating, &mut solver, 3))
        .expect("BMC violation verdict");
    let bmc_violation = bmc.violation.expect("BMC detects filled state");
    assert_eq!(explicit_violation.trace, bmc_violation.trace);
    fsl_runtime::replay_trace(violating.clone(), &explicit_violation.trace)
        .expect("explicit trace replays");
    fsl_runtime::replay_trace(violating, &bmc_violation.trace).expect("BMC trace replays");

    let (reachable_failure, state_failure) = engines::nested_option_comparator_negative_controls(
        "nested_option_violating",
        &reachables(&explicit.reachables),
        &reachables(&bmc.reachables),
        &explicit_violation.trace,
        &bmc_violation.trace,
    );
    assert_eq!((reachable_failure.edge.as_str(), reachable_failure.field.as_str()), ("explicit_bmc", "reachables"));
    assert_eq!((state_failure.edge.as_str(), state_failure.field.as_str()), ("trace_explicit_bmc", "trace"));
}
