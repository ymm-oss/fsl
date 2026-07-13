// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{FsResolver, KernelExpr, build_model, parse_kernel_source};
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(path: &Path) -> (fsl_core::KernelSpec, fsl_core::KernelModel) {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let kernel = parse_kernel_source(
        &source,
        &FsResolver::new(path.parent().expect("fixture directory")),
    )
    .expect("parse fixture");
    let model = build_model(kernel.clone()).expect("build fixture");
    (kernel, model)
}

#[test]
fn public_kernel_is_versioned_typed_traceable_and_deterministic() {
    let path = fixture("kernel_contract.fsl");
    let (kernel, model) = load(&path);
    let path = path.to_string_lossy();
    let first = fsl_core::public_kernel_contract(&kernel, &model, &path, "kernel")
        .expect("export public Kernel");
    let second =
        fsl_core::public_kernel_contract(&kernel, &model, &path, "kernel").expect("repeat export");
    assert_eq!(first, second);
    assert_eq!(first["schema_version"], "1.0.0");
    assert_eq!(first["semantics"]["assignment"], "simultaneous");
    assert_eq!(first["semantics"]["failure_state"], "rollback");
    assert_eq!(first["actions"][0]["requirement"]["id"], "REQ-208-ADD");
    assert_eq!(first["actions"][0]["origin"]["lowered"], false);
    assert!(first["actions"][0]["span"]["line"].as_u64().is_some());
    assert!(first["actions"][0]["requires"][0]["type"].is_object());
    assert!(first["actions"][0]["updates"][0]["value"]["type"].is_object());
    assert_eq!(
        first["actions"][1]["partial_operations"]
            .as_array()
            .expect("partial operations")
            .iter()
            .map(|operation| operation["operation"].as_str().expect("operation"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["head", "pop"])
    );
}

#[test]
fn public_kernel_rejects_an_unlowered_expression() {
    let path = fixture("kernel_contract.fsl");
    let (kernel, mut model) = load(&path);
    let span = model.invariants[0].span;
    model.invariants[0].expr = KernelExpr::Call {
        name: "not_lowered".to_owned(),
        args: Vec::new(),
        span,
    };
    let error = fsl_core::public_kernel_contract(&kernel, &model, "fixture.fsl", "kernel")
        .expect_err("unlowered expression must fail");
    assert!(error.message.contains("unlowered predicate call"));
}

#[test]
fn public_kernel_preserves_guard_order_and_pattern_bindings() {
    let source = r"
spec Binding {
  type Bit = 0..1
  state { x: Option<Bit>, y: Bit }
  init { x = some(1)  y = 0 }
  action take() {
    requires x is some(v) and v == 1
    let copied = v
    y = copied
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse binding model");
    let model = build_model(kernel.clone()).expect("build binding model");
    let contract = fsl_core::public_kernel_contract(&kernel, &model, "binding.fsl", "kernel")
        .expect("export binding model");
    assert_eq!(contract["actions"][0]["guards"][0]["kind"], "requires");
    assert_eq!(contract["actions"][0]["guards"][1]["kind"], "let");
    assert_eq!(
        contract["actions"][0]["updates"][0]["value"]["type"]["kind"],
        "named"
    );
}

#[test]
fn typestate_consumes_the_versioned_public_kernel_contract() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let path = workspace.join("specs/order_workflow.fsl");
    let (kernel, model) = load(&path);
    let contract = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        path.to_str().expect("UTF-8 path"),
        "kernel",
    )
    .expect("export public Kernel");

    let report = fsl_tools::analyze_typestate(&contract).expect("analyze public Kernel");

    assert_eq!(report["result"], "typestate");
    assert_eq!(report["spec"], "OrderWorkflow");
    assert_eq!(report["summary"]["full"], 1);
    assert_eq!(report["entities"][0]["actions"][0]["action"], "place");
    assert_eq!(report["entities"][0]["actions"][3]["action"], "cancel");
}

#[test]
fn typestate_rejects_an_incompatible_public_kernel_version() {
    let path = fixture("kernel_contract.fsl");
    let (kernel, model) = load(&path);
    let mut contract = fsl_core::public_kernel_contract(&kernel, &model, "fixture.fsl", "kernel")
        .expect("export public Kernel");
    contract["schema_version"] = Value::String("2.0.0".to_owned());

    let error = fsl_tools::analyze_typestate(&contract)
        .expect_err("unknown public Kernel versions must fail closed");

    assert!(error.contains("unsupported public Kernel schema_version"));
}

#[test]
fn native_cli_typestate_outputs_match_the_v1_golden_files() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let binary = env!("CARGO_BIN_EXE_fslc");

    for (extra, expected) in [
        (
            Vec::<&str>::new(),
            include_bytes!("fixtures/typestate_order.v1.json").as_slice(),
        ),
        (
            vec!["--ts"],
            include_bytes!("fixtures/typestate_order.v1.ts").as_slice(),
        ),
    ] {
        let output = Command::new(binary)
            .current_dir(workspace)
            .args(["typestate", "specs/order_workflow.fsl"])
            .args(extra)
            .output()
            .expect("run native typestate CLI");

        assert!(output.status.success());
        assert_eq!(output.stdout, expected);
    }
}

