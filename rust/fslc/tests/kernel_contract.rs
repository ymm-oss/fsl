// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use fsl_core::{FsResolver, KernelExpr, ProjectionDef, build_model, parse_kernel_source};
use serde_json::{Value, json};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root().join(format!(
        "rust/target/kernel-contract-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run_cli(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn compiled_kernel_v1_schema() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("schemas/fslc/kernel/kernel.v1.schema.json"))
            .expect("read Kernel v1 schema"),
    )
    .expect("Kernel v1 schema JSON");
    jsonschema::validator_for(&schema).expect("Kernel v1 schema compiles")
}

fn kernel_state<'a>(contract: &'a Value, name: &str) -> &'a Value {
    contract["state"]
        .as_array()
        .expect("kernel state")
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("missing state '{name}'"))
}

/// Returns the `init` assignment statement whose target variable is `name`.
///
/// `init` is an object (`origin` / `requirement` / `span` / `statements`), not an
/// array of named entries, and each statement carries its variable under
/// `target.name` rather than a top-level `name`. Reading it as a flat array
/// panicked with "kernel init"; the first revision of this helper assumed the
/// wrong shape and could not be caught while the tests still stopped at the
/// `kernel_status` assertion.
fn kernel_init<'a>(contract: &'a Value, name: &str) -> &'a Value {
    contract["init"]["statements"]
        .as_array()
        .expect("kernel init statements")
        .iter()
        .find(|statement| statement["target"]["name"] == name)
        .unwrap_or_else(|| panic!("missing init assignment for '{name}'"))
}

