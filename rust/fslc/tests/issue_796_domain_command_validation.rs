// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Cross-command validation controls for #796.
//!
//! `domain analyze` projects the domain AST and `domain expand` renders
//! generated Kernel text, but neither may accept a source document that typed
//! domain lowering rejects. In particular, renderer string normalization can
//! leave an unknown authored identifier unchanged and previously produced a
//! Kernel source that `fslc check` rejected. Both commands must instead return
//! the same located semantic diagnostic as `check`.

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

fn run_text(args: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit status"),
    )
}

fn diagnostic_reason(output: &Value) -> &str {
    output["message"]
        .as_str()
        .expect("semantic diagnostic message")
        .split_once(" at ")
        .map_or_else(
            || {
                output["message"]
                    .as_str()
                    .expect("semantic diagnostic message")
            },
            |(reason, _)| reason,
        )
}

fn assert_rejected_like_check(fixture: &str) {
    let (check, check_status) = run(&["check", fixture]);
    assert_eq!(check_status, 2, "check: {check:#}");
    assert_eq!(check["kind"], "semantics", "check: {check:#}");
    assert!(check.get("loc").is_some(), "check: {check:#}");
    let reason = diagnostic_reason(&check);

    for command in [["domain", "analyze"], ["domain", "expand"]] {
        let (actual, status) = run(&[command[0], command[1], fixture]);
        assert_eq!(status, 2, "{command:?}: {actual:#}");
        assert_eq!(actual["result"], "error", "{command:?}: {actual:#}");
        assert_eq!(actual["kind"], "semantics", "{command:?}: {actual:#}");
        assert_eq!(
            diagnostic_reason(&actual),
            reason,
            "{command:?} must share check's semantic diagnostic reason: actual={actual:#}, check={check:#}"
        );
        assert_eq!(
            actual["loc"], check["loc"],
            "{command:?} must preserve check's source location: actual={actual:#}, check={check:#}"
        );
    }
}

/// Rejecting control: before #796 both commands returned exit 0 for this
/// unknown state reference, while `check` rejected it at the authored
/// expression. Keeping all three envelopes aligned prevents either command
/// from again producing a false-green analysis or an invalid Kernel export.
#[test]
fn domain_analyze_and_expand_reject_unknown_names_like_check() {
    assert_rejected_like_check(
        "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl",
    );
}

/// `domain expand --output` must validate before writing: an invalid document
/// cannot create a partial Kernel file or overwrite an existing one.
#[test]
fn domain_expand_rejection_does_not_write_output() {
    let fixture = "rust/fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl";
    let output_path = std::env::temp_dir().join(format!(
        "fslc-issue-796-{}-expand-output.fsl",
        std::process::id()
    ));
    std::fs::write(&output_path, "existing output").expect("write sentinel");
    let output = output_path.to_string_lossy().into_owned();

    let (actual, status) = run(&["domain", "expand", fixture, "--output", &output]);

    assert_eq!(status, 2, "{actual:#}");
    assert_eq!(actual["result"], "error", "{actual:#}");
    assert_eq!(actual["kind"], "semantics", "{actual:#}");
    assert_eq!(
        std::fs::read_to_string(&output_path).expect("read sentinel"),
        "existing output",
        "domain expand overwrote output after validation failed: {actual:#}"
    );
    std::fs::remove_file(&output_path).expect("remove sentinel");
}

/// #798 is the sibling generated-name case: direct lowering rejects authored
/// use of a generated enum member, but the renderer formerly left that already
/// qualified text in place, making a falsely valid Kernel. The shared lowering
/// validation rejects it before either command emits a success envelope.
#[test]
fn domain_analyze_and_expand_reject_generated_kernel_names_like_check() {
    assert_rejected_like_check(
        "rust/fslc/tests/fixtures/domain_characterization/ai_internal_name_misuse.fsl",
    );
}

/// Accepting control: valid documents retain both the structural analysis and
/// inspectable generated Kernel source paths after validation is introduced.
#[test]
fn domain_analyze_and_expand_still_accept_valid_domain_specs() {
    let fixture = "examples/domain/order_fulfillment_saga.fsl";

    let (analyze, analyze_status) = run(&["domain", "analyze", fixture]);
    assert_eq!(analyze_status, 0, "{analyze:#}");
    assert_eq!(analyze["result"], "analyzed");

    let (expand, expand_status) = run_text(&["domain", "expand", fixture]);
    assert_eq!(expand_status, 0, "{expand}");
    assert!(
        expand.starts_with("spec "),
        "domain expand must keep emitting raw Kernel source: {expand}"
    );
}
