// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for #641: `fslc domain check` must preserve the
//! nested kernel's vacuity warnings and action coverage because they qualify
//! confidence in the generated kernel verdict. In particular, an unreachable
//! generated action can signal a domain-lowering defect even when the bounded
//! kernel result is otherwise `verified`.

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

const WARNING: &str = "rust/fslc/tests/fixtures/issue_641_domain_unreachable_decide.fsl";
const CLEAN: &str = "rust/fslc/tests/fixtures/issue_641_domain_clean.fsl";

/// The issue #641 contract: a verified generated kernel can still carry a
/// vacuity warning that qualifies the verdict, so `domain check` must keep
/// the non-empty warning channel in its nested stable projection.
#[test]
fn domain_check_preserves_kernel_warnings() {
    let (check, status) = run(&["domain", "check", WARNING, "--depth", "4"]);
    assert_eq!(status, 0, "{check:#}");
    assert!(
        check["kernel"]["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty()),
        "{check:#}"
    );
}

/// Negative control: a domain whose decisions are all covered and whose
/// state cannot deadlock must remain warning-free, so projecting the channel
/// does not manufacture diagnostics or alter the successful domain verdict.
#[test]
fn domain_check_does_not_overfire_kernel_warnings() {
    let (check, status) = run(&["domain", "check", CLEAN, "--depth", "4"]);
    assert_eq!(status, 0, "{check:#}");
    assert_eq!(check["result"], "verified_under_assumptions");
    let warnings = &check["kernel"]["warnings"];
    assert!(
        warnings.is_null() || warnings.as_array().is_some_and(std::vec::Vec::is_empty),
        "{check:#}"
    );
}

/// Action coverage is the structured companion to a never-enabled warning:
/// domain users need the generated action's coverage object to distinguish
/// a lowering-quality signal from a bare bounded verdict.
#[test]
fn domain_check_preserves_kernel_action_coverage() {
    let (check, status) = run(&["domain", "check", WARNING, "--depth", "4"]);
    assert_eq!(status, 0, "{check:#}");
    assert!(check["kernel"]["action_coverage"].is_object(), "{check:#}");
}
