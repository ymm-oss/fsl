// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn assert_vendored_z3(root: &Path, workflow: &str) {
    assert!(!workflow.contains("Z3_SYS_Z3_VERSION"));
    assert!(workflow.contains("MACOSX_DEPLOYMENT_TARGET: \"14.0\""));
    assert!(workflow.contains("os: macos-15"));
    assert!(!workflow.contains("os: macos-14"));
    let workspace = std::fs::read_to_string(root.join("rust/Cargo.toml")).expect("workspace");
    assert!(workspace.contains("features = [\"vendored\", \"z3_4_16\"]"));
    assert!(!workspace.contains("\"gh-release\""));
}

fn assert_windows_release_smoke(workflow: &str) {
    assert!(workflow.contains("$env:GITHUB_REF_NAME.Substring(1)"));
    assert!(workflow.contains("binary version does not match tag"));
    assert!(workflow.contains("$verifyExit = $LASTEXITCODE"));
    assert!(workflow.contains("$verifyResult = $verifyOutput | ConvertFrom-Json"));
    assert!(workflow.contains("$verifyResult.result -ne \"verified\""));
}

fn assert_installer_release_contract(root: &Path) {
    let installer = std::fs::read_to_string(root.join("install.sh")).expect("installer");
    assert!(!installer.contains("echo \"macos-x64\""));
    assert!(installer.contains("RELEASE_TAG=$(latest_release_tag)"));
    assert!(!installer.contains("git clone"));
    assert!(!installer.contains("command -v git"));
    assert!(installer.contains("releases/download/$RELEASE_TAG"));
    assert!(!installer.contains("releases/latest/download"));
    assert!(installer.contains("$DATA_HOME/fsl"));
    assert!(installer.contains("RELEASE_NAME=\"$RELEASE_TAG-${CLI_HASH:0:12}\""));
    assert!(installer.contains("mktemp -d \"$INSTALL_DIR/.activate.XXXXXX\""));
    assert!(installer.contains("mv -fh \"$ACTIVATION_LINK\" \"$CURRENT_LINK\""));
    assert!(installer.contains("mv -fT \"$ACTIVATION_LINK\" \"$CURRENT_LINK\""));
    assert!(installer.contains("stage_release_asset \"fsl-skills.tar.gz\""));
    assert!(installer.contains("[ \"$RELEASE_TAG\" != \"v3.0.0\" ]"));
    assert!(installer.contains("archive/refs/tags/$RELEASE_TAG.tar.gz"));
    assert!(installer.contains("d2d691a98af28f4aaa77ded08b35978539a0d1e3c65e8b7f29783f143a447598"));
    assert!(installer.contains("EXPECTED_VERSION=\"fslc ${RELEASE_TAG#v}\""));
    assert!(installer.contains("RESOLVED_VERSION=$(fslc --version"));
    assert!(installer.contains("diff -qr \"$STAGING_DIR\" \"$RELEASE_DIR\""));
    assert!(installer.contains("FSL_DATA_DIR must be an absolute path"));
    assert!(installer.contains("$HOME/.fsl/.venv/bin/$cmd_name"));
    assert!(!installer.contains("*\"/.venv/bin/$cmd_name\""));
    assert!(installer.contains("ln -s \"$SKILL_SRC\" \"$SKILL_DST\""));
    assert!(installer.contains("$SKILL_DST.pre-native-v3"));
    let stage_cli = installer
        .find("stage_release_asset \"fslc-$TARGET\"")
        .unwrap();
    let stage_lsp = installer
        .find("stage_release_asset \"fslc-lsp-$TARGET\"")
        .unwrap();
    let activate = installer
        .find("mv -fh \"$ACTIVATION_LINK\" \"$CURRENT_LINK\"")
        .unwrap();
    let preflight = installer
        .find("preflight_command_link fslc \"$FSL_BIN\"")
        .unwrap();
    let prepare_lsp = installer
        .find("link_command fslc-lsp \"$FSL_LSP_BIN\"")
        .unwrap();
    assert!(
        stage_cli < stage_lsp
            && stage_lsp < preflight
            && preflight < prepare_lsp
            && prepare_lsp < activate
    );
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
    assert_eq!(schemas.len(), 28, "published schema inventory changed");
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

#[test]
fn native_release_unit_is_atomic_pinned_and_platform_closed() {
    let root = root();
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("release workflow")
        .replace("\r\n", "\n");
    assert_eq!(workflow.matches("softprops/action-gh-release@").count(), 1);
    assert!(workflow.contains("name: assemble complete release unit"));
    assert!(workflow.contains("needs: [build, vsix, kernel-contract, skills]"));
    assert!(workflow.contains("name: release-unit"));
    assert!(workflow.contains("name: publish atomic release unit"));
    assert!(workflow.contains("needs: [assemble]"));
    assert!(workflow.contains("merge-multiple: true"));
    assert!(workflow.contains("draft: true"));
    assert!(workflow.contains("body_path: release-notes.md"));
    assert!(workflow.contains("Verify the remote draft release unit"));
    assert!(workflow.contains("diff -u expected-assets.txt remote-assets.txt"));
    assert!(workflow.contains("gh release edit \"$GITHUB_REF_NAME\" --draft=false --latest"));
    assert!(workflow.contains("npm ci"));
    assert!(workflow.contains("cp ../../LICENSE LICENSE"));
    assert!(workflow.contains("npm exec -- vsce package"));
    assert!(workflow.contains("tar -czf dist/fsl-skills.tar.gz"));
    assert!(workflow.contains("fsl-skills.tar.gz fsl-skills.tar.gz.sha256"));
    assert!(!workflow.contains("npx --yes @vscode/vsce"));
    assert_eq!(workflow.matches("            target: ").count(), 4);
    for target in ["macos-arm64", "linux-x64", "linux-arm64", "windows-x64"] {
        assert!(workflow.contains(&format!("target: {target}")));
    }
    assert!(workflow.contains("os: ubuntu-24.04\n            target: linux-x64"));
    assert!(workflow.contains("GLIBC_2.39"));
    assert!(!workflow.contains("target: macos-x64"));
    assert!(workflow.contains("\"fslc ${GITHUB_REF_NAME#v}\""));
    assert_windows_release_smoke(&workflow);
    for mutable in [
        "uses: actions/checkout@v4",
        "uses: actions/setup-node@v4",
        "uses: actions/upload-artifact@v4",
        "uses: actions/download-artifact@v4",
        "uses: dtolnay/rust-toolchain@stable",
        "uses: softprops/action-gh-release@v2",
    ] {
        assert!(
            !workflow.contains(mutable),
            "mutable release action: {mutable}"
        );
    }
    assert!(workflow.contains("toolchain: 1.88.0"));
    assert_vendored_z3(&root, &workflow);
    assert_installer_release_contract(&root);

    let package: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("editors/vscode/package.json"))
            .expect("VS Code package"),
    )
    .expect("VS Code package JSON");
    assert_eq!(package["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(package["devDependencies"]["@vscode/vsce"], "3.9.2");
    let package_lock: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("editors/vscode/package-lock.json"))
            .expect("VS Code package lock"),
    )
    .expect("VS Code package lock JSON");
    assert_eq!(package_lock["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        package_lock["packages"][""]["devDependencies"]["@vscode/vsce"],
        "3.9.2"
    );

    let runbook = std::fs::read_to_string(root.join("docs/RELEASE.md")).expect("release runbook");
    let skill = std::fs::read_to_string(root.join(".claude/skills/release/SKILL.md"))
        .expect("release skill");
    for contract in [
        "short-lived branch -> main -> production -> vX.Y.Z",
        "Never tag `main`",
        "workflow_dispatch",
        "explicit confirmation",
    ] {
        assert!(runbook.contains(contract), "runbook lost {contract}");
        assert!(skill.contains(contract), "skill lost {contract}");
    }
    assert!(runbook.contains("git tag -a vX.Y.Z PRODUCTION_SHA"));
    assert!(skill.contains("tag `vX.Y.Z` at the gated `production` HEAD"));
    assert!(runbook.contains("cannot validate its own first installation"));
    assert!(skill.contains("cannot validate its own first installation"));
    assert_eq!(
        std::fs::canonicalize(root.join(".codex/skills/release")).expect("Codex release link"),
        std::fs::canonicalize(root.join(".claude/skills/release"))
            .expect("canonical release skill")
    );
}

