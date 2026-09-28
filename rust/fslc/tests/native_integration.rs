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

/// The signed release manifest mise reads, and the two things that can make it
/// describe something other than this release.
fn assert_packslip_release_contract(workflow: &str, root: &Path) {
    // One run per command, because the release publishes two commands as
    // separate bare executables and one project cannot hold two `raw`
    // artifacts for one platform.
    assert_eq!(workflow.matches("uses: jdx/packslip@").count(), 2);
    // The project names the command, and the bundle file name follows it.
    // Users write the project in `mise.toml` and mise keys its signer pin by
    // it, so the first release that carries a packslip fixes the name.
    assert!(workflow.contains("project: github.com/ymm-oss/fsl/fslc\n"));
    assert!(workflow.contains("project: github.com/ymm-oss/fsl/fslc-lsp\n"));

    // Only the signing job can mint an OIDC token, and it writes nothing to
    // the release. The job that drafts the release and the job that makes it
    // public hold `contents: write` and nothing more.
    // A job runs from its key to the next line indented by exactly two
    // spaces, which is the next job or the comment above it.
    let job = |name: &str| {
        let start = workflow
            .find(&format!("\n  {name}:\n"))
            .unwrap_or_else(|| panic!("the {name} job"))
            + 1;
        let body = &workflow[start..];
        let end = body
            .match_indices("\n  ")
            .map(|(at, _)| at)
            .find(|&at| !body[at + 3..].starts_with(' '))
            .unwrap_or(body.len());
        &body[..end]
    };
    let (publish, sign, release) = (job("publish"), job("sign"), job("release"));
    assert_eq!(workflow.matches("id-token: write").count(), 1);
    assert!(sign.contains("id-token: write") && sign.contains("attestations: write"));
    assert!(sign.contains("contents: read") && !sign.contains("contents: write"));
    assert!(!sign.contains("gh release"));
    for job in [publish, release] {
        assert!(job.contains("contents: write"));
        assert!(!job.contains("id-token") && !job.contains("attestations"));
        assert!(!job.contains("uses: jdx/packslip@"));
    }
    assert!(sign.contains("needs: [publish]") && release.contains("needs: [sign]"));

    // The action would upload each bundle as soon as it is signed, before the
    // gate runs and before the other bundle exists. A bundle left on the draft
    // by a failed attempt then fails the exact asset check when the job runs
    // again, so the bundles stay local until the gate passes.
    assert_eq!(workflow.matches("uses: jdx/packslip@").count(), 2);
    assert_eq!(sign.matches("          upload: false\n").count(), 2);
    assert!(release.contains("--clobber"));
    let uploaded = release
        .find("gh release upload")
        .expect("the bundle upload");
    let rechecked = release
        .find("diff -u expected-assets.txt remote-assets.txt")
        .expect("the check after the upload");
    let made_public = release
        .find("--draft=false --latest")
        .expect("the step that makes the release public");
    assert!(uploaded < rechecked && rechecked < made_public);
    assert!(release.contains("echo packslip.fslc.sigstore.json"));
    assert!(release.contains("echo packslip.fslc-lsp.sigstore.json"));
    assert!(release.contains("is not the bundle the gate passed."));

    // Step outputs reach `run:` through `env:`, never by expansion into the
    // script text.
    assert!(!workflow.contains("'${{ steps."));
    assert!(sign.contains("FSLC_BUNDLE: ${{ steps.packslip-fslc.outputs.bundle }}"));

    // Every asset named in full. A `fslc-*` glob also matches every
    // `fslc-lsp-*` asset, which is the trap the mise `matching` option sets.
    for command in ["fslc", "fslc-lsp"] {
        for target in ["macos-arm64", "linux-x64", "linux-arm64", "windows-x64.exe"] {
            let asset = format!("release-assets/{command}-{target}\n");
            assert!(workflow.contains(&asset), "packslip lost {asset}");
        }
    }
    assert!(!workflow.contains("release-assets/fslc-*"));

    // The skills come from the directory, never from a list beside it.
    assert!(workflow.contains("skill/&=repo:skills/&"));
    assert!(workflow.contains("resources: ${{ steps.skill-resources.outputs.value }}"));

    // The draft is checked against `release-assets/` exactly before anything
    // is signed.
    assert!(publish.contains("diff -u expected-assets.txt remote-assets.txt"));
    let gated = sign
        .find("diff -u present-skills.txt declared-skills.txt")
        .expect("the gate");
    let handed_over = sign.find("name: packslips").expect("the bundle hand-over");
    assert!(sign.find("uses: jdx/packslip@").expect("the signing") < gated && gated < handed_over);

    // The platform of an artifact is read off its file name, and that is what
    // selects the binary a user is handed.
    for claim in [
        r#"{"name":"fslc-macos-arm64","os":"darwin","arch":"aarch64","libc":null,"format":"raw"}"#,
        r#"{"name":"fslc-lsp-windows-x64.exe","os":"windows","arch":"x86_64","libc":null,"format":"raw"}"#,
    ] {
        assert!(workflow.contains(claim), "packslip lost the claim {claim}");
    }

    // mise reads the manifest. The README has to name the backend that does.
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README");
    assert!(readme.contains("\"packslip:github.com/ymm-oss/fsl/fslc\""));
    assert!(readme.contains("\"packslip:github.com/ymm-oss/fsl/fslc-lsp\""));
    assert!(readme.contains("mise skills sync"));
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

    // A symbolic link this installer did not create belongs to whatever put it
    // there. `mise skills sync` links into its own versioned payload exactly as
    // this installer does, and taking that link leaves the other tool pointing
    // at nothing it knows about. A link whose target is gone is abandoned, so
    // `[ -e ... ]` keeps it replaceable.
    assert!(installer.contains("foreign_skill_link() {"));
    assert!(installer.contains("which another tool placed."));
    let guard = installer.find("foreign_skill_link() {").expect("the guard");
    let preflight = installer
        .find("if foreign_skill_link \"$destination\" \"$source\"; then")
        .expect("the guard runs before any write");
    let backup = installer
        .find("SKILL_BACKUP=\"$SKILL_DST.pre-native-v3\"")
        .expect("the backup path");
    let placement = installer
        .find("elif foreign_skill_link \"$SKILL_DST\" \"$SKILL_SRC\"; then")
        .expect("the guard runs before the backup");
    assert!(
        guard < preflight && preflight < placement && placement < backup,
        "a link another tool placed must be reached before the move aside"
    );
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
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node["commands"].as_array().is_some_and(Vec::is_empty))
            .count(),
        52,
        "the public contract must enumerate every live native leaf"
    );
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

    for path in [
        vec!["causal"],
        vec!["causal", "check"],
        vec!["causal", "analyze"],
        vec!["causal", "verify-expectations"],
        vec!["causal", "observe-expectations"],
        vec!["causal", "diff"],
        vec!["causal", "ledger"],
    ] {
        assert!(paths.contains(&path), "missing public CLI path: {path:?}");
    }
    assert!(
        !paths.contains(&vec!["causal", "verify"]),
        "causal review must not acquire a proof-like verify command"
    );

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
    assert_eq!(schemas.len(), 46, "published schema inventory changed");
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
    assert!(workflow.contains(
        "gh release edit \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" --draft=false --latest"
    ));
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
    assert!(workflow.contains(
        "./tools/check-release-binary-linkage.sh rust/target/release/fslc rust/target/release/fslc-lsp"
    ));
    let release_abi = std::fs::read_to_string(root.join("tools/check-release-binary-linkage.sh"))
        .expect("release ABI guard");
    assert!(release_abi.contains("readelf --version-info"));
    assert!(release_abi.contains("GLIBC_2.39"));
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
        "uses: jdx/packslip@v1",
    ] {
        assert!(
            !workflow.contains(mutable),
            "mutable release action: {mutable}"
        );
    }
    assert!(workflow.contains("toolchain: 1.88.0"));
    assert_packslip_release_contract(&workflow, &root);
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

    let runbook =
        std::fs::read_to_string(root.join("docs/design/RELEASE.md")).expect("release runbook");
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

