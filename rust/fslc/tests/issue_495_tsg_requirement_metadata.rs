// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #495: the native TSG projection must emit the
//! documented `requirement`/`acceptance`/`forbidden`/`kpi` node kinds and
//! `covers` edges (`docs/DESIGN-analysis.md` §2), and — the actual
//! deliverable — a requirement-family review finding must be *detected*,
//! not merely representable. Before the fix, `build_tsg` never projected
//! any of these kinds for a standalone `.fsl`/requirements spec (only the
//! separate `.toml` project-manifest path had an equivalent, and even that
//! path never saw acceptance/forbidden scenarios), so
//! `requirement_property_graph` always had zero edges,
//! `--focus requirement:ID` always failed, and `disconnected_requirement`
//! could never fire — a detector that exists but detects nothing.

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

/// The actual deliverable: a requirement attached only to `init` (which has
/// no TSG node) must surface as a `disconnected_requirement` finding under
/// `--profile ai-review` — not merely a node kind that exists but is never
/// exercised by a detector.
#[test]
fn disconnected_requirement_actually_fires_for_an_orphaned_requirement() {
    let (output, status) = run(&[
        "analyze",
        "rust/fslc/tests/fixtures/analyze_orphan_requirement.fsl",
        "--profile",
        "ai-review",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    let finding = findings
        .iter()
        .find(|finding| finding["finding_type"] == "disconnected_requirement")
        .unwrap_or_else(|| panic!("disconnected_requirement did not fire: {output:#}"));
    assert_eq!(
        finding["involved_nodes"],
        serde_json::json!(["requirement:REQ-INIT-ONLY"])
    );
}

/// Regression control: a requirement that *is* connected (covers a real
/// action/property) must not be flagged, so the fix does not over-trigger.
#[test]
fn disconnected_requirement_does_not_fire_for_a_connected_requirement() {
    let (output, status) = run(&[
        "analyze",
        "examples/e2e/2_requirements.fsl",
        "--profile",
        "ai-review",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    assert!(
        !findings
            .iter()
            .any(|finding| finding["finding_type"] == "disconnected_requirement"),
        "{output:#}"
    );
}

/// `requirement_property_graph` must actually connect requirements to their
/// covered actions — before the fix this projection always had zero edges
/// for a standalone spec.
#[test]
fn requirement_property_graph_has_real_covers_edges() {
    let (output, status) = run(&[
        "analyze",
        "examples/e2e/2_requirements.fsl",
        "--projection",
        "requirement_property_graph",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let edges = output["edges"].as_array().expect("edges array");
    assert!(!edges.is_empty(), "{output:#}");
    assert!(
        edges.iter().any(|edge| edge["kind"] == "covers"),
        "{output:#}"
    );
}

/// `--focus requirement:ID` must resolve — before the fix no `requirement:*`
/// node existed for a standalone spec, so every such focus failed as an
/// unknown node (`kind:"name"`).
#[test]
fn focus_on_a_requirement_node_resolves() {
    let (output, status) = run(&[
        "analyze",
        "examples/e2e/2_requirements.fsl",
        "--projection",
        "impact_graph",
        "--focus",
        "requirement:REQ-1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_ne!(output["kind"], "name");
}

/// `acceptance`/`forbidden` scenario nodes must appear in the base TSG, and
/// an `@requirement(...)`-annotated scenario must get a `covers` edge from
/// the requirement that covers it.
#[test]
fn acceptance_and_forbidden_nodes_appear_and_are_covered_by_requirements() {
    let (output, status) = run(&[
        "analyze",
        "examples/annotations/annotated_claims.fsl",
        "--projection",
        "tsg",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let nodes = output["nodes"].as_array().expect("nodes array");
    assert!(
        nodes.iter().any(|node| node["id"] == "acceptance:AC-1"),
        "{output:#}"
    );
    assert!(
        nodes.iter().any(|node| node["id"] == "forbidden:NEG-1"),
        "{output:#}"
    );
    let edges = output["edges"].as_array().expect("edges array");
    assert!(
        edges.iter().any(|edge| {
            edge["kind"] == "covers"
                && edge["from"] == "requirement:REQ-ACCEPT"
                && edge["to"] == "acceptance:AC-1"
        }),
        "{output:#}"
    );
    // Well-formedness: `requirement:REQ-ACCEPT` is cited only via a
    // `@requirement(...)` annotation on the acceptance case itself, never
    // as a `model.requirement_targets()` Kernel target — the enrichment
    // must create that node too, not just the edge pointing at it.
    let node_ids = nodes
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for edge in edges {
        assert!(
            node_ids.contains(edge["from"].as_str().unwrap_or_default()),
            "dangling edge.from: {edge:#}"
        );
        assert!(
            node_ids.contains(edge["to"].as_str().unwrap_or_default()),
            "dangling edge.to: {edge:#}"
        );
    }
}

/// KPI projection nodes (`kpi NAME = count ENTITY in STAGE`) must appear in
/// the TSG.
#[test]
fn kpi_nodes_appear_in_the_tsg() {
    let (output, status) = run(&[
        "analyze",
        "examples/e2e/2_requirements.fsl",
        "--projection",
        "tsg",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let nodes = output["nodes"].as_array().expect("nodes array");
    let kpi = nodes
        .iter()
        .find(|node| node["id"] == "kpi:paid_claims")
        .unwrap_or_else(|| panic!("kpi node missing: {output:#}"));
    assert_eq!(kpi["kind"], "kpi");
}
