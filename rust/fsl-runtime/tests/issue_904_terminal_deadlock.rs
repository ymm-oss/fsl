// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fslc/tests/fixtures")
        .join(name)
}

fn run_bfs(name: &str) -> fsl_runtime::BfsResult {
    let path = fixture(name);
    let source = std::fs::read_to_string(&path).expect("read terminal fixture");
    let resolver = FsResolver::new(path.parent().expect("fixture directory"));
    let kernel = parse_kernel_source(&source, &resolver).expect("parse terminal fixture");
    let model = build_model(kernel).expect("build terminal fixture");
    fsl_runtime::bfs(model, 1).expect("run legacy BFS")
}

#[test]
fn legacy_bfs_does_not_report_an_intended_terminal_state_as_deadlocked() {
    let result = run_bfs("assurance_terminal_once.fsl");

    assert_eq!(
        result.deadlock_step, None,
        "terminal positive control: produced {:?}, expected None",
        result.deadlock_step
    );
}

#[test]
fn legacy_bfs_still_reports_the_missing_terminal_sibling_as_deadlocked() {
    let result = run_bfs("assurance_terminal_once_missing.fsl");

    assert_eq!(
        result.deadlock_step,
        Some(1),
        "missing-terminal sibling: produced {:?}, expected Some(1)",
        result.deadlock_step
    );
}
