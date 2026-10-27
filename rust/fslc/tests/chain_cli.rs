// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for `fslc chain` (issues #489, #500): a manifest reader
//! that silently discards an unparseable `depth`, silently treats zero
//! recognized layer sections as success, or fails the `[impl]` layer for the
//! documented bare-filename invocation is a confidently-green false negative
//! (AGENTS.md). Every test here fails if the corresponding fix is reverted.

#[cfg(unix)]
use std::fmt::Write as _;
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

fn issue_1475_project(design: &str) -> PathBuf {
    let dir = scratch_dir("1475-missing-own-file");
    fs::copy(
        repo_root().join("tests/fixtures/approval.fsl"),
        dir.join("spec.fsl"),
    )
    .expect("copy checked source");
    fs::write(dir.join("identity.fsl"), "refinement Identity { impl ApprovalReview abs ApprovalReview map approved = approved map ever_approved = ever_approved action approve() -> approve() }\n")
        .expect("write valid mapping");
    fs::write(
        dir.join("project.toml"),
        format!("[requirements]\nfile = \"spec.fsl\"\n[design]\n{design}\n"),
    )
    .expect("write manifest");
    dir
}

fn issue_1475_check_detail(dir: &Path) -> Value {
    let expected = serde_json::json!({
        "fsl":"1.0", "result":"ok", "spec":"ApprovalReview",
        "warnings":[
            {
                "code":"legacy_string_metadata", "kind":"legacy_string_metadata",
                "taxonomy":"deprecated", "severity":"warning", "edition":"current",
                "message":"legacy string metadata is deprecated; use a typed annotation",
                "loc":{"file":"./spec.fsl", "line":12, "column":20, "end_line":12, "end_column":63},
                "canonical_replacement":"@requirement(\"REQ-190\", \"a pending review can be approved\")",
                "machine_applicable":true,
                "edits":[
                    {"start":228, "end":228, "replacement":"@requirement(\"REQ-190\", \"a pending review can be approved\")\n  "},
                    {"start":245, "end":288, "replacement":""}
                ]
            },
            {
                "code":"legacy_string_metadata", "kind":"legacy_string_metadata",
                "taxonomy":"deprecated", "severity":"warning", "edition":"current",
                "message":"legacy string metadata is deprecated; use a typed annotation",
                "loc":{"file":"./spec.fsl", "line":18, "column":30, "end_line":18, "end_column":76},
                "canonical_replacement":"@requirement(\"REQ-190\", \"an approved review remains approved\")",
                "machine_applicable":true,
                "edits":[
                    {"start":369, "end":369, "replacement":"@requirement(\"REQ-190\", \"an approved review remains approved\")\n  "},
                    {"start":396, "end":442, "replacement":""}
                ]
            }
        ]
    });
    let checked = run(dir, &["check", "./spec.fsl"]);
    assert_eq!(checked.status.code(), Some(0));
    assert_eq!(json(&checked)["warnings"], expected["warnings"]);
    expected
}

fn issue_1475_missing_file(jobs: &str) {
    let dir = issue_1475_project("refine_against = \"requirements\"\nmapping = \"identity.fsl\"");
    let output = run(
        &dir,
        &["chain", "project.toml", "--keep-going", "--jobs", jobs],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "chain");
    assert_eq!(
        value["message"],
        "one or more chain layers returned an error"
    );
    assert_eq!(
        value["failed"],
        serde_json::json!(["design", "design->requirements"])
    );
    assert_eq!(value["layers"].as_array().expect("layers").len(), 3);
    assert_eq!(layer(&value, "requirements")["status"], "passed");
    assert_eq!(
        layer(&value, "requirements")["detail"],
        issue_1475_check_detail(&dir)
    );
    for (name, kind) in [("design", "check"), ("design->requirements", "refine")] {
        let entry = layer(&value, name);
        assert_eq!(entry["status"], "failed");
        assert_eq!(entry["kind"], kind);
        assert_eq!(entry["exit_code"], 2);
        assert_eq!(
            entry["detail"],
            serde_json::json!({"result":"error", "kind":"io", "message":"[design] file is required"})
        );
    }
    let again = run(
        &dir,
        &["chain", "project.toml", "--keep-going", "--jobs", jobs],
    );
    assert_eq!(again.status.code(), Some(2));
    assert_eq!(
        value,
        json(&again),
        "entire ordered envelope, no exclusions"
    );
}

