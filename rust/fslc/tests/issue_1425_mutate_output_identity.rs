// SPDX-License-Identifier: Apache-2.0

//! Preservation controls for #1425, captured from the named pre-fix main
//! binary (see `fixtures/issue_1425/README.md`). Compare all stdout bytes: no
//! field filtering, path rewriting, JSON reserialization, or timing exclusions.

use std::path::Path;
use std::process::Command;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn assert_identity(name: &str, spec: &str, depth: &str) {
    for cap in ["default", "0", "1", "2"] {
        let expected = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/issue_1425")
                .join(format!("{name}.{cap}.stdout.json")),
        )
        .expect("read pre-fix stdout fixture");
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command
            .args(["mutate", spec, "--depth", depth])
            .current_dir(workspace_root())
            .env("FSLC_CACHE", "off");
        if cap != "default" {
            command.args(["--max-mutants", cap]);
        }
        let output = command.output().expect("run native mutate");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name} cap={cap}: stderr={}",
            String::from_utf8_lossy(&output.stderr),
        );
        // Ensure a baseline-only refusal cannot masquerade as mutation data.
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("mutate JSON");
        assert_eq!(value["result"], "mutated", "{name} cap={cap}: {value}");
        assert_eq!(
            output.stdout, expected,
            "{name} cap={cap}: complete stdout must match the pre-fix binary",
        );
    }
}

#[test]
fn issue_524_cardinality() {
    assert_identity(
        "issue_524",
        "rust/fslc/tests/fixtures/issue_524_mutant_cardinality.fsl",
        "2",
    );
}

#[test]
fn issue_848_bank() {
    assert_identity(
        "issue_848",
        "rust/fslc/tests/fixtures/issue_848_bank_healthy.fsl",
        "8",
    );
}

#[test]
fn bounded_counter() {
    assert_identity(
        "bounded_counter",
        "examples/gallery/valid/tiny_bounded_counter.fsl",
        "4",
    );
}

#[test]
fn traffic_light() {
    assert_identity(
        "traffic_light",
        "examples/gallery/valid/tiny_traffic_light.fsl",
        "4",
    );
}

#[test]
fn turnstile() {
    assert_identity(
        "turnstile",
        "examples/gallery/valid/tiny_turnstile.fsl",
        "4",
    );
}

#[test]
fn option_structural_equality() {
    assert_identity(
        "option_equality",
        "examples/gallery/valid/tiny_option_structural_equality.fsl",
        "4",
    );
}

#[test]
fn forbidden_guarded_cancel() {
    assert_identity(
        "guarded_cancel",
        "examples/gallery/valid/small_forbidden_guarded_cancel.fsl",
        "4",
    );
}