#[test]
fn production_accepts_only_governed_source_branches() {
    let root = root();
    let policy = std::fs::read_to_string(root.join(".github/workflows/production-policy.yml"))
        .expect("production policy")
        .replace("\r\n", "\n");
    assert!(policy.contains("branches: [production]"));
    assert!(policy.contains("  pull_request_target:"));
    assert!(!policy.contains("\n  pull_request:\n"));
    assert!(!policy.contains("actions/checkout@"));
    assert!(policy.contains("HEAD_REF: ${{ github.event.pull_request.head.ref }}"));
    assert!(policy.contains("HEAD_REPO: ${{ github.event.pull_request.head.repo.full_name }}"));
    assert!(policy.contains("REPOSITORY: ${{ github.repository }}"));
    #[cfg(not(windows))]
    {
        let policy_script = policy
            .split_once("        run: |\n")
            .expect("inline production policy")
            .1
            .lines()
            .map(|line| line.strip_prefix("          ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        let run_policy = |head_ref: &str, head_repo: &str| {
            Command::new("bash")
                .arg("-c")
                .arg(&policy_script)
                .env("HEAD_REF", head_ref)
                .env("HEAD_REPO", head_repo)
                .env("REPOSITORY", "expected-repository")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        };

        for accepted in ["main", "release/v3.0", "hotfix/v3.0.1"] {
            assert!(run_policy(accepted, "expected-repository"));
            assert!(!run_policy(accepted, "foreign-repository"));
        }
        for rejected in [
            "feature/release",
            "release/v3",
            "release/v3.0.0",
            "hotfix/v3.0",
            "main-extra",
        ] {
            assert!(!run_policy(rejected, "expected-repository"));
        }
    }
}
