// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/domain_origin_violation.fsl")
}

fn run(arguments: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid CLI JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

#[test]
fn verify_and_counterexample_trace_use_domain_declarations_as_primary_names() {
    let path = fixture();
    let (output, status) = run(&[
        "verify".to_owned(),
        path.to_string_lossy().into_owned(),
        "--depth".to_owned(),
        "1".to_owned(),
        "--deadlock".to_owned(),
        "ignore".to_owned(),
        "--no-cache".to_owned(),
    ]);
    assert_eq!(status, 1);
    assert_eq!(output["result"], "violated");
    assert_eq!(output["invariant"], "mustBeApproved");
    assert_eq!(output["generated_name"], "Order_mustBeApproved");
    assert_eq!(
        output["origin"]["primary"]["source_file"],
        path.to_string_lossy().as_ref()
    );
    assert_eq!(output["origin"]["primary"]["span"]["start"]["line"], 15);
    assert_eq!(
        output["origin"]["primary"]["declaration_path"],
        serde_json::json!([
            "Orders",
            "aggregate",
            "Order",
            "invariant",
            "mustBeApproved"
        ])
    );
    assert_eq!(output["trace"][0]["step"], 0);
}

#[test]
fn explain_surfaces_origin_without_promoting_generated_names() {
    let path = fixture();
    let (output, status) = run(&[
        "explain".to_owned(),
        path.to_string_lossy().into_owned(),
        "--depth".to_owned(),
        "1".to_owned(),
    ]);
    assert_eq!(status, 0);
    let property = output["skeleton"]["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .find(|property| property["name"] == "mustBeApproved")
        .expect("domain invariant");
    assert_eq!(property["generated_name"], "Order_mustBeApproved");
    assert_eq!(property["origin"]["dialect"], "domain");
    assert_eq!(
        property["requirement"]["id"], "DOMAIN-INVARIANT",
        "requirement traceability remains a separate field"
    );
    assert_ne!(
        property["origin"]["identity"],
        property["requirement"]["id"]
    );
}

#[test]
fn public_kernel_v1_remains_closed_and_does_not_emit_internal_chain_fields() {
    let path = fixture();
    let (output, status) = run(&["kernel".to_owned(), path.to_string_lossy().into_owned()]);
    assert_eq!(status, 0);
    assert_eq!(output["schema_version"], "1.0.0");
    let origin = output["properties"]["invariants"][0]["origin"]
        .as_object()
        .expect("public v1 origin");
    assert_eq!(
        origin
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        ["declaration", "dialect", "generated", "lowered"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    for internal in ["identity", "primary", "secondary", "lowering_steps"] {
        assert!(!origin.contains_key(internal));
    }
}
