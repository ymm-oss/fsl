// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for `fslc chain` (issues #489, #500): a manifest reader
//! that silently discards an unparseable `depth`, silently treats zero
//! recognized layer sections as success, or fails the `[impl]` layer for the
//! documented bare-filename invocation is a confidently-green false negative
//! (AGENTS.md). Every test here fails if the corresponding fix is reverted.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn chain_fixture_dir() -> PathBuf {
    repo_root().join("tests/fixtures/chain")
}

/// A fresh scratch directory per test under `rust/target/`, so parallel test
/// binaries never collide and no cleanup step is required (gitignored).
fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root().join(format!(
        "rust/target/chain-cli-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// The committed fixture manifests' `[impl] command` invokes `python -c
/// "print('impl ok')"`. `python` is not reliably on PATH on Windows CI
/// runners, and the nested `\"..\"` quoting is interpreted differently
/// under `cmd /C` than under `sh -c` (the exact class of shell-portability
/// gap issue #500 is itself about). Rewriting the committed fixture would
/// ripple into the frozen Python compatibility suite that also reads it, so
/// only the scratch-dir *copy* is patched, to this same test binary's own
/// executable: no interpreter dependency, and a single unquoted absolute
/// path plus plain arguments behaves identically under both shells (no
/// nested quoting is introduced). `business.fsl` is always present in the
/// copy and independently verified by the `[business]` layer that already
/// runs earlier in the same chain, so it is a reliable, deterministic
/// success.
const PYTHON_IMPL_COMMAND: &str = "command = \"python -c \\\"print('impl ok')\\\"\"";

fn portable_impl_command_line() -> String {
    // TOML/JSON string escaping for the path itself (Windows paths carry
    // `\`), not shell quoting — the shell command has no embedded quotes.
    let escaped_path = env!("CARGO_BIN_EXE_fslc").replace('\\', "\\\\");
    format!("command = \"{escaped_path} check business.fsl\"")
}

/// `git checkout` on Windows CI (`core.autocrlf=true`) rewrites this
/// repository's LF fixture text to CRLF; `fs::read_to_string` (unlike C
/// stdio text mode) never translates line endings back, so the checked-out
/// bytes carry a literal `\r` before every `\n`. Every subsequent
/// line-literal match in this file (this function's own
/// [`PYTHON_IMPL_COMMAND`] replace, and each test's own `"depth = 2\n"` /
/// `"refine_against = \"requirements\"\n"` replace against the manifest
/// this function writes into the scratch dir) assumes bare `\n`, so
/// `copy_chain_fixture` normalizes once here — the one place every scratch
/// copy passes through — rather than requiring every downstream read to
/// repeat the normalization.
fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn copy_chain_fixture(dir: &Path) {
    for entry in fs::read_dir(chain_fixture_dir()).expect("read chain fixture dir") {
        let entry = entry.expect("fixture entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let dest = dir.join(path.file_name().expect("file name"));
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let manifest = fs::read_to_string(&path).expect("read manifest fixture");
            let manifest = normalize_line_endings(&manifest);
            assert!(
                manifest.contains(PYTHON_IMPL_COMMAND),
                "fixture {} no longer has the expected [impl] command line to rewrite",
                path.display()
            );
            let manifest = manifest.replace(PYTHON_IMPL_COMMAND, &portable_impl_command_line());
            fs::write(&dest, manifest).expect("write portable manifest copy");
        } else {
            fs::copy(&path, &dest).expect("copy fixture file");
        }
    }
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run native fslc")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn layer<'a>(value: &'a Value, name: &str) -> &'a Value {
    value["layers"]
        .as_array()
        .expect("layers array")
        .iter()
        .find(|entry| entry["layer"] == name)
        .unwrap_or_else(|| panic!("layer '{name}' present in {value}"))
}

#[test]
fn chain_clean_manifest_runs_five_layers_and_verifies() {
    let dir = scratch_dir("clean");
    copy_chain_fixture(&dir);

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(
        output.status.success(),
        "clean manifest chain failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["result"], "verified");
    assert_eq!(value["layers"].as_array().expect("layers").len(), 5);
}

#[test]
fn chain_default_and_bare_filename_invocations_run_impl_layer() {
    // Regression for #500: `Path::parent()` on a bare filename (no directory
    // component) returns `Some("")`, and `Command::current_dir("")` used to
    // fail with an io error, so the documented default invocation never
    // reached the [impl] layer.
    let dir = scratch_dir("bare-filename");
    copy_chain_fixture(&dir);

    for args in [vec!["chain", "fsl-project.toml"], vec!["chain"]] {
        let output = run(&dir, &args);
        assert!(
            output.status.success(),
            "chain {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json(&output);
        assert_eq!(value["result"], "verified", "chain {args:?}: {value}");
        let impl_layer = layer(&value, "impl");
        assert_eq!(impl_layer["status"], "passed");
        assert_eq!(impl_layer["exit_code"], 0);
    }
}

#[test]
fn chain_rejects_manifest_with_inline_comment_depth() {
    // Regression for #489 (path 1): a TOML inline comment after `depth`
    // failed the bare `usize` parse and used to fall back silently to the
    // hardcoded default of 8 via `unwrap_or(8)`, understating a declared
    // depth without any diagnostic.
    let dir = scratch_dir("inline-comment-depth");
    copy_chain_fixture(&dir);
    let manifest = fs::read_to_string(dir.join("fsl-project.toml")).expect("read manifest");
    let manifest = manifest.replace("depth = 2\n", "depth = 2  # keep it deep\n");
    assert!(
        manifest.contains("# keep it deep"),
        "fixture no longer has a `depth = 2` line to rewrite"
    );
    fs::write(dir.join("fsl-project.toml"), manifest).expect("write manifest");

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    let design_layer = layer(&value, "design");
    assert_eq!(design_layer["status"], "failed");
    assert_eq!(design_layer["exit_code"], 2);
    let message = design_layer["detail"]["message"]
        .as_str()
        .expect("error message");
    assert!(message.contains("invalid depth value"), "{message}");
}

#[test]
fn chain_omitted_depth_still_defaults() {
    // Distinguishes an absent `depth` key (falls back to the `check` path)
    // from a present-but-unparseable one (previous test): only omission may
    // default silently.
    let dir = scratch_dir("omitted-depth");
    copy_chain_fixture(&dir);
    let manifest = fs::read_to_string(dir.join("fsl-project.toml")).expect("read manifest");
    let manifest = manifest.replace("depth = 2\n", "");
    fs::write(dir.join("fsl-project.toml"), manifest).expect("write manifest");

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["result"], "verified");
    assert_eq!(layer(&value, "design")["kind"], "check");
}

#[test]
fn chain_rejects_malformed_refine_depth() {
    // A present-but-invalid `refine_depth` must not silently fall through to
    // `depth` or the target layer's `depth` (or the hardcoded default).
    let dir = scratch_dir("malformed-refine-depth");
    copy_chain_fixture(&dir);
    let manifest = fs::read_to_string(dir.join("fsl-project.toml")).expect("read manifest");
    let manifest = manifest.replace(
        "refine_against = \"requirements\"\n",
        "refine_against = \"requirements\"\nrefine_depth = notanumber\n",
    );
    fs::write(dir.join("fsl-project.toml"), manifest).expect("write manifest");

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    let refine_layer = layer(&value, "design->requirements");
    assert_eq!(refine_layer["status"], "failed");
    assert_eq!(refine_layer["exit_code"], 2);
    let message = refine_layer["detail"]["message"]
        .as_str()
        .expect("error message");
    assert!(message.contains("invalid refine_depth value"), "{message}");
}

#[test]
fn chain_rejects_empty_manifest() {
    // Regression for #489 (path 2): zero recognized layer sections used to
    // report `result: "verified"` with `layers: []` at exit 0.
    let dir = scratch_dir("empty-manifest");
    fs::write(dir.join("fsl-project.toml"), "").expect("write empty manifest");

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert!(value.get("layers").is_none(), "{value}");
}

#[test]
fn chain_rejects_unknown_section_name() {
    // A misspelled layer section name (e.g. `[businesss]`) used to be
    // ignored rather than diagnosed, silently dropping that layer from the
    // chain even when it sits alongside otherwise-valid sections.
    let dir = scratch_dir("unknown-section");
    fs::write(
        dir.join("fsl-project.toml"),
        "[businesss]\nfile = \"business.fsl\"\n\n[requirements]\nfile = \"requirements.fsl\"\n",
    )
    .expect("write manifest");

    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert!(value.get("layers").is_none(), "{value}");
    let message = value["message"].as_str().expect("error message");
    assert!(message.contains("businesss"), "{message}");
}

const DEPTH_GATE_SPEC: &str = r"spec DepthGate {
  type Counter = 0..12

  state {
    counter: Counter
  }

  init {
    counter = 0
  }

  action tick() {
    requires counter < 12
    counter = counter + 1
  }

  invariant BelowNine { counter < 9 }
}
";

#[test]
fn chain_honors_declared_depth_beyond_the_silent_fallback_of_eight() {
    // `DepthGate`'s invariant only breaks at step 9: depth 8 (the old
    // silent-fallback value) reports `verified`, depth 9 reports `violated`.
    // A manifest that declares depth = 9 must actually search 9 steps
    // (proving #489's fix does not just error out but also uses the
    // declared value correctly), while the same depth written with a
    // trailing inline comment must fail closed instead of quietly reusing 8
    // and missing the counterexample.
    let dir = scratch_dir("depth-gate");
    fs::write(dir.join("depth_gate.fsl"), DEPTH_GATE_SPEC).expect("write spec");

    fs::write(
        dir.join("fsl-project.toml"),
        "[business]\nfile = \"depth_gate.fsl\"\ndepth = 9\n",
    )
    .expect("write manifest");
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["result"], "violated");

    fs::write(
        dir.join("fsl-project.toml"),
        "[business]\nfile = \"depth_gate.fsl\"\ndepth = 9  # keep it deep\n",
    )
    .expect("write manifest with inline comment");
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["result"], "error");
}
