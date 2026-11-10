// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

#![allow(clippy::duplicate_mod)] // Independent statics model separate runs.

#[allow(dead_code)]
#[path = "support/compose_scratch.rs"]
mod first;
#[allow(dead_code)]
#[path = "support/compose_scratch.rs"]
mod second;

#[test]
fn independent_runs_with_the_same_label_pid_and_counter_do_not_collide() {
    let first = first::scratch("issue-1563-independent-runs", &[]);
    let second = second::scratch("issue-1563-independent-runs", &[]);
    assert_ne!(first, second);
    assert!(first.is_dir() && second.is_dir());
}
