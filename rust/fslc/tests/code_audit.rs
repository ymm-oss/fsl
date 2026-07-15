// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).expect("JSON output");
    (value, output.status.code().expect("exit code"))
}

#[test]
fn publishes_closed_annotation_and_output_schemas() {
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fslc/analysis");
    for (filename, id) in [
        ("code-trace.v0.schema.json", "fsl-code-trace.v0"),
        ("code-audit.v0.schema.json", "code-audit.v0"),
    ] {
        let schema: Value = serde_json::from_slice(
            &std::fs::read(schemas.join(filename)).expect("read published schema"),
        )
        .expect("valid JSON Schema");
        assert!(schema["$id"].as_str().unwrap().ends_with(filename));
        assert!(schema.to_string().contains(id));
        assert_eq!(schema["additionalProperties"], false);
    }
}

#[test]
fn reports_complete_requirement_to_code_coverage() {
    let spec = fixture("typed_annotation_outputs.fsl");
    let code = fixture("code_audit");
    let (output, status) = run(&[
        "analyze",
        spec.to_str().unwrap(),
        "--projection",
        "code_audit",
        "--code",
        code.to_str().unwrap(),
    ]);

    assert_eq!(status, 0);
    assert_eq!(output["result"], "analyzed");
    assert_eq!(output["schema_version"], "code-audit.v0");
    assert_eq!(output["coverage"]["requirements"]["total"], 4);
    assert_eq!(output["coverage"]["requirements"]["covered"], 4);
    assert_eq!(output["coverage"]["requirement_targets"]["total"], 6);
    assert_eq!(output["coverage"]["requirement_targets"]["covered"], 6);
    assert_eq!(output["findings"], serde_json::json!([]));
    assert_eq!(
        output["coverage"]["by_origin_assurance"]["generated_from_source"]["requirement_targets"],
        3
    );
}

#[test]
fn reports_missing_orphan_and_mismatched_annotations_without_failing() {
    let root = std::env::temp_dir().join(format!("fsl-code-audit-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let code = root.join("audit.rs");
    std::fs::write(
        &code,
        concat!(
            "// @fsl.trace {\"schema\":\"fsl-code-trace.v0\",\"requirement_id\":\"REQ-ACTION\",\"kernel_target\":\"property:reachable:Published\",\"origin_assurance\":\"source_backed\"}\n",
            "// @fsl.trace {\"schema\":\"fsl-code-trace.v0\",\"requirement_id\":\"REQ-UNKNOWN\",\"kernel_target\":\"action:publish\",\"origin_assurance\":\"unknown\"}\n",
        ),
    )
    .unwrap();
    let spec = fixture("typed_annotation_outputs.fsl");
    let (output, status) = run(&[
        "analyze",
        spec.to_str().unwrap(),
        "--projection",
        "code_audit",
        "--code",
        code.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(status, 0);
    let kinds = output["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["finding_type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"annotation_target_mismatch"));
    assert!(kinds.contains(&"orphan_code_annotation"));
    assert!(kinds.contains(&"missing_requirement_implementation"));
    let order = output["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["requirement_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                finding["kernel_target"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                finding["location"]["file"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                finding["location"]["line"].as_u64().unwrap_or_default(),
                finding["location"]["column"].as_u64().unwrap_or_default(),
                finding["finding_type"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}

#[test]
fn rejects_malformed_annotations_and_invalid_cli_combinations() {
    let root = std::env::temp_dir().join(format!("fsl-code-audit-bad-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let code = root.join("audit.rs");
    std::fs::write(&code, "// @fsl.trace {not-json}\n").unwrap();
    let spec = fixture("typed_annotation_outputs.fsl");

    let (malformed, malformed_status) = run(&[
        "analyze",
        spec.to_str().unwrap(),
        "--projection",
        "code_audit",
        "--code",
        code.to_str().unwrap(),
    ]);
    let (missing_code, missing_status) = run(&[
        "analyze",
        spec.to_str().unwrap(),
        "--projection",
        "code_audit",
    ]);
    let (wrong_projection, wrong_status) = run(&[
        "analyze",
        spec.to_str().unwrap(),
        "--code",
        code.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(
        (malformed["kind"].as_str(), malformed_status),
        (Some("semantics"), 2)
    );
    assert_eq!(
        (missing_code["kind"].as_str(), missing_status),
        (Some("semantics"), 2)
    );
    assert_eq!(
        (wrong_projection["kind"].as_str(), wrong_status),
        (Some("semantics"), 2)
    );
}
