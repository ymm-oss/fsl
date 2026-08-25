// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! CLI coverage for the deterministic structural-analysis DOT/Mermaid exporter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CART: &str = "specs/cart_v1.fsl";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc")
}

fn scratch_file(name: &str, source: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-analyze-export-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write analysis fixture");
    path
}

#[test]
fn dot_and_mermaid_exports_are_byte_deterministic() {
    for (format, header) in [
        ("dot", b"digraph".as_slice()),
        ("mermaid", b"graph TD".as_slice()),
    ] {
        let arguments = ["analyze", CART, "--projection", "tsg", "--format", format];
        let first = run(&arguments);
        let second = run(&arguments);

        assert_eq!(first.status.code(), Some(0), "{format}");
        assert_eq!(second.status.code(), Some(0), "{format}");
        assert!(first.stderr.is_empty(), "{format}");
        assert!(second.stderr.is_empty(), "{format}");
        assert_eq!(first.stdout, second.stdout, "{format}");
        assert!(first.stdout.starts_with(header), "{format}");
    }
}

#[test]
fn dot_export_escapes_a_backslash_from_a_valid_fsl_requirement_id() {
    let fixture = scratch_file(
        "escaped-requirement.fsl",
        r#"spec EscapedRequirement {
  state { ready: Bool }
  init { ready = true }

  invariant Ready "REQ\PATH: ready remains true" { ready }
}
"#,
    );
    let output = run(&[
        "analyze",
        fixture.to_str().expect("UTF-8 fixture path"),
        "--projection",
        "tsg",
        "--format",
        "dot",
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let dot = String::from_utf8(output.stdout).expect("UTF-8 DOT output");
    assert!(dot.starts_with("digraph fsl_analysis {\n"));
    let requirement = dot
        .lines()
        .find(|line| line.contains("shape=\"box\""))
        .expect("requirement node");
    assert_eq!(
        requirement,
        "  \"requirement:REQ\\\\PATH\" [label=\"REQ\\\\PATH\", shape=\"box\"];"
    );
}

#[test]
fn non_graph_profile_rejects_dot_and_mermaid_exports() {
    for format in ["dot", "mermaid"] {
        let output = run(&[
            "analyze",
            CART,
            "--profile",
            "ai-review",
            "--format",
            format,
        ]);

        assert_eq!(output.status.code(), Some(2), "{format}");
        assert!(output.stderr.is_empty(), "{format}");
        let error: Value = serde_json::from_slice(&output.stdout).expect("error JSON");
        assert_eq!(error["result"], "error", "{format}");
        assert_eq!(error["kind"], "semantics", "{format}");
        assert_eq!(
            error["message"], "DOT/Mermaid export is supported for graph projections, not profiles",
            "{format}"
        );
    }
}