fn assert_empty_relation_init(init: &Value, label: &str) {
    assert_eq!(init["value"]["kind"], "set_lit", "{label}");
    assert_eq!(init["value"]["type"]["kind"], "relation", "{label}");
    assert_eq!(init["value"]["items"], json!([]), "{label}");
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
fn public_kernel_uses_checked_model_validation_for_unprojected_metadata() {
    let path = fixture("kernel_contract.fsl");
    let (kernel, mut model) = load(&path);
    let span = model.invariants[0].span;
    model.projections.push(ProjectionDef {
        name: "corrupted_kpi".to_owned(),
        entity: "Corrupted".to_owned(),
        stage: "Corrupted".to_owned(),
        expr: KernelExpr::Call {
            name: "not_lowered".to_owned(),
            args: Vec::new(),
            span,
        },
        span,
    });

    let error = fsl_core::public_kernel_contract(&kernel, &model, "fixture.fsl", "kernel")
        .expect_err("exporter must use the neutral checked-model validator");
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
fn typestate_rejects_conditional_nodes_instead_of_omitting_them() {
    let source = "spec S { type N = 0..1 state { x: N, gate: Bool } init { x = 0 gate = true } action choose() { x = if gate then 1 else 0 gate = gate } invariant I { true } }";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse model");
    let model = build_model(kernel.clone()).expect("build model");
    let mut contract = fsl_core::public_kernel_contract(&kernel, &model, "s.fsl", "kernel")
        .expect("export public Kernel");

    let error = fsl_tools::analyze_typestate(&contract)
        .expect_err("unsupported conditional must not be silently ignored");
    assert!(error.contains("conditional expressions"));

    contract["actions"][0]["updates"][0]["value"]["kind"] =
        Value::String("unknown_node".to_owned());
    let error = fsl_tools::analyze_typestate(&contract)
        .expect_err("unknown public Kernel nodes must be rejected");
    assert!(error.contains("unknown expression kind 'unknown_node'"));
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
        states
            .iter()
            .any(|state| { state["state"]["x"] == json!({"kind":"some","value":{"kind":"none"}}) }),
        "conformance must preserve some(none): {states:#?}"
    );
    assert!(
        states.iter().any(|state| {
            state["state"]["x"] == json!({"kind":"some","value":{"kind":"some","value":1}})
        }),
        "conformance must preserve some(some(1)): {states:#?}"
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
    let error = fsl_core::public_kernel_contract_for_version(
        &kernel,
        &model,
        "specs/bank_system.fsl",
        "compose",
        fsl_core::PublicKernelVersion::V2,
    )
    .expect_err("compose provenance is not representable in v2");
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
    assert!(kinds.contains("trans"));
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
fn native_cli_conformance_output_matches_the_v2_golden_vector() {
    let input = fixture("conformance_failures.fsl");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "conformance",
            input.to_str().expect("UTF-8 path"),
            "--depth",
            "0",
            "--kernel-version",
            "2",
        ])
        .output()
        .expect("run native CLI");
    assert!(output.status.success());
    let actual: Value = serde_json::from_slice(&output.stdout).expect("actual JSON");
    let golden = fixture("conformance_failures.v2.json");
    if std::env::var_os("FSLC_CONFORMANCE_V2_UPDATE").is_some() {
        std::fs::write(
            &golden,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&actual).expect("serialize v2 vectors")
            ),
        )
        .expect("update v2 conformance golden");
    }
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(golden).expect("read golden v2 vector"))
            .expect("golden JSON");
    assert_eq!(actual, expected);
    assert_eq!(actual["kernel_schema_version"], "2.0.0");
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
fn native_cli_kernel_v2_output_matches_the_origin_golden_contract() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let relative = "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl";
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace)
        .args(["kernel", relative, "--kernel-version", "2"])
        .output()
        .expect("run native CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).expect("actual JSON");
    assert_eq!(actual["schema_version"], "2.0.0");
    assert_eq!(
        actual["provenance"]["identity_stability"],
        "exact_source_revision"
    );

    let golden = fixture("kernel_origin.v2.json");
    if std::env::var_os("FSLC_KERNEL_V2_UPDATE").is_some() {
        std::fs::write(
            &golden,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&actual).expect("serialize v2 golden")
            ),
        )
        .expect("update v2 golden");
    }
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(golden).expect("read v2 golden contract"))
            .expect("golden JSON");
    assert_eq!(actual, expected);

    let from_rust_directory = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace.join("rust"))
        .args([
            "kernel",
            "fslc/tests/fixtures/domain_characterization/expressions_valid.fsl",
            "--kernel-version",
            "2",
        ])
        .output()
        .expect("run native CLI below repository root");
    assert!(from_rust_directory.status.success());
    let from_rust_directory: Value =
        serde_json::from_slice(&from_rust_directory.stdout).expect("subdirectory JSON");
    assert_eq!(from_rust_directory, actual);
}

#[test]
fn native_cli_negotiates_kernel_and_conformance_majors_fail_closed() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let relative = "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl";
    let conformance = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace)
        .args([
            "conformance",
            relative,
            "--depth",
            "0",
            "--kernel-version",
            "2",
        ])
        .output()
        .expect("run v2 conformance");
    assert!(conformance.status.success());
    let conformance: Value =
        serde_json::from_slice(&conformance.stdout).expect("v2 conformance JSON");
    assert_eq!(conformance["schema_version"], "2.0.0");
    assert_eq!(conformance["kernel_schema_version"], "2.0.0");

    let unsupported = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace)
        .args(["kernel", relative, "--kernel-version", "3"])
        .output()
        .expect("run unsupported major");
    assert_eq!(unsupported.status.code(), Some(2));
    let unsupported: Value = serde_json::from_slice(&unsupported.stdout).expect("error envelope");
    assert_eq!(unsupported["result"], "error");
    assert_eq!(unsupported["kind"], "semantics");
    assert!(
        unsupported["message"]
            .as_str()
            .is_some_and(|message| { message.contains("unsupported public Kernel major") })
    );

    let unsupported_spelling = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace)
        .args(["kernel", relative, "--kernel-version", "2.0.0"])
        .output()
        .expect("run unsupported version spelling");
    assert_eq!(unsupported_spelling.status.code(), Some(2));
}

