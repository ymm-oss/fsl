// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native CLI")
}

fn contract() -> Value {
    let output = run(&["--cli-contract"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let published: Value = serde_json::from_slice(&output.stdout).expect("published CLI contract");
    let checked_in: Value = serde_json::from_str(include_str!("../cli-contract.json"))
        .expect("checked-in CLI contract");
    assert_eq!(published, checked_in);
    published
}

fn walk<'a>(node: &'a Value, nodes: &mut Vec<&'a Value>) {
    nodes.push(node);
    for child in node["commands"].as_array().expect("commands") {
        walk(child, nodes);
    }
}

#[test]
fn native_cli_help_matches_the_embedded_contract_at_every_command_path() {
    let contract = contract();
    assert_eq!(contract["schema"], "fsl-cli-contract.v1");
    let mut nodes = Vec::new();
    walk(&contract["root"], &mut nodes);
    let mut paths = BTreeSet::new();

    for node in nodes {
        assert_eq!(
            node.as_object()
                .expect("CLI node")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["actions", "commands", "help", "path", "prog"])
        );
        let path = node["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("path part"))
            .collect::<Vec<_>>();
        assert!(paths.insert(path.clone()), "duplicate CLI path: {path:?}");
        let destinations = node["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .map(|action| action["dest"].as_str().expect("action destination"))
            .collect::<Vec<_>>();
        assert_eq!(
            destinations.len(),
            destinations.iter().collect::<BTreeSet<_>>().len(),
            "duplicate action destination at {path:?}"
        );

        let mut args = path;
        args.push("--help");
        let output = run(&args);
        assert!(output.status.success(), "help failed for {args:?}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 help"),
            node["help"].as_str().expect("contract help"),
            "help drift at {args:?}"
        );
    }

    let invalid_engine = run(&["verify", "specs/cart_v1.fsl", "--engine", "explict"]);
    assert_eq!(invalid_engine.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&invalid_engine.stdout)
            .contains("--engine must be bmc, induction, explicit, or auto")
    );

    let unknown = run(&["not-a-command", "--help"]);
    assert_eq!(unknown.status.code(), Some(2));

    let help_after_argument = run(&["verify", "specs/cart_v1.fsl", "--help"]);
    assert!(help_after_argument.status.success());
    let mut nodes = Vec::new();
    walk(&contract["root"], &mut nodes);
    let verify = nodes
        .into_iter()
        .find(|node| node["path"] == serde_json::json!(["verify"]))
        .expect("verify command");
    assert_eq!(
        String::from_utf8(help_after_argument.stdout).expect("UTF-8 help"),
        verify["help"].as_str().expect("verify help")
    );
}

#[test]
fn native_cli_envelopes_match_the_published_schema() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("schemas/fslc/envelope.v1.schema.json"))
            .expect("read envelope schema"),
    )
    .expect("parse envelope schema");
    let required = schema["required"].as_array().expect("required fields");

    for args in [
        vec!["check", "specs/cart_v1.fsl"],
        vec![
            "verify",
            "examples/gallery/valid/tiny_turnstile.fsl",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
    ] {
        let output = run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).expect("result envelope");
        assert!(
            required
                .iter()
                .all(|field| envelope.get(field.as_str().expect("field")).is_some())
        );

        assert_versions(&envelope["versions"]);

        if args[0] == "verify" {
            assert_verification_cost(&envelope["cost"]);
        }
    }
}

