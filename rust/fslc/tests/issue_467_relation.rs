// SPDX-License-Identifier: Apache-2.0

//! CLI-level coupled-change coverage for issue #467: `relation A -> B`
//! native support end to end through the `fslc` binary's JSON envelope and
//! exit-code contract.

use std::process::Command;

fn run(args: &[&str]) -> (serde_json::Value, i32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

#[test]
fn relation_check_and_verify_are_exit_0_ok_and_verified() {
    // docs/LANGUAGE.md:583-585 promises `relation A -> B` and all seven
    // operations unconditionally; both `check` and `verify` must accept it.
    let (check_output, check_status) = run(&[
        "check",
        "rust/fslc/tests/fixtures/issue_467_relation_demo.fsl",
    ]);
    assert_eq!(check_status, 0, "{check_output}");
    assert_eq!(check_output["result"], "ok");

    let (verify_output, verify_status) = run(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_467_relation_demo.fsl",
        "--depth",
        "2",
        "--no-cache",
    ]);
    assert_eq!(verify_status, 0, "{verify_output}");
    assert_eq!(verify_output["result"], "verified");
    assert_eq!(
        verify_output["reachables"]["CanDelegate"]["witnessed_at_step"],
        1
    );
}