#[test]
fn published_v2_schema_ids_match_the_rust_api_constants() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let kernel: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/kernel.v2.schema.json"))
            .expect("read v2 Kernel schema"),
    )
    .expect("v2 Kernel schema JSON");
    let conformance: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/conformance.v2.schema.json"))
            .expect("read v2 conformance schema"),
    )
    .expect("v2 conformance schema JSON");
    assert_eq!(kernel["$id"], fsl_core::KERNEL_V2_SCHEMA_ID);
    assert_eq!(kernel["properties"]["schema_version"]["const"], "2.0.0");
    assert_eq!(conformance["$id"], fslc_rust::CONFORMANCE_V2_SCHEMA_ID);
    assert_eq!(
        conformance["properties"]["kernel_schema_version"]["const"],
        "2.0.0"
    );
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
    let testgen_trace: Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace.join("schemas/fslc/kernel/testgen-trace.v1.schema.json"),
        )
        .expect("read testgen trace schema"),
    )
    .expect("testgen trace schema JSON");
    let replay_trace: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/replay-trace.v1.schema.json"))
            .expect("read replay trace schema"),
    )
    .expect("replay trace schema JSON");
    let reproducer: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/reproducer.v1.schema.json"))
            .expect("read reproducer schema"),
    )
    .expect("reproducer schema JSON");
    let test_plan: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("schemas/fslc/kernel/test-plan.v1.schema.json"))
            .expect("read test plan schema"),
    )
    .expect("test plan schema JSON");
    assert_eq!(kernel["$id"], fsl_core::KERNEL_SCHEMA_ID);
    assert_eq!(conformance["$id"], fslc_rust::CONFORMANCE_SCHEMA_ID);
    assert_eq!(testgen_trace["$id"], fsl_core::TESTGEN_TRACE_V1_SCHEMA_ID);
    assert_eq!(replay_trace["$id"], fsl_core::REPLAY_TRACE_V1_SCHEMA_ID);
    assert_eq!(reproducer["$id"], fsl_core::REPRODUCER_V1_SCHEMA_ID);
    assert_eq!(test_plan["$id"], fsl_core::TEST_PLAN_V1_SCHEMA_ID);
    assert_eq!(
        replay_trace["properties"]["schema_version"]["enum"],
        json!([
            fsl_core::REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION,
            fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION,
            fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION
        ])
    );
    assert_eq!(
        replay_trace["properties"]["kernel_schema_version"]["enum"],
        json!([
            fsl_core::KERNEL_V1_SCHEMA_VERSION,
            fsl_core::KERNEL_V2_SCHEMA_VERSION
        ])
    );
    assert_eq!(
        replay_trace["$defs"]["event"]["properties"]["action"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        replay_trace["$defs"]["event"]["allOf"][0]["then"]["properties"]["params"]["maxProperties"],
        0
    );
    assert_eq!(
        replay_trace["allOf"][0]["then"]["properties"]["events"]["items"]["properties"]["action"]["type"],
        "string"
    );
    let kinds = kernel["$defs"]["expression"]["properties"]["kind"]["enum"]
        .as_array()
        .expect("expression kind enum");
    assert!(kinds.contains(&Value::String("ite".to_owned())));
    assert!(!kinds.contains(&Value::String("totally_unknown".to_owned())));
}

