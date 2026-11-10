// SPDX-License-Identifier: Apache-2.0

//! Accepted compose output pins the source-separated binding names from #1573.

use std::path::Path;
use std::process::Command;

fn assert_approved_explain(file: &str, expected: &str) {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_1547_guard_reservations");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["explain", file])
        .current_dir(&directory)
        .output()
        .expect("native explain");
    assert!(output.status.success(), "{file}: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{file}: {:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        expected,
        "{file}: the full approved rendering, including source binding names, must be preserved"
    );
}

#[test]
fn sibling_negated_patterns_keep_source_names_separate() {
    assert_approved_explain(
        "c.fsl",
        include_str!("fixtures/issue_1547_guard_reservations/c.explain.json"),
    );
}

#[test]
fn conditional_guard_pattern_keeps_source_names_separate() {
    assert_approved_explain(
        "c_if.fsl",
        include_str!("fixtures/issue_1547_guard_reservations/c_if.explain.json"),
    );
}
