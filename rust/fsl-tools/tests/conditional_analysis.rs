// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use fsl_core::{FsResolver, build_model, parse_kernel_source};

#[test]
fn analysis_reads_condition_and_both_conditional_branches() {
    let source = "spec S { state { gate: Bool, left: Bool, right: Bool } init { gate = true left = true right = false } action choose() { requires if gate then left else right gate = gate } invariant I { true } }";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel).expect("build model");
    let tsg = fsl_tools::build_tsg(&model);
    let reads = tsg["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|edge| edge["from"] == "action:choose" && edge["kind"] == "reads")
        .filter_map(|edge| edge["to"].as_str())
        .collect::<BTreeSet<_>>();

    assert!(reads.contains("state:gate"));
    assert!(reads.contains("state:left"));
    assert!(reads.contains("state:right"));
}
