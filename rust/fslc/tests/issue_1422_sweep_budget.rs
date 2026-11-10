// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

fn fixture(source: &str) -> PathBuf {
    let root = scratch_identity::unique(
        std::env::temp_dir().join(format!("fsl-1422-sweep-{}", std::process::id())),
    );
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("subject.fsl"), source).unwrap();
    root
}

fn error(kind: &str, message: &str) -> Value {
    json!({"fsl":"1.0", "result":"error", "kind":kind, "message":message})
}

fn expect_packet(root: &Path, file: &str, options: &[&str], expected: &Value) {
    let mut previous = None;
    for repeat in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .current_dir(root)
            .args(["sweep", file])
            .args(options)
            .env("FSLC_CACHE", "off")
            .output()
            .unwrap();
        let raw = (output.stdout, output.stderr, output.status.code());
        eprintln!(
            "ISSUE_1422_SWEEP_OBSERVATION {}",
            json!({"file":file,
            "options":options, "repeat":repeat, "stdout":raw.0, "stderr":raw.1, "exit":raw.2})
        );
        let packet: Value = serde_json::from_slice(&raw.0).unwrap();
        assert_eq!(
            (&packet, raw.1.as_slice(), raw.2),
            (expected, &[][..], Some(2)),
            "{options:?}"
        );
        if let Some(previous) = &previous {
            assert_eq!(&raw, previous);
        }
        previous = Some(raw);
    }
}

const SOURCE: &str = include_str!("fixtures/sweep_clean.fsl");
const WIDE: &[&str] = &["--instances", "Missing=1..10001", "--depth", "0..0"];

fn budget_case(options: &[&str]) {
    let root = fixture(SOURCE);
    let expected = error("semantics", "sweep grid exceeds the maximum of 10000 cells");
    expect_packet(&root, "subject.fsl", options, &expected);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn instance_width_10001_precedes_the_known_first_cell_scope_refusal() {
    budget_case(WIDE);
}

#[test]
fn value_width_10001_precedes_the_known_first_cell_scope_refusal() {
    budget_case(&["--values", "Missing=0..10000", "--depth", "0..0"]);
}

#[test]
fn inclusive_depth_10001_precedes_the_known_first_cell_scope_refusal() {
    budget_case(&["--instances", "Missing=1..1", "--depth", "0..10000"]);
}

#[test]
fn composed_10200_cells_precede_the_known_first_cell_scope_refusal() {
    budget_case(&[
        "--instances",
        "Missing=1..100",
        "--values",
        "Other=0..50",
        "--depth",
        "0..1",
    ]);
}

#[test]
fn inclusive_boundary_and_normalized_duplicates_keep_the_old_scope_packet() {
    let root = fixture(SOURCE);
    let expected = error(
        "semantics",
        "--instances/--values only apply to specs with entity/number declarations; this spec declares neither",
    );
    for options in [
        &["--instances", "Missing=1..1", "--depth", "0..0"][..],
        &["--instances", "Missing=1..10000", "--depth", "0..0"],
        &[
            "--instances",
            "Missing=1..100",
            "--values",
            "Other=0..49",
            "--depth",
            "0..1",
        ],
        &[
            "--instances",
            " Missing =1..10001",
            "--instances",
            "Missing=1..1",
            "--depth",
            "0..0",
        ],
        &[
            "--values",
            " Missing =0..10000",
            "--values",
            "Missing=0..0",
            "--depth",
            "0..0",
        ],
    ] {
        expect_packet(&root, "subject.fsl", options, &expected);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_option_range_file_literate_and_edition_gates_keep_precedence() {
    let root = fixture(SOURCE);
    let mut unknown = WIDE.to_vec();
    unknown.push("--unknown");
    expect_packet(
        &root,
        "subject.fsl",
        &unknown,
        &error("usage", "unknown sweep option '--unknown'"),
    );
    expect_packet(
        &root,
        "subject.fsl",
        &["--instances", "Missing=1..10001", "--depth", "bad"],
        &error("semantics", "invalid --depth value 'bad': expected LO..HI"),
    );
    expect_packet(
        &root,
        "subject.fsl",
        &["--instances", "Missing=0..10000", "--depth", "0..0"],
        &error(
            "semantics",
            "invalid --instances value 'Missing=0..10000': instance lower bound must be >= 1",
        ),
    );
    let missing = std::fs::read_to_string(root.join("missing.fsl")).unwrap_err();
    expect_packet(
        &root,
        "missing.fsl",
        WIDE,
        &error("io", &format!("missing.fsl: {missing}")),
    );
    expect_packet(
        &root,
        "missing.md",
        WIDE,
        &json!({"fsl":"1.0", "result":"error", "kind":"usage",
        "message":"fslc sweep does not support literate Markdown input; only check, verify, scenarios extract ```fsl fenced code from a .md file -- pass 'missing.md' to one of those, or extract the fenced code into a .fsl file first",
        "diagnostic_code":"FSL-INPUT-LITERATE-UNSUPPORTED", "loc":{"file":"missing.md"}}),
    );
    std::fs::write(root.join("future.fsl"), format!("fsl 6\n{SOURCE}")).unwrap();
    expect_packet(
        &root,
        "future.fsl",
        WIDE,
        &json!({"fsl":"1.0", "result":"error", "kind":"parse",
        "message":"unsupported syntax edition 'fsl 6'; supported editions: 4, 5 at 1:1",
        "diagnostic_code":"unsupported_edition", "loc":{"line":1, "column":1}}),
    );
    std::fs::remove_dir_all(root).unwrap();
}
