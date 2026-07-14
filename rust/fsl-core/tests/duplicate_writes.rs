// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{CoreError, FileResolver, build_model, parse_kernel_source};

struct EmptyResolver;

impl FileResolver for EmptyResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        Err(CoreError {
            message: format!("file not found: {path}"),
            line: 1,
            column: 1,
            origin: None,
        })
    }
}

fn build(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel = parse_kernel_source(source, &EmptyResolver).expect("parse");
    build_model(kernel)
}

#[test]
fn rejects_duplicate_action_write_during_model_build() {
    let error = build(
        "spec Duplicate { state { x: Bool } init { x = false } \
         action write_twice() { x = true x = false } }",
    )
    .expect_err("duplicate write must fail");

    assert!(
        error
            .message
            .contains("may not assign the same state location more than once")
    );
}

#[test]
fn permits_the_same_location_in_exclusive_branches() {
    build(
        "spec Branches { state { x: Bool } init { x = false } \
         action choose() { if x { x = false } else { x = true } } }",
    )
    .expect("exclusive branch writes are valid");
}

#[test]
fn rejects_a_write_after_both_exclusive_branches() {
    let error = build(
        "spec BranchThenWrite { state { x: Bool } init { x = false } \
         action choose() { if x { x = false } else { x = true } x = false } }",
    )
    .expect_err("post-branch duplicate write must fail");

    assert!(
        error
            .message
            .contains("may not assign the same state location more than once")
    );
}

#[test]
fn rejects_parameter_indexes_that_may_alias() {
    let error = build(
        "spec Alias { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall i: K { m[i] = false } } \
         action write(k: K, j: K) { m[k] = true m[j] = false } }",
    )
    .expect_err("parameter indexes may alias");

    assert!(error.message.contains("same state location"));
}

#[test]
fn permits_provably_distinct_constant_indexes() {
    build(
        "spec Distinct { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall i: K { m[i] = false } } \
         action write() { m[0] = true m[1] = false } }",
    )
    .expect("different constant indexes cannot alias");
}

#[test]
fn rejects_a_constant_index_repeated_by_forall() {
    let error = build(
        "spec Repeated { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall i: K { m[i] = false } } \
         action write() { forall i: K { m[0] = true } } }",
    )
    .expect_err("constant write repeats across forall iterations");

    assert!(error.message.contains("same state location"));
}

#[test]
fn permits_a_forall_write_indexed_by_its_binder() {
    build(
        "spec Bulk { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall i: K { m[i] = false } } \
         action write() { forall i: K { m[i] = true } } }",
    )
    .expect("each binder value selects a distinct map entry");
}

#[test]
fn rejects_a_write_that_may_overlap_a_forall_write() {
    let error = build(
        "spec BulkThenOne { type K = 0..1 state { m: Map<K, Bool> } \
         init { forall i: K { m[i] = false } } \
         action write() { forall i: K { m[i] = true } m[0] = false } }",
    )
    .expect_err("post-forall write overlaps one binder value");

    assert!(error.message.contains("same state location"));
}

#[test]
fn permits_distinct_fields_even_when_indexes_may_alias() {
    build(
        "spec Fields { type K = 0..1 struct Pair { left: Bool, right: Bool } \
         state { m: Map<K, Pair> } \
         init { forall i: K { m[i] = Pair { left: false, right: false } } } \
         action write(k: K, j: K) { m[k].left = true m[j].right = true } }",
    )
    .expect("different fields are different locations");
}
