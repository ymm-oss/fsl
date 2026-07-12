// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

#[test]
fn native_cli_checks_a_repository_spec_without_python() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "specs/cart_v1.fsl"])
        .current_dir(root)
        .output()
        .expect("run native CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "ok");
    assert_eq!(value["spec"], "ShoppingCart");
}
