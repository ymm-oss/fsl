// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #496: `analyze` batch mode must never silently
//! drop an explicitly-named input just because it does not end in `.fsl`.
//! `collect_analysis_files`'s `.fsl`-only filter is meant for *directory*
//! expansion only; before the fix it also applied to files named directly
//! on the command line, so `analyze a.toml b.txt` returned `files:[]`,
//! `errors:[]`, `result:"analyzed"`, exit 0 — a batch that "analyzed"
//! nothing at all reported success.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
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

const UNSUPPORTED_A: &str = "rust/fslc/tests/fixtures/analyze_batch_unsupported_a.txt";
const UNSUPPORTED_B: &str = "rust/fslc/tests/fixtures/analyze_batch_unsupported_b.txt";

/// Two explicit non-`.fsl` inputs, neither a project manifest: both must
/// survive into `files[]`/`errors[]` with their own diagnostic, and the
/// batch must report `result:"error"`/exit 2 — never `analyzed`/exit 0 for
/// a run that analyzed nothing.
#[test]
fn batch_preserves_explicit_unsupported_files_instead_of_silently_dropping_them() {
    let (output, status) = run(&["analyze", UNSUPPORTED_A, UNSUPPORTED_B]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["mode"], "batch");

    let files = output["files"].as_array().expect("files array");
    assert_eq!(
        files.len(),
        2,
        "both explicit inputs must appear: {output:#}"
    );
    let errors = output["errors"].as_array().expect("errors array");
    assert_eq!(
        errors.len(),
        2,
        "both explicit inputs must error: {output:#}"
    );

    let file_names = files
        .iter()
        .map(|entry| entry["file"].as_str().expect("file name"))
        .collect::<Vec<_>>();
    assert!(
        file_names
            .iter()
            .any(|name| name.ends_with("analyze_batch_unsupported_a.txt"))
    );
    assert!(
        file_names
            .iter()
            .any(|name| name.ends_with("analyze_batch_unsupported_b.txt"))
    );
    for entry in files {
        assert_eq!(entry["result"], "error", "{entry:#}");
        assert!(entry["message"].is_string(), "{entry:#}");
    }
}

/// Regression control: an explicit `.toml` project manifest is a
/// first-class supported batch input (routed through the same manifest
/// handling `analyze <manifest>.toml --projection traceability_graph`
/// already uses in single-file mode) — it must not be swept up as
/// "unsupported" alongside the `.txt` fix above.
#[test]
fn batch_still_routes_an_explicit_project_manifest_through_traceability_analysis() {
    let (output, status) = run(&[
        "analyze",
        "tests/fixtures/chain/fsl-project.toml",
        "specs/cart_v1.fsl",
        "--projection",
        "traceability_graph",
    ]);
    assert_eq!(output["mode"], "batch", "{output:#}");
    let files = output["files"].as_array().expect("files array");
    let manifest_entry = files
        .iter()
        .find(|entry| {
            entry["file"]
                .as_str()
                .is_some_and(|name| name.ends_with("fsl-project.toml"))
        })
        .unwrap_or_else(|| panic!("manifest entry missing: {output:#}"));
    assert_eq!(
        manifest_entry["result"], "analyzed",
        "project manifest must still analyze successfully: {output:#}"
    );
    // `specs/cart_v1.fsl` does not accept traceability_graph, so it is
    // expected to error in this run — the point is the manifest itself
    // was not dropped or misrouted, and exit reflects the real failure.
    assert_eq!(status, 2, "{output:#}");
}

/// Regression control: directory expansion keeps filtering to `.fsl` only
/// (a non-`.fsl` file sitting in a scanned directory is not something the
/// user named explicitly, so it must stay excluded).
#[test]
fn batch_over_a_directory_still_filters_to_fsl_only() {
    let (output, status) = run(&["analyze", "specs/"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "analyzed");
    let files = output["files"].as_array().expect("files array");
    assert!(!files.is_empty(), "{output:#}");
    for entry in files {
        assert!(
            entry["file"].as_str().is_some_and(|name| Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("fsl"))),
            "{entry:#}"
        );
    }
}