#[test]
fn conformance_distinguishes_nested_options_and_guard_partials() {
    let source = r"
spec NestedOption {
  type Bit = 0..1
  state { x: Option<Option<Bit>>, y: Bit }
  init { x = none  y = 0 }
  action wrap() { x = some(none) }
  action fill() { requires x is some(v)  x = some(some(1)) }
  action guard_partial() { requires 1 / y == 0  y = y }
  action ensure_partial() { y = y  ensures 1 / y == 0 }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse option model");
    let model = build_model(kernel).expect("build option model");
    let output = fslc_rust::conformance_vectors(&model, 2).expect("generate vectors");
    let states = output["states"].as_array().expect("states");
    assert!(
        states
            .iter()
            .any(|state| state["state"]["x"]["kind"] == "none")
    );
    assert!(
        states
            .iter()
            .any(|state| state["state"]["x"]["kind"] == "some")
    );
    assert!(
        output["vectors"]
            .as_array()
            .expect("vectors")
            .iter()
            .filter(|vector| {
                matches!(
                    vector["action"]["name"].as_str(),
                    Some("guard_partial" | "ensure_partial")
                )
            })
            .all(|vector| vector["outcome"]["kind"] == "partial_op")
    );
}

#[test]
fn compose_export_fails_instead_of_fabricating_component_source_paths() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let path = workspace.join("specs/bank_system.fsl");
    let (kernel, model) = load(&path);
    let error = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        path.to_str().expect("UTF-8 path"),
        "compose",
    )
    .expect_err("compose provenance is not representable in v1");
    assert!(error.message.contains("component source filenames"));
}

#[test]
fn conformance_vectors_cover_failures_without_state_changes() {
    let input = fixture("conformance_failures.fsl");
    let (kernel, model) = load(&input);
    let output = fslc_rust::conformance_vectors(&model, 1).expect("generate vectors");
    let vectors = output["vectors"].as_array().expect("vectors");
    let kinds = vectors
        .iter()
        .map(|vector| vector["outcome"]["kind"].as_str().expect("outcome"))
        .collect::<BTreeSet<_>>();
    assert!(kinds.contains("partial_op"));
    assert!(kinds.contains("requires_failed"));
    assert!(kinds.contains("type_bound"));
    assert!(kinds.contains("invariant"));
    assert!(kinds.contains("ensures"));
    assert!(vectors.iter().all(|vector| {
        vector["outcome"]["kind"] == "ok"
            || vector["outcome"]["state_changed"] == Value::Bool(false)
    }));

    let contract = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        input.to_str().expect("UTF-8 path"),
        "kernel",
    )
    .expect("export failure contract");
    let operations = contract["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .flat_map(|action| {
            action["partial_operations"]
                .as_array()
                .expect("partial operations")
        })
        .map(|operation| operation["operation"].as_str().expect("operation"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        operations,
        BTreeSet::from(["at", "divide", "index", "pop", "remainder"])
    );
}

#[test]
fn native_cli_exports_lowered_requirements_without_python() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let requirements = workspace.join("examples/e2e/2_requirements.fsl");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["kernel", requirements.to_str().expect("UTF-8 path")])
        .output()
        .expect("run native CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contract: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(contract["result"], "kernel");
    assert_eq!(contract["spec"]["source"]["dialect"], "requirements");
    assert_eq!(contract["actions"][0]["origin"]["lowered"], true);
    assert!(contract["actions"][0]["requirement"]["id"].is_string());
}

#[test]
fn native_cli_conformance_output_matches_the_v1_golden_vector() {
    let input = fixture("conformance_failures.fsl");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "conformance",
            input.to_str().expect("UTF-8 path"),
            "--depth",
            "0",
        ])
        .output()
        .expect("run native CLI");
    assert!(output.status.success());
    let actual: Value = serde_json::from_slice(&output.stdout).expect("actual JSON");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("conformance_failures.v1.json"))
            .expect("read golden vector"),
    )
    .expect("golden JSON");
    assert_eq!(actual, expected);
}

#[test]
fn native_cli_kernel_output_matches_the_v1_golden_contract() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let relative = "rust/fslc/tests/fixtures/kernel_contract.fsl";
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace)
        .args(["kernel", relative])
        .output()
        .expect("run native CLI");
    assert!(output.status.success());
    let actual: Value = serde_json::from_slice(&output.stdout).expect("actual JSON");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("kernel_contract.v1.json"))
            .expect("read golden Kernel contract"),
    )
    .expect("golden JSON");
    assert_eq!(actual, expected);
}

#[test]
fn native_cli_contract_and_help_publish_the_new_commands() {
    let binary = env!("CARGO_BIN_EXE_fslc");
    let contract = Command::new(binary)
        .arg("--cli-contract")
        .output()
        .expect("read CLI contract");
    let contract: Value = serde_json::from_slice(&contract.stdout).expect("CLI contract JSON");
    let paths = contract["root"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .filter_map(|command| command["path"].as_array())
        .filter_map(|path| path.first())
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    assert!(paths.contains("kernel"));
    assert!(paths.contains("conformance"));

    for command in ["kernel", "conformance"] {
        let help = Command::new(binary)
            .args([command, "--help"])
            .output()
            .expect("run help");
        assert!(help.status.success());
        assert!(
            String::from_utf8_lossy(&help.stdout).starts_with(&format!("usage: fslc {command}"))
        );
    }
}

#[test]
fn published_schema_ids_match_the_rust_api_constants() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let kernel: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/kernel.v1.schema.json"))
            .expect("read Kernel schema"),
    )
    .expect("Kernel schema JSON");
    let conformance: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/conformance.v1.schema.json"))
            .expect("read conformance schema"),
    )
    .expect("conformance schema JSON");
    assert_eq!(kernel["$id"], fsl_core::KERNEL_SCHEMA_ID);
    assert_eq!(conformance["$id"], fslc_rust::CONFORMANCE_SCHEMA_ID);
    let kinds = kernel["$defs"]["expression"]["properties"]["kind"]["enum"]
        .as_array()
        .expect("expression kind enum");
    assert!(!kinds.contains(&Value::String("totally_unknown".to_owned())));
}
