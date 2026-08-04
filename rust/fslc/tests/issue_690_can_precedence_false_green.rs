// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Semantic negative control for #690 symptom 1: `Context::normalize`
//! (`rust/fsl-core/src/domain.rs`) rendered a `can(Command)` expansion by
//! joining each `requires`/`rejects` piece with literal `" and "` without
//! individually parenthesizing each piece. Because `and` binds tighter
//! than `or` in FSL's grammar, a `decide` with two or more pieces where one
//! piece contains a top-level `or` misgrouped on the rendered/re-parsed
//! path (`domain expand` / `domain testgen` / `domain scaffold` /
//! `check_domain`) while the directly-lowered typed path (`check`/`verify`
//! on the `.fsl` domain source) built the correct grouping directly.
//!
//! `rust/fsl-core/tests/domain_render_agreement.rs` pins that the two
//! paths' *public Kernel contracts* agree (issue #664's structural gate).
//! This file pins the property #690 is actually about: that the two paths
//! reach the *same verifier verdict* on independent `Bool` state where the
//! misgrouping is truth-value-observable, not just AST-shape-observable
//! (`rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl`'s
//! `can(Cancel)` is over a single-valued, mutually exclusive enum, so a
//! misgrouping there cannot flip a verdict -- seeing this land in
//! `rust/fslc/tests/` rather than folding it into `domain_render_agreement.rs`
//! keeps the structural-contract-agreement gate and the
//! verdict/false-green gate as two independently reviewable claims, per
//! `rust/fsl-core/tests/domain_render_agreement.rs`'s own precedent of not
//! calling `fslc verify` at all).
//!
//! Before the fix: path A (`verify` on the domain source) reported
//! `violated`, path B (`verify` on `domain expand`'s rendered output)
//! reported `verified` -- a false green, the class AGENTS.md ranks above a
//! crash. The negative control (revert the one-line fix in
//! `rust/fsl-core/src/domain.rs`) reproduces exactly that divergence; see
//! the task report, not this file, for the reverted-fix transcript, since
//! this file must keep pinning the *fixed* behavior on `main`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE: &str =
    "rust/fslc/tests/fixtures/domain_characterization/can_expansion_precedence.fsl";

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

fn expanded_source() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["domain", "expand", FIXTURE])
        .current_dir(root())
        .output()
        .expect("run native fslc domain expand");
    assert!(
        output.status.success(),
        "domain expand failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 rendered kernel source")
}

fn tempfile_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-issue-690-can-precedence-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch dir");
    directory
}

/// The rendered `can(Open)` expansion must parenthesize each
/// `requires`/`rejects` piece individually, not just wrap the whole joined
/// list once. This is the direct textual assertion for the fix (each piece
/// is now atomic wherever the substitution lands), independent of whether
/// re-parsing happens to agree on this particular fixture.
#[test]
fn rendered_can_expansion_parenthesizes_each_piece() {
    let rendered = expanded_source();
    let invariant_line = rendered
        .lines()
        .find(|line| line.contains("aImpliesCanOpen"))
        .unwrap_or_else(|| panic!("no aImpliesCanOpen invariant line in:\n{rendered}"));
    assert!(
        invariant_line.contains("((gate_a or gate_b) and (gate_c))"),
        "expected each can(Open) piece individually parenthesized, got: {invariant_line}"
    );
}

/// The headline false-green control: `verify` on the domain source
/// (path A, the typed model `lower_domain` builds) and `verify` on
/// `domain expand`'s rendered-then-reparsed output (path B) must report
/// the same verdict on `aImpliesCanOpen`. Before #690's fix, path A
/// reported `violated` and path B reported `verified` for this exact
/// fixture -- the false green this test exists to prevent from coming
/// back.
#[test]
fn verify_agrees_between_typed_model_and_rendered_kernel() {
    let (verify_a, status_a) = run(&["verify", FIXTURE, "--depth", "6", "--no-cache"]);
    assert_eq!(status_a, 1, "path A: {verify_a:#}");
    assert_eq!(verify_a["result"], "violated", "path A: {verify_a:#}");
    assert_eq!(
        verify_a["invariant"], "aImpliesCanOpen",
        "path A: {verify_a:#}"
    );

    let directory = tempfile_dir();
    let expanded_path = directory.join("can_expansion_precedence.expanded.fsl");
    std::fs::write(&expanded_path, expanded_source()).expect("write expanded kernel source");

    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            expanded_path.to_str().expect("utf8 path"),
            "--depth",
            "6",
            "--no-cache",
        ])
        .output()
        .expect("run native fslc verify on expanded kernel");
    let verify_b: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status_b = output.status.code().expect("exit status");

    assert_eq!(status_b, 1, "path B: {verify_b:#}");
    assert_eq!(verify_b["result"], "violated", "path B: {verify_b:#}");
    assert_eq!(
        status_a, status_b,
        "path A/B exit codes disagree: {verify_a:#} vs {verify_b:#}"
    );
    assert_eq!(
        verify_a["result"], verify_b["result"],
        "path A/B verdicts disagree -- the #690 false green is back: {verify_a:#} vs {verify_b:#}"
    );

    std::fs::remove_file(&expanded_path).ok();
}