#[test]
fn issue_1475_missing_own_file_jobs_one() {
    issue_1475_missing_file("1");
}

#[test]
fn issue_1475_missing_own_file_jobs_two() {
    issue_1475_missing_file("2");
}

#[test]
fn issue_1475_missing_own_file_preserves_cutoff() {
    let dir = issue_1475_project("refine_against = \"requirements\"\nmapping = \"identity.fsl\"");
    for jobs in ["1", "2"] {
        let output = run(&dir, &["chain", "project.toml", "--jobs", jobs]);
        assert_eq!(output.status.code(), Some(2));
        let value = json(&output);
        assert_eq!(value["result"], "error");
        assert_eq!(layer(&value, "requirements")["status"], "passed");
        assert_eq!(
            layer(&value, "design")["detail"],
            serde_json::json!({"result":"error", "kind":"io", "message":"[design] file is required"})
        );
        assert_eq!(
            layer(&value, "design->requirements"),
            &serde_json::json!({
                "layer":"design->requirements", "kind":"refine", "status":"skipped", "result":"skipped", "exit_code":0
            })
        );
    }
}

#[test]
fn issue_1475_missing_target_or_mapping_preserves_precedence() {
    for (design, missing_target_file) in [
        (
            "refine_against = \"absent\"\nmapping = \"identity.fsl\"",
            false,
        ),
        ("refine_against = \"requirements\"", false),
        (
            "refine_against = \"requirements\"\nmapping = \"identity.fsl\"",
            true,
        ),
    ] {
        let dir = issue_1475_project(design);
        if missing_target_file {
            fs::write(
                dir.join("project.toml"),
                format!("[requirements]\n[design]\n{design}\n"),
            )
            .expect("omit only the target section file");
        }
        let output = run(
            &dir,
            &["chain", "project.toml", "--keep-going", "--jobs", "1"],
        );
        assert_eq!(output.status.code(), Some(2));
        let value = json(&output);
        assert_eq!(value["result"], "error");
        let target = if design.contains("absent") {
            "absent"
        } else {
            "requirements"
        };
        assert_eq!(
            layer(&value, &format!("design->{target}"))["detail"],
            serde_json::json!({
                "result":"error", "kind":"io", "message":format!("[design] unknown refine_against layer: {target}")
            })
        );
    }
}

