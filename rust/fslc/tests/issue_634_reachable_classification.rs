// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE: &str = "rust/fslc/tests/fixtures/issue_634_reachable_classification.fsl";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn verify(depth: usize) -> (Value, i32) {
    let depth = depth.to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            FIXTURE,
            "--engine",
            "bmc",
            "--depth",
            &depth,
            "--deadlock",
            "ignore",
            "--no-cache",
        ])
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("fslc exit status"))
}

#[test]
fn static_contradiction_names_its_blocking_invariant() {
    let (output, status) = verify(0);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "reachable_failed");
    let unreached = output["unreached"].as_array().expect("unreached array");
    let impossible = unreached
        .iter()
        .find(|entry| entry["name"] == "Impossible")
        .expect("Impossible diagnosis");
    assert_eq!(impossible["classification"], "over_constrained");
    assert_eq!(impossible["blocking_requires"][0]["kind"], "invariant");
    assert_eq!(impossible["blocking_requires"][0]["name"], "Cap");
    assert_eq!(
        impossible["recommended_action"],
        "fix the blocking type bound/invariant and rerun verification"
    );
    assert!(
        !impossible["recommended_action"]
            .as_str()
            .expect("recommended action")
            .contains("depth"),
        "{impossible:#}"
    );
    assert_eq!(
        impossible["blocking_requires"][0]["requirement"]["id"],
        "REQ-CAP"
    );
}

#[test]
fn satisfiable_state_predicate_remains_depth_limited() {
    let (output, status) = verify(0);
    assert_eq!(status, 1, "{output:#}");
    let later = output["unreached"]
        .as_array()
        .expect("unreached array")
        .iter()
        .find(|entry| entry["name"] == "Later")
        .expect("Later diagnosis");
    assert_eq!(later["classification"], "insufficient_depth");
    assert!(later.get("blocking_requires").is_none(), "{later:#}");
}