fn assert_versions(value: &Value) {
    let versions = value.as_object().expect("versions");
    assert_eq!(
        versions.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["core", "solver", "verifier"])
    );
    assert_eq!(versions["verifier"]["name"], "fslc-rust");
    assert_eq!(versions["verifier"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(versions["core"]["name"], "fsl-core");
    assert_eq!(versions["core"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(versions["solver"]["name"], "z3");
    assert_eq!(versions["solver"]["backend"], "native-z3");
    assert!(
        versions["solver"]["version"]
            .as_str()
            .expect("solver version")
            .starts_with("Z3 4.16.0")
    );
}

fn assert_verification_cost(value: &Value) {
    let cost = value.as_object().expect("verification cost");
    assert_eq!(
        cost.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["elapsed_s", "properties", "solver"])
    );
    let elapsed = cost["elapsed_s"].as_f64().expect("elapsed seconds");
    assert!(elapsed >= 0.0);
    let solver = cost["solver"].as_object().expect("solver cost");
    assert_eq!(
        solver.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "check_elapsed_s",
            "checks",
            "conflicts",
            "decisions",
            "memory_mb",
            "propagations",
        ])
    );
    assert!(solver["checks"].as_u64().expect("checks") > 0);
    let check_elapsed = solver["check_elapsed_s"]
        .as_f64()
        .expect("solver elapsed seconds");
    assert!((0.0..=elapsed).contains(&check_elapsed));
    for field in ["conflicts", "decisions", "propagations", "memory_mb"] {
        assert!(
            solver[field].is_null()
                || solver[field]
                    .as_f64()
                    .is_some_and(|measurement| measurement >= 0.0),
            "invalid solver measurement: {field}"
        );
    }
    let properties = cost["properties"].as_array().expect("property costs");
    assert!(properties.windows(2).all(|pair| {
        (
            pair[0]["kind"].as_str().expect("property kind"),
            pair[0]["name"].as_str().expect("property name"),
        ) <= (
            pair[1]["kind"].as_str().expect("property kind"),
            pair[1]["name"].as_str().expect("property name"),
        )
    }));
    assert_eq!(
        properties
            .iter()
            .map(|property| property["checks"].as_u64().expect("property checks"))
            .sum::<u64>(),
        solver["checks"].as_u64().expect("solver checks")
    );
    for property in properties {
        let property = property.as_object().expect("property cost");
        assert_eq!(
            property.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(["checks", "elapsed_s", "kind", "name"])
        );
        assert!(!property["kind"].as_str().expect("property kind").is_empty());
        assert!(!property["name"].as_str().expect("property name").is_empty());
        assert!(property["checks"].as_u64().expect("property checks") > 0);
        assert!(property["elapsed_s"].as_f64().expect("property elapsed") >= 0.0);
    }
}

fn collect_schemas(directory: &Path, schemas: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("read schema directory") {
        let path = entry.expect("schema entry").path();
        if path.is_dir() {
            collect_schemas(&path, schemas);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            schemas.push(path);
        }
    }
}

#[test]
fn published_schema_inventory_is_complete_and_parseable() {
    let mut schemas = Vec::new();
    collect_schemas(&root().join("schemas/fslc"), &mut schemas);
    schemas.sort();
    assert_eq!(schemas.len(), 34, "published schema inventory changed");
    let mut ids = BTreeSet::new();
    for path in schemas {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read published schema"))
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let id = schema["$id"]
            .as_str()
            .unwrap_or_else(|| panic!("{} has no $id", path.display()));
        assert!(ids.insert(id.to_owned()), "duplicate schema $id: {id}");
    }
}

#[test]
fn workspace_packages_are_not_publishable() {
    let root = root();
    let metadata = Command::new("cargo")
        .args([
            "metadata",
            "--manifest-path",
            "rust/Cargo.toml",
            "--no-deps",
            "--locked",
            "--format-version",
            "1",
        ])
        .current_dir(&root)
        .output()
        .expect("run cargo metadata");
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: Value = serde_json::from_slice(&metadata.stdout).expect("cargo metadata JSON");
    let packages = metadata["packages"].as_array().expect("workspace packages");
    assert!(!packages.is_empty());
    assert!(
        packages
            .iter()
            .all(|package| package["publish"].as_array().is_some_and(Vec::is_empty))
    );
    assert!(!root.join(".github/workflows/publish.yml").exists());
}