#[test]
fn issue_1475_valid_chain_full_output_is_preserved() {
    let dir = issue_1475_project(
        "file = \"spec.fsl\"\nrefine_against = \"requirements\"\nmapping = \"identity.fsl\"",
    );
    let check_detail = issue_1475_check_detail(&dir);
    let output = run(
        &dir,
        &["chain", "project.toml", "--keep-going", "--jobs", "1"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    // main.rs uses Path::display; the Windows failure log shows native `.\spec.fsl` paths.
    let spec = format!(".{}spec.fsl", std::path::MAIN_SEPARATOR);
    let mapping = format!(".{}identity.fsl", std::path::MAIN_SEPARATOR);
    assert_eq!(
        value,
        serde_json::json!({
            "fsl":"1.0", "result":"verified", "manifest":"project.toml", "keep_going":true,
            "layers":[
                {"layer":"requirements", "kind":"check", "file":spec, "status":"passed", "result":"ok", "exit_code":0,
                 "detail":check_detail},
                {"layer":"design", "kind":"check", "file":spec, "status":"passed", "result":"ok", "exit_code":0,
                 "detail":check_detail},
                {"layer":"design->requirements", "kind":"refine", "file":spec, "against":"requirements", "abs_file":spec, "mapping":mapping, "depth":8,
                 "status":"passed", "result":"refines", "exit_code":0,
                 "detail":{"fsl":"1.0", "impl":"ApprovalReview", "abs":"ApprovalReview", "result":"refines", "checked_to_depth":8, "action_map":{"approve":"approve"}}}
            ]
        })
    );
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

// ---- Issue #1147: `chain` goes through the verify cache -------------------
//
// `chain` used to call `run_verify` directly, so it never read or wrote the
// verify cache: two runs of an unchanged project left the cache directory
// empty. Every test below points `FSLC_CACHE_DIR` at a private directory
// (and clears `FSLC_CACHE`), so none of them observes a developer's cache.

fn run_cached(cwd: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .env("FSLC_CACHE_DIR", cache)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native fslc")
}

fn cache_entry_count(cache: &Path) -> usize {
    fn walk(dir: &Path) -> usize {
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        entries
            .map(|entry| {
                let path = entry.expect("cache entry").path();
                if path.is_dir() { walk(&path) } else { 1 }
            })
            .sum()
    }
    walk(cache)
}

fn layer_cache_hit(value: &Value, name: &str) -> bool {
    layer(value, name)["detail"]["cache"]["hit"] == true
}

/// A chain result with the two things that legitimately differ between a cold
/// and a warm run removed: the additive `cache` annotation of a hit, and
/// wall-clock fields.
fn without_cache_and_timing(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "cache" && !key.ends_with("elapsed_s"))
                .map(|(key, value)| (key.clone(), without_cache_and_timing(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_cache_and_timing).collect()),
        other => other.clone(),
    }
}

#[test]
fn chain_second_run_over_an_unchanged_project_hits_the_cache() {
    let dir = scratch_dir("cache-hit");
    let cache = scratch_dir("cache-hit-store");
    copy_chain_fixture(&dir);

    assert_eq!(cache_entry_count(&cache), 0);
    let cold = run_cached(&dir, &cache, &["chain", "fsl-project.toml"]);
    assert!(
        cold.status.success(),
        "{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    let after_cold = cache_entry_count(&cache);
    assert!(after_cold >= 2, "cold chain wrote {after_cold} entries");
    let cold = json(&cold);
    assert!(!layer_cache_hit(&cold, "business"));
    assert!(!layer_cache_hit(&cold, "design"));

    let warm = run_cached(&dir, &cache, &["chain", "fsl-project.toml"]);
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stderr)
    );
    assert_eq!(
        cache_entry_count(&cache),
        after_cold,
        "a hit writes nothing"
    );
    let warm = json(&warm);
    assert!(layer_cache_hit(&warm, "business"), "{warm}");
    assert!(layer_cache_hit(&warm, "design"), "{warm}");
    // Same verdict, layer by layer, apart from the additive `cache` field.
    assert_eq!(
        without_cache_and_timing(&cold),
        without_cache_and_timing(&warm)
    );
}

#[test]
fn chain_editing_one_layer_reverifies_only_that_layer() {
    let dir = scratch_dir("cache-edit");
    let cache = scratch_dir("cache-edit-store");
    copy_chain_fixture(&dir);
    let first = run_cached(&dir, &cache, &["chain", "fsl-project.toml"]);
    assert!(first.status.success());

    let design = fs::read_to_string(dir.join("design.fsl")).expect("read design");
    fs::write(dir.join("design.fsl"), format!("{design}\n// edited\n")).expect("edit design");

    let second = json(&run_cached(&dir, &cache, &["chain", "fsl-project.toml"]));
    assert!(layer_cache_hit(&second, "business"), "{second}");
    assert!(
        !layer_cache_hit(&second, "design"),
        "an edited layer must be re-verified, not served from cache: {second}"
    );
}

#[test]
fn chain_with_an_empty_cache_directory_is_cold_again() {
    let dir = scratch_dir("cache-empty");
    let cache = scratch_dir("cache-empty-a");
    let other = scratch_dir("cache-empty-b");
    copy_chain_fixture(&dir);
    assert!(
        run_cached(&dir, &cache, &["chain", "fsl-project.toml"])
            .status
            .success()
    );

    // Control: the hit really comes from the store, not from something that
    // makes every run look warm.
    let cold = json(&run_cached(&dir, &other, &["chain", "fsl-project.toml"]));
    assert!(!layer_cache_hit(&cold, "business"), "{cold}");
    assert!(!layer_cache_hit(&cold, "design"), "{cold}");
    assert!(cache_entry_count(&other) >= 2);
}

/// Issue #1200: `JUnit` reports the `[impl]` layer's `report` key points at.
#[cfg(unix)]
const JUNIT_ALL_SKIPPED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites><testsuite name="pytest" errors="0" failures="0" skipped="2" tests="2">
<testcase classname="test_cart" name="test_scenario_cover_checkout"><skipped type="pytest.skip" message="Adapter not implemented">skipped</skipped></testcase>
<testcase classname="test_cart" name="test_random_walk_conformance"><skipped type="pytest.skip" message="Adapter not implemented">skipped</skipped></testcase>
</testsuite></testsuites>
"#;

#[cfg(unix)]
const JUNIT_PASSED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites><testsuite name="pytest" errors="0" failures="0" skipped="0" tests="2">
<testcase classname="test_cart" name="test_scenario_cover_checkout" time="0.001"/>
<testcase classname="test_cart" name="test_random_walk_conformance" time="0.002"/>
</testsuite></testsuites>
"#;

#[cfg(unix)]
const JUNIT_EMPTY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites><testsuite name="pytest" errors="0" failures="0" skipped="0" tests="0"></testsuite></testsuites>
"#;

#[cfg(unix)]
const JUNIT_FAILED: &str =
    r#"<testsuite tests="1"><testcase name="a"><failure message="boom"/></testcase></testsuite>"#;

/// A manifest with only an `[impl]` layer whose command copies the named
/// `JUnit` fixture to `report.xml` (so the report is written by this run) and
/// then exits with `exit_code`.
#[cfg(unix)]
fn impl_report_project(name: &str, junit: &str, exit_code: i32, report: Option<&str>) -> PathBuf {
    let dir = scratch_dir(name);
    fs::create_dir_all(dir.join("fixtures")).expect("create fixtures dir");
    fs::write(dir.join("fixtures/junit.xml"), junit).expect("write junit fixture");
    let command = format!("cp fixtures/junit.xml report.xml && exit {exit_code}");
    let mut manifest = format!(
        "[impl]\ncommand = {}\n",
        serde_json::to_string(&command).expect("encode command")
    );
    if let Some(report) = report {
        let _ = writeln!(
            manifest,
            "report = {}",
            serde_json::to_string(report).expect("encode report")
        );
    }
    fs::write(dir.join("fsl-project.toml"), manifest).expect("write manifest");
    dir
}

#[cfg(unix)]
#[test]
fn chain_impl_all_skipped_report_is_indeterminate_not_passed() {
    let dir = impl_report_project("impl-all-skipped", JUNIT_ALL_SKIPPED, 0, Some("report.xml"));
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    let table = String::from_utf8_lossy(&output.stderr);
    assert!(
        table.contains("impl  command  failed  indeterminate  no test executed (0/2)"),
        "{table}"
    );
    assert_eq!(output.status.code(), Some(1), "{value:#}");
    assert_eq!(value["result"], "indeterminate", "{value:#}");
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "indeterminate", "{value:#}");
    assert_eq!(impl_layer["status"], "failed", "{value:#}");
    assert_eq!(impl_layer["exit_code"], 1, "{value:#}");
    assert_eq!(
        impl_layer["detail"]["tests"],
        serde_json::json!({"total": 2, "executed": 0, "skipped": 2, "failures": 0, "errors": 0}),
        "{value:#}"
    );
    assert!(
        impl_layer["detail"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.starts_with("no test executed")),
        "{value:#}"
    );
}