/// Detector: empty relation initializers must export through `fslc kernel` as
/// schema-valid `set_lit` values. Mutation: delete the proposed empty-
/// `TypeRef::Relation` arm in `public_kernel.rs`; before the fix that restores
/// exit 2, `kind="semantics"`, and `message="collection literal type mismatch"`.
#[test]
fn empty_relation_initializer_exports_as_schema_valid_kernel() {
    let fixture_path = "rust/fslc/tests/fixtures/issue_467_relation_demo.fsl";

    let (check_output, check_status) = run_cli(&["check", fixture_path]);
    assert_eq!(check_status, 0, "{check_output:#}");
    assert_eq!(check_output["result"], "ok");

    let (kernel_output, kernel_status) = run_cli(&["kernel", fixture_path]);
    assert_eq!(kernel_status, 0, "{kernel_output:#}");
    assert_eq!(kernel_output["result"], "kernel");
    compiled_kernel_v1_schema()
        .validate(&kernel_output)
        .expect("kernel output validates against public Kernel v1 schema");

    assert_eq!(
        kernel_state(&kernel_output, "delegates")["type"]["kind"],
        "relation"
    );
    assert_eq!(
        kernel_state(&kernel_output, "roles")["type"]["kind"],
        "relation"
    );
    assert_empty_relation_init(kernel_init(&kernel_output, "delegates"), "delegates");
    assert_empty_relation_init(kernel_init(&kernel_output, "roles"), "roles");
}

/// Detector: the issue's minimal requirements reproduction must agree on `check`
/// and `kernel`. Mutation: delete the proposed empty-`TypeRef::Relation` arm in
/// `public_kernel.rs`; before the fix that restores exit 2, `kind="semantics"`,
/// and `message="collection literal type mismatch"`.
#[test]
fn minimal_requirements_empty_relation_has_consistent_check_and_kernel() {
    const SOURCE: &str = r"requirements KernelEmptyRelation {
  entity Company
  enum Capability { Skill }
  state { enabled: relation Company -> Capability }
  init { enabled = Set {} }
}
verify { instances Company = 1 }
";
    let dir = scratch_dir("minimal-empty-relation");
    let path = dir.join("kernel_empty_relation.fsl");
    fs::write(&path, SOURCE).expect("write minimal reproduction");
    let path = path.to_str().expect("UTF-8 path");

    let (check_output, check_status) = run_cli(&["check", path, "--strict-tags"]);
    assert_eq!(check_status, 0, "{check_output:#}");
    assert_eq!(check_output["result"], "ok");

    let (kernel_output, kernel_status) = run_cli(&["kernel", path]);
    assert_eq!(kernel_status, 0, "{kernel_output:#}");
    assert_eq!(kernel_output["result"], "kernel");
    assert_eq!(kernel_output["spec"]["name"], "KernelEmptyRelation");
    assert_eq!(kernel_output["spec"]["source"]["dialect"], "requirements");
    assert_eq!(
        kernel_state(&kernel_output, "enabled")["type"]["kind"],
        "relation"
    );
    assert_empty_relation_init(kernel_init(&kernel_output, "enabled"), "enabled");
}

/// Preservation control: a non-empty relation literal must remain a semantic
/// rejection on both public paths after the projection fix.
#[test]
fn nonempty_relation_literal_remains_a_semantic_rejection() {
    const SOURCE: &str = r"requirements KernelEmptyRelation {
  entity Company
  enum Capability { Skill }
  state { enabled: relation Company -> Capability }
  init { enabled = Set { 0 } }
}
verify { instances Company = 1 }
";
    const EXPECTED_MESSAGE: &str = "invalid init statement: relation literals only support the empty initializer Set {}; use r = r.add(a, b) to add pairs at 5:10";

    let dir = scratch_dir("nonempty-relation-literal");
    let path = dir.join("nonempty_relation.fsl");
    fs::write(&path, SOURCE).expect("write rejecting source");
    let path = path.to_str().expect("UTF-8 path");

    for command in ["check", "kernel"] {
        let (output, status) = run_cli(&[command, path]);
        assert_eq!(status, 2, "{command}: {output:#}");
        assert_eq!(output["result"], "error", "{command}");
        assert_eq!(output["kind"], "semantics", "{command}");
        assert_eq!(
            output["message"].as_str(),
            Some(EXPECTED_MESSAGE),
            "{command}"
        );
    }
}