/// Runs the installer's own guard against real links. A link an earlier run of
/// the installer made must still be migrated, under any data directory, while
/// a link another tool placed, such as one `mise skills sync` made, is kept.
#[cfg(not(windows))]
#[test]
fn installer_migrates_its_own_skill_links_and_keeps_foreign_ones() {
    let installer = std::fs::read_to_string(root().join("install.sh")).expect("installer");
    let start = installer
        .find("installer_skill_link() {")
        .expect("the legacy link rule");
    let guard = installer.find("foreign_skill_link() {").expect("the guard");
    let end = guard + installer[guard..].find("\n}\n").expect("the guard's end") + 3;
    let functions = &installer[start..end];

    let dir = std::env::temp_dir().join(format!("fsl-installer-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let make = |path: &str| {
        std::fs::create_dir_all(dir.join(path)).unwrap();
        dir.join(path)
    };
    // Each link stands where the installer puts one: named after its skill.
    let link = |target: &Path, case: &str| {
        let path = make(&format!("links/{case}")).join("fsl");
        std::os::unix::fs::symlink(target, &path).unwrap();
        path
    };

    // The payload this run installs.
    make("data/releases/new/skills/fsl");
    std::os::unix::fs::symlink("releases/new", dir.join("data/current")).unwrap();
    let source = dir.join("data/current/skills/fsl");

    // A pre-native clone, `~/.fsl` by default.
    make("clone/.git");
    std::fs::write(make("clone").join("install.sh"), "").unwrap();
    make("clone/skills/fsl");
    // A native payload under a data directory this run no longer uses.
    make("old-data/releases/old/skills/fsl");
    std::os::unix::fs::symlink("releases/old", dir.join("old-data/current")).unwrap();
    // What `mise skills sync` links to.
    make("mise/installs/fslc/4.8.0/.mise-packslip/repo/skills/fsl");
    // Look-alikes that no run of this installer made.
    make("other/current/skills/fsl");
    make("plain/skills/fsl");

    let run_guard = |destination: &Path| {
        Command::new("bash")
            .arg("-c")
            .arg(format!("{functions}\nforeign_skill_link \"$1\" \"$2\""))
            .arg("guard")
            .arg(destination)
            .arg(&source)
            .status()
            .unwrap()
            .success()
    };
    let foreign = [
        link(
            &dir.join("mise/installs/fslc/4.8.0/.mise-packslip/repo/skills/fsl"),
            "mise",
        ),
        link(&dir.join("other/current/skills/fsl"), "other"),
        link(&dir.join("plain/skills/fsl"), "plain"),
    ];
    let ours = [
        link(&dir.join("clone/skills/fsl"), "clone"),
        link(&dir.join("old-data/current/skills/fsl"), "old-data"),
        link(&source, "current"),
        link(&dir.join("gone/skills/fsl"), "broken"),
        make("links/real-directory/fsl"),
    ];
    for destination in &foreign {
        assert!(
            run_guard(destination),
            "{} must be kept",
            destination.display()
        );
    }
    for destination in &ours {
        assert!(
            !run_guard(destination),
            "{} must be migrated",
            destination.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
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