#[cfg(unix)]
#[test]
fn chain_impl_report_with_zero_test_cases_is_indeterminate() {
    let dir = impl_report_project("impl-zero-tests", JUNIT_EMPTY, 0, Some("report.xml"));
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(1), "{value:#}");
    assert_eq!(
        layer(&value, "impl")["result"],
        "indeterminate",
        "{value:#}"
    );
    assert_eq!(layer(&value, "impl")["detail"]["tests"]["total"], 0);
}

#[cfg(unix)]
#[test]
fn chain_impl_report_with_executed_tests_passes() {
    let dir = impl_report_project("impl-passed", JUNIT_PASSED, 0, Some("report.xml"));
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(0), "{value:#}");
    assert_eq!(value["result"], "verified", "{value:#}");
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "passed", "{value:#}");
    assert_eq!(impl_layer["detail"]["tests"]["executed"], 2, "{value:#}");
    assert_eq!(impl_layer["detail"]["report"], "report.xml", "{value:#}");
}

#[cfg(unix)]
#[test]
fn chain_impl_report_directory_sums_its_xml_files() {
    let dir = impl_report_project("impl-report-dir", JUNIT_PASSED, 0, Some("results"));
    fs::create_dir_all(dir.join("results")).expect("create results dir");
    fs::write(dir.join("fixtures/skipped.xml"), JUNIT_ALL_SKIPPED).expect("write skipped");
    let command = "cp fixtures/junit.xml results/a.xml && cp fixtures/skipped.xml results/b.xml";
    fs::write(
        dir.join("fsl-project.toml"),
        format!(
            "[impl]\ncommand = {}\nreport = \"results\"\n",
            serde_json::to_string(command).expect("encode command")
        ),
    )
    .expect("write manifest");
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "passed", "{value:#}");
    assert_eq!(
        impl_layer["detail"]["tests"],
        serde_json::json!({"total": 4, "executed": 2, "skipped": 2, "failures": 0, "errors": 0}),
        "{value:#}"
    );
}

