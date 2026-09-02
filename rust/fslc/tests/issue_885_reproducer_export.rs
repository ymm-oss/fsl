// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{REPRODUCER_V1_SCHEMA_ID, REPRODUCER_V1_SCHEMA_VERSION};
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/{name}")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn scratch(name: &str) -> PathBuf {
    let path = root().join(format!(
        "rust/target/issue-885-{}-{}.json",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn compiled_schema() -> jsonschema::Validator {
    let workspace = root();
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/reproducer.v1.schema.json"))
            .expect("read reproducer schema"),
    )
    .expect("reproducer schema JSON");
    jsonschema::validator_for(&schema).expect("schema compiles")
}

#[test]
fn export_writes_a_schema_valid_reproducer_for_a_safety_invariant_violation() {
    let output_path = scratch("sweep-violating");
    let (value, status) = run(&[
        "counterexample",
        "export",
        &fixture("sweep_violating.fsl"),
        "--depth",
        "4",
        "-o",
        output_path.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(status, 1, "{value:#}");
    assert_eq!(value["result"], "violated");
    assert_eq!(value["violation_kind"], "invariant");
    assert_eq!(value["reproducer"]["schema"], REPRODUCER_V1_SCHEMA_ID);
    let artifact: Value =
        serde_json::from_str(&std::fs::read_to_string(&output_path).expect("artifact"))
            .expect("artifact JSON");
    compiled_schema()
        .validate(&artifact)
        .expect("artifact validates against reproducer.v1");
    assert_eq!(artifact["schema_version"], REPRODUCER_V1_SCHEMA_VERSION);
    assert_eq!(artifact["result"], "reproducer");
    assert_eq!(
        artifact["canonical_steps"],
        json!([{"action": "deposit", "params": {"a": 2}}])
    );
    assert!(
        artifact["origin"]["spec_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn bmc_and_explicit_projections_match_for_the_same_shortest_trace() {
    let bmc_path = scratch("bmc");
    let explicit_path = scratch("explicit");
    let spec = fixture("sweep_violating.fsl");
    let (_, bmc_status) = run(&[
        "counterexample",
        "export",
        &spec,
        "--depth",
        "4",
        "--engine",
        "bmc",
        "-o",
        bmc_path.to_str().expect("UTF-8 path"),
    ]);
    let (_, explicit_status) = run(&[
        "counterexample",
        "export",
        &spec,
        "--depth",
        "4",
        "--engine",
        "explicit",
        "-o",
        explicit_path.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(bmc_status, 1);
    assert_eq!(explicit_status, 1);
    let bmc: Value =
        serde_json::from_str(&std::fs::read_to_string(&bmc_path).expect("bmc artifact"))
            .expect("bmc JSON");
    let explicit: Value =
        serde_json::from_str(&std::fs::read_to_string(&explicit_path).expect("explicit artifact"))
            .expect("explicit JSON");
    assert_eq!(bmc["canonical_steps"], explicit["canonical_steps"]);
    assert_eq!(bmc["trace"], explicit["trace"]);
    assert_eq!(bmc["violation"], explicit["violation"]);
    assert_ne!(
        bmc["verification"]["engine"],
        explicit["verification"]["engine"]
    );
    assert!(bmc["verification"].get("engine_metadata").is_some());
    assert!(explicit["verification"].get("engine_metadata").is_some());
    let _ = std::fs::remove_file(bmc_path);
    let _ = std::fs::remove_file(explicit_path);
}

#[test]
fn unsupported_shapes_are_rejected_before_export() {
    let cases = [
        (
            "leadsto",
            &fixture("testgen_leadsto_violation.fsl"),
            "leadsTo",
        ),
        (
            "nondeterministic-init",
            &fixture("explicit_nondeterministic_init.fsl"),
            "nondeterministic",
        ),
        (
            "refinement",
            &fixture("issue_450_mapping.fsl"),
            "refinement",
        ),
    ];
    for (name, spec, needle) in cases {
        let output_path = scratch(name);
        let (value, status) = run(&[
            "counterexample",
            "export",
            spec,
            "--depth",
            "4",
            "-o",
            output_path.to_str().expect("UTF-8 path"),
        ]);
        assert_eq!(status, 2, "{name}: {value:#}");
        assert_eq!(value["result"], "error");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| message.contains(needle)),
            "{name}: {value:#}"
        );
        assert!(
            !output_path.exists(),
            "{name}: must not write an artifact when rejected"
        );
    }
    let output_path = scratch("induction");
    let (value, status) = run(&[
        "counterexample",
        "export",
        &fixture("sweep_violating.fsl"),
        "--depth",
        "4",
        "--engine",
        "induction",
        "-o",
        output_path.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(status, 2);
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("CTI")),
        "{value:#}"
    );
    assert!(!output_path.exists());
}

#[test]
fn verified_specs_fail_closed_without_writing_an_artifact() {
    let output_path = scratch("verified");
    let (value, status) = run(&[
        "counterexample",
        "export",
        "specs/cart_v1.fsl",
        "--depth",
        "8",
        "-o",
        output_path.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(status, 2, "{value:#}");
    assert_eq!(value["result"], "error");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("no counterexample")),
        "{value:#}"
    );
    assert!(!output_path.exists());
}
