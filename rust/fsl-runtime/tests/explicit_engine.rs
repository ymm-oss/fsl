// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

#[test]
fn explicit_bfs_proves_at_state_space_closure() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } \
         action finish() { requires not done done = true } \
         invariant BooleanState { done or not done } terminal { done } }",
    );
    let result = fsl_runtime::verify_explicit(model, 4, 100).expect("explicit verification");
    assert!(result.closure);
    assert!(!result.budget_exceeded);
    assert!(result.violation.is_none());
    assert_eq!(result.states_explored, 2);
    assert_eq!(result.depth_reached, 1);
    assert_eq!(result.deadlock_step, None);
}

#[test]
fn explicit_bfs_fails_closed_at_the_state_budget() {
    let model = model(
        "spec Counter { type Count = 0..2 state { count: Count } init { count = 0 } \
         action add() { requires count < 2 count = count + 1 } }",
    );
    let result = fsl_runtime::verify_explicit(model, 4, 1).expect("budget verdict");
    assert!(result.budget_exceeded);
    assert!(!result.closure);
    assert!(result.violation.is_none());
    assert_eq!(result.states_explored, 1);
    assert_eq!(result.depth_reached, 0);
}

#[test]
fn explicit_bfs_rejects_underconstrained_or_order_dependent_init() {
    let missing = model(
        "spec Missing { state { x: Bool, y: Bool } init { x = false } \
         action stay() { x = x y = y } }",
    );
    let error = fsl_runtime::verify_explicit(missing, 2, 100).expect_err("missing init rejected");
    assert_eq!(error.message, "init does not assign state variable(s): y");

    let read_before_write = model(
        "spec ReadBeforeWrite { state { x: Bool, y: Bool } \
         init { x = y y = false } action stay() { x = x y = y } }",
    );
    let error = fsl_runtime::verify_explicit(read_before_write, 2, 100)
        .expect_err("order-dependent init rejected");
    assert_eq!(
        error.message,
        "init references state variable 'y' before it is assigned"
    );
}

#[test]
fn deterministic_init_rejects_state_dependent_forall_domains() {
    let unassigned_range = model(
        "spec UnassignedRange { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { forall i in 0..n { m[i] = true } n = 0 } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(unassigned_range, 2, 100)
        .expect_err("state-dependent init range rejected");
    assert_eq!(
        error.message,
        "init forall range bounds must be compile-time constants; state variable 'n' is not allowed"
    );

    let assigned_range = model(
        "spec AssignedRange { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { n = 2 forall i in 0..n { m[i] = true } } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(assigned_range, 2, 100)
        .expect_err("assigned state still cannot bound an init range");
    assert_eq!(
        error.message,
        "init forall range bounds must be compile-time constants; state variable 'n' is not allowed"
    );

    let state_collection = model(
        "spec StateCollection { type Slot = 0..2 \
         state { s: Set<Slot>, m: Map<Slot, Bool> } \
         init { s = Set { 0, 1 } forall i in s { m[i] = true } } \
         action stay() { s = s } }",
    );
    let error = fsl_runtime::verify_explicit(state_collection, 2, 100)
        .expect_err("state collection init domain rejected");
    assert_eq!(
        error.message,
        "init forall over a state collection is not supported; state variable 's' is not allowed"
    );

    let state_range_filter = model(
        "spec StateRangeFilter { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { forall i in 0..2 where n == 0 { m[i] = true } n = 0 } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(state_range_filter, 2, 100)
        .expect_err("state-dependent init range filter rejected");
    assert_eq!(
        error.message,
        "init references state variable 'n' before it is assigned"
    );

    let const_range = model(
        "spec ConstRange { const CAP = 2 type Slot = 0..2 \
         state { m: Map<Slot, Bool> } \
         init { forall i in 0..CAP { m[i] = true } } \
         action stay() { m[0] = m[0] } }",
    );
    let result = fsl_runtime::verify_explicit(const_range, 2, 100)
        .expect("compile-time const init range accepted");
    assert!(result.closure);
    assert!(result.violation.is_none());
}

#[test]
fn deterministic_init_tracks_branches_foralls_and_duplicate_locations() {
    let both_branches = model(
        "spec Branches { state { flag: Bool, value: Bool } \
         init { flag = false if flag { value = true } else { value = false } } \
         action stay() { flag = flag value = value } }",
    );
    fsl_runtime::verify_explicit(both_branches, 1, 100)
        .expect("both init branches definitely assign value");

    let one_branch = model(
        "spec OneBranch { state { flag: Bool, value: Bool } \
         init { flag = false if flag { value = true } } \
         action stay() { flag = flag value = value } }",
    );
    let error =
        fsl_runtime::verify_explicit(one_branch, 1, 100).expect_err("one branch is incomplete");
    assert_eq!(
        error.message,
        "init does not assign state variable(s): value"
    );

    let distinct_keys = model(
        "spec DistinctKeys { enum Key { A, B } state { values: Map<Key, Bool> } \
         init { values[A] = false values[B] = true } \
         action stay() { values[A] = values[A] } }",
    );
    fsl_runtime::verify_explicit(distinct_keys, 1, 100)
        .expect("separate concrete map keys are not duplicate init writes");

    let duplicate = model(
        "spec Duplicate { state { value: Bool } init { value = false value = true } \
         action stay() { value = value } }",
    );
    let error =
        fsl_runtime::verify_explicit(duplicate, 1, 100).expect_err("duplicate init rejected");
    assert_eq!(
        error.message,
        "state variable 'value' assigned more than once in init"
    );

    let nested_forall = model(
        "spec Nested { const MAX = 1 state { values: Map<Int, Int> } \
         init { forall i in 0..MAX: { forall j in 0..MAX: { values[i] = j } } } \
         action stay() { values[0] = values[0] } }",
    );
    let error = fsl_runtime::verify_explicit(nested_forall, 1, 100)
        .expect_err("nested init forall rejected");
    assert_eq!(error.message, "nested forall in init is not supported");
}

#[test]
fn explicit_violation_trace_replays_through_the_monitor() {
    let model = model(
        "spec Overflow { type Count = 0..1 state { count: Count } init { count = 0 } \
         action add() { count = count + 1 } }",
    );
    let result =
        fsl_runtime::verify_explicit(model.clone(), 3, 100).expect("explicit verification");
    let violation = result.violation.expect("type-bound violation");
    assert_eq!(violation.violation.kind, "type_bound");
    assert_eq!(violation.violation.step, 2);
    fsl_runtime::replay_trace(model, &violation.trace).expect("replay explicit trace");
}