#[cfg(unix)]
#[test]
fn chain_impl_stale_or_missing_report_is_indeterminate() {
    // The report predates the command (two seconds old): a leftover from an
    // earlier run is not this run's evidence.
    let dir = impl_report_project("impl-stale", JUNIT_PASSED, 0, Some("report.xml"));
    fs::write(
        dir.join("fsl-project.toml"),
        "[impl]\ncommand = \"true\"\nreport = \"report.xml\"\n",
    )
    .expect("write manifest");
    let report = dir.join("report.xml");
    fs::write(&report, JUNIT_PASSED).expect("write stale report");
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(&report)
        .and_then(|file| file.set_modified(past))
        .expect("age the report");
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "indeterminate", "{value:#}");
    assert!(
        impl_layer["detail"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("unchanged since before the command")),
        "{value:#}"
    );

    fs::remove_file(&report).expect("remove report");
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    assert_eq!(
        layer(&value, "impl")["result"],
        "indeterminate",
        "{value:#}"
    );
}

#[cfg(unix)]
#[test]
fn chain_impl_report_failures_and_nonzero_exit_stay_failed() {
    let dir = impl_report_project("impl-report-failure", JUNIT_FAILED, 0, Some("report.xml"));
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    assert_eq!(value["result"], "violated", "{value:#}");
    assert_eq!(layer(&value, "impl")["result"], "failed", "{value:#}");

    let dir = impl_report_project("impl-exit-1", JUNIT_PASSED, 1, Some("report.xml"));
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    assert_eq!(value["result"], "violated", "{value:#}");
    assert_eq!(layer(&value, "impl")["result"], "failed", "{value:#}");
}

#[cfg(unix)]
#[test]
fn chain_impl_without_report_or_evidence_is_indeterminate() {
    // Issue #1200 (F1): exit 0 alone cannot tell a passing suite from one
    // that executed nothing, so a manifest naming neither a report nor
    // `evidence = "exit_code"` does not pass.
    let dir = impl_report_project("impl-no-report", JUNIT_ALL_SKIPPED, 0, None);
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(1), "{value:#}");
    assert_eq!(value["result"], "indeterminate", "{value:#}");
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "indeterminate", "{value:#}");
    assert_eq!(impl_layer["detail"]["returncode"], 0, "{value:#}");
    assert!(
        impl_layer["detail"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("names no report")),
        "{value:#}"
    );
    let table = String::from_utf8_lossy(&output.stderr);
    assert!(
        table.contains("impl  command  failed  indeterminate  evidence=missing"),
        "{table}"
    );

    // A failing command is still `failed`, not `indeterminate`.
    let dir = impl_report_project("impl-no-report-exit-1", JUNIT_ALL_SKIPPED, 1, None);
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    assert_eq!(value["result"], "violated", "{value:#}");
    assert_eq!(layer(&value, "impl")["result"], "failed", "{value:#}");
}

