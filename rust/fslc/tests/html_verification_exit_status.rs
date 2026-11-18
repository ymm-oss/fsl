// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Fail-closed exit contract for `fslc html` (#1009).
//!
//! `html` runs bounded verification and embeds the folded envelope in the
//! report, but before #1009 discarded the verification status and always
//! returned exit 0 from `generated_content_result`. The fix mirrors `ledger`:
//! the report is still rendered, and the process exit follows
//! `outcome::exit_status` on the verification envelope.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::path::{Path, PathBuf};

use serde_json::Value;

/// `sweep_violating.fsl` is the repository's smallest violated BMC baseline
/// (`sweep_contract`, issue #885). `verify` exits 1 with `result:"violated"`.
const VIOLATED_FIXTURE: &str = "rust/fslc/tests/fixtures/sweep_violating.fsl";
/// `impl_iv.fsl` is the inline-`implements` detector fixture from
/// `inline_implements_fail_closed` (#1002/#1026): primary invariant violated
/// and the seam folds to `impl_violated`.
const INLINE_IMPLEMENTS_FIXTURE: &str =
    "rust/fslc/tests/fixtures/inline_implements_fail_closed/impl_iv.fsl";
const HEALTHY_FIXTURE: &str = "rust/fslc/tests/fixtures/sweep_clean.fsl";
const PARSE_FIXTURE: &str = "examples/gallery/errors/parse_missing_expression.fsl";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn scratch_dir(label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "html-exit-1009-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

fn run_html_to_file(spec: &str, output_path: &Path) -> (Value, i32) {
    let output = fslc_cli::command()
        .args([
            "html",
            spec,
            "--depth",
            "3",
            "-o",
            output_path.to_str().expect("UTF-8 output path"),
        ])
        .current_dir(workspace_root())
        .output()
        .expect("run native fslc html");
    let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (envelope, output.status.code().expect("native exit status"))
}

/// detector (#1009): a violated baseline must not exit 0 while still rendering
/// the report artifact.
#[test]
fn html_exits_one_on_a_violated_baseline_and_still_generates_the_report() {
    let output_path = scratch_dir("violated").join("report.html");
    let (envelope, status) = run_html_to_file(VIOLATED_FIXTURE, &output_path);
    assert_eq!(status, 1, "{envelope}");
    assert_eq!(envelope["result"], "generated", "{envelope}");
    let html = std::fs::read_to_string(&output_path).expect("read HTML report");
    assert!(
        !html.trim().is_empty(),
        "report must still be generated on failure"
    );
    assert!(
        html.contains("<!doctype html>"),
        "expected a self-contained HTML report"
    );
}

/// detector (#1002/#1026 class): inline `implements` failure must fold to exit 1.
#[test]
fn html_exits_one_when_inline_implements_fails() {
    let output_path = scratch_dir("implements").join("report.html");
    let (envelope, status) = run_html_to_file(INLINE_IMPLEMENTS_FIXTURE, &output_path);
    assert_eq!(status, 1, "{envelope}");
    assert_eq!(envelope["result"], "generated", "{envelope}");
}

/// preservation control: a verifying baseline must keep exit 0.
#[test]
fn html_exits_zero_on_a_verifying_baseline() {
    let output_path = scratch_dir("healthy").join("report.html");
    let (envelope, status) = run_html_to_file(HEALTHY_FIXTURE, &output_path);
    assert_eq!(status, 0, "{envelope}");
    assert_eq!(envelope["result"], "generated", "{envelope}");
}

/// preservation control: parse errors keep the ordinary spec-error exit.
#[test]
fn html_parse_errors_keep_the_spec_error_exit() {
    let output = fslc_cli::command()
        .args(["html", PARSE_FIXTURE])
        .current_dir(workspace_root())
        .output()
        .expect("run native fslc html");
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(envelope["result"], "error", "{envelope}");
    assert_eq!(envelope["kind"], "parse", "{envelope}");
    assert_eq!(
        output.status.code(),
        Some(2),
        "parse errors exit 2 per issue #484 matrix: {envelope}"
    );
}

fn embedded_json(html: &str, label: &str) -> Value {
    let marker = format!("<details><summary>{label}</summary><div><div class=\"code-block\"><pre>");
    let escaped = html
        .split_once(&marker)
        .expect("named raw JSON section")
        .1
        .split_once("</pre>")
        .expect("raw JSON closing pre")
        .0;
    let json = escaped
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json).expect("embedded JSON")
}

/// Detector (#1871): HTML witnesses always use warn, while verification follows
/// the request. Following ignore would omit the deadlock witness; following
/// error would stop witness generation on the violated baseline.
fn assert_html_deadlock_witness(mode: &str, status: i32, result: &str) {
    let output = fslc_cli::command()
        .args([
            "html",
            "examples/gallery/errors/violated_deadlock_terminal.fsl",
            "--depth",
            "3",
            "--deadlock",
            mode,
        ])
        .current_dir(workspace_root())
        .output()
        .expect("run native fslc html");
    assert_eq!(output.status.code(), Some(status), "{mode}: {output:?}");
    let html = std::str::from_utf8(&output.stdout).expect("UTF-8 HTML");
    let explained = embedded_json(html, "explain JSON");
    assert!(
        explained["witnesses"]
            .as_array()
            .expect("witnesses")
            .iter()
            .any(|scenario| scenario["name"] == "deadlock_terminal"),
        "{mode} must still build the deadlock_terminal witness: {explained}"
    );
    let verification = embedded_json(html, "verify JSON");
    assert_eq!(verification["result"], result, "{mode}: {verification}");
}

#[test]
fn html_deadlock_ignore_still_builds_warn_witnesses() {
    assert_html_deadlock_witness("ignore", 0, "verified");
}

#[test]
fn html_deadlock_error_still_builds_warn_witnesses() {
    assert_html_deadlock_witness("error", 1, "violated");
}