#[cfg(unix)]
#[test]
fn chain_impl_exit_code_evidence_is_explicit_and_warned() {
    let dir = scratch_dir("impl-exit-code-evidence");
    fs::write(
        dir.join("fsl-project.toml"),
        "[impl]\ncommand = \"true\"\nevidence = \"exit_code\"\n",
    )
    .expect("write manifest");
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(0), "{value:#}");
    assert_eq!(value["result"], "verified", "{value:#}");
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "passed", "{value:#}");
    assert_eq!(
        impl_layer["detail"]["evidence"], "exit_code_only",
        "{value:#}"
    );
    assert!(impl_layer["detail"].get("tests").is_none(), "{value:#}");
    assert_eq!(
        value["warnings"][0]["kind"], "impl_exit_code_only",
        "{value:#}"
    );
    assert_eq!(value["warnings"][0]["layer"], "impl", "{value:#}");
    let table = String::from_utf8_lossy(&output.stderr);
    assert!(
        table.contains("impl  command  passed  passed  evidence=exit_code_only"),
        "{table}"
    );
}

#[test]
fn chain_impl_unknown_key_is_a_parse_error_naming_it() {
    let dir = scratch_dir("impl-unknown-key");
    // The command never runs: the typo is rejected with the manifest.
    fs::write(
        dir.join("fsl-project.toml"),
        "[impl]\ncommand = \"unused\"\nreprot = \"s.xml\"\n",
    )
    .expect("write manifest");
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(2), "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "parse", "{value:#}");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown [impl] key(s): [reprot]")),
        "{value:#}"
    );
    assert!(value.get("layers").is_none(), "no layer may run: {value:#}");
}

#[test]
fn chain_impl_unknown_key_is_reported_even_when_an_earlier_layer_would_fail() {
    // Issue #1200: the `[impl]` table is validated with the manifest, before
    // any layer runs, so a failing `[business]` layer (which stops the chain
    // without --keep-going) cannot hide the typo.
    let dir = scratch_dir("impl-unknown-key-after-failure");
    copy_chain_fixture(&dir);
    let manifest = fs::read_to_string(dir.join("fsl-project-broken.toml"))
        .expect("read broken manifest")
        .replace("evidence = \"exit_code\"\n", "reprot = \"s.xml\"\n");
    assert!(manifest.contains("reprot"), "{manifest}");
    fs::write(dir.join("fsl-project-typo.toml"), manifest).expect("write typo manifest");
    // Control: the same broken manifest without the typo fails at a layer.
    let control = json(&run(&dir, &["chain", "fsl-project-broken.toml"]));
    assert_eq!(control["result"], "violated", "{control:#}");

    let output = run(&dir, &["chain", "fsl-project-typo.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(2), "{value:#}");
    assert_eq!(value["kind"], "parse", "{value:#}");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("[reprot]")),
        "{value:#}"
    );
}

#[cfg(unix)]
#[test]
fn chain_impl_report_written_before_the_command_in_the_same_second_is_not_fresh() {
    // Issue #1200 (F2): a passing report that exists before the command runs
    // is not this run's evidence, even when it was written within the same
    // second (no whole-second truncation).
    let dir = scratch_dir("impl-same-second");
    fs::write(
        dir.join("fsl-project.toml"),
        "[impl]\ncommand = \"true\"\nreport = \"report.xml\"\n",
    )
    .expect("write manifest");
    fs::write(dir.join("report.xml"), JUNIT_PASSED).expect("write report");
    let value = json(&run(&dir, &["chain", "fsl-project.toml"]));
    let impl_layer = layer(&value, "impl");
    assert_eq!(impl_layer["result"], "indeterminate", "{value:#}");
    assert!(
        impl_layer["detail"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("unchanged since before the command")),
        "{value:#}"
    );
}

#[test]
fn chain_impl_empty_report_key_is_a_parse_error() {
    let dir = scratch_dir("impl-empty-report");
    fs::write(
        dir.join("fsl-project.toml"),
        // The command never runs: an empty `report` is rejected first.
        "[impl]\ncommand = \"unused\"\nreport = \"\"\n",
    )
    .expect("write manifest");
    let output = run(&dir, &["chain", "fsl-project.toml"]);
    let value = json(&output);
    assert_eq!(output.status.code(), Some(2), "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "parse", "{value:#}");
}
