// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The TSG vocabulary must not depend on which input form named the spec.
//!
//! `docs/DESIGN-analysis.md` §2 lists the stable node and edge kinds without
//! conditioning them on input form, but the project-manifest path built each
//! layer with `fsl_tools::build_tsg` alone. `build_tsg` sees only the lowered
//! `KernelModel`, and `acceptance`/`forbidden` cases have no lowered form, so
//! the same source yielded a different vocabulary depending on whether it was
//! analyzed standalone or through a `.toml` manifest (#558).
//!
//! The check below is a *property over kinds*, not a golden of one graph: it
//! projects the manifest graph back down per layer and compares kind sets with
//! the standalone graph of that layer's own file. A golden would freeze one
//! shape; this keeps detecting divergence as either path grows new kinds.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const MANIFEST: &str = "rust/fslc/tests/fixtures/manifest_vocabulary/fsl-project.toml";
const LAYERS: [(&str, &str); 2] = [
    (
        "business",
        "rust/fslc/tests/fixtures/manifest_vocabulary/business.fsl",
    ),
    (
        "requirements",
        "rust/fslc/tests/fixtures/manifest_vocabulary/requirements.fsl",
    ),
];

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

/// The manifest graph renames a layer's `spec` node by layer role; that is a
/// deliberate manifest-only distinction, not a vocabulary difference.
fn normalized_kind(kind: &str) -> &str {
    match kind {
        "business_spec" | "requirements_spec" | "design_spec" => "spec",
        other => other,
    }
}

fn node_kinds(graph: &Value) -> BTreeSet<String> {
    graph["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|node| node["kind"].as_str())
        .map(|kind| normalized_kind(kind).to_owned())
        .collect()
}

fn edge_kinds(graph: &Value) -> BTreeSet<String> {
    graph["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter_map(|edge| edge["kind"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Project the manifest graph back down to one layer. A layer-local node id is
/// `<layer>:<id>` and a layer-local edge id is `edge:<layer>:<original edge
/// id>`, which always starts `edge:<layer>:edge:`. Cross-layer edges
/// (`lower_anchor`, the manifest/file `declares` edges, refinement maps) never
/// take that shape, so they are excluded without an allowlist of kinds.
fn layer_slice(manifest: &Value, layer: &str) -> Value {
    let node_prefix = format!("{layer}:");
    let edge_prefix = format!("edge:{layer}:edge:");
    let nodes = manifest["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter(|node| {
            node["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&node_prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    let edges = manifest["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|edge| {
            edge["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&edge_prefix))
                && edge["from"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&node_prefix))
                && edge["to"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&node_prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    serde_json::json!({"nodes": nodes, "edges": edges})
}

/// The negative control this gap never had.
#[test]
fn manifest_and_standalone_input_agree_on_the_graph_vocabulary() {
    let (manifest, status) = run(&["analyze", MANIFEST, "--projection", "traceability_graph"]);
    assert_eq!(status, 0, "{manifest:#}");

    for (layer, file) in LAYERS {
        let (standalone, status) = run(&["analyze", file, "--projection", "tsg"]);
        assert_eq!(status, 0, "{layer}: {standalone:#}");
        let slice = layer_slice(&manifest, layer);
        assert_eq!(
            node_kinds(&slice),
            node_kinds(&standalone),
            "{layer} node kinds diverge; manifest slice {slice:#}"
        );
        assert_eq!(
            edge_kinds(&slice),
            edge_kinds(&standalone),
            "{layer} edge kinds diverge; manifest slice {slice:#}"
        );
    }
}

/// Pin that the property above is not passing vacuously: every source-only
/// kind must really be present on the manifest side, under its layer prefix.
/// The requirements layer supplies the scenario nodes and their step edges,
/// the business layer supplies the `control` catalog entry.
#[test]
fn the_manifest_graph_carries_every_source_only_kind() {
    let (manifest, status) = run(&["analyze", MANIFEST, "--projection", "traceability_graph"]);
    assert_eq!(status, 0, "{manifest:#}");
    let ids = manifest["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<BTreeSet<_>>();
    for id in [
        "requirements:acceptance:AC-1",
        "requirements:forbidden:FB-1",
        "business:control:CTRL-CLOSURE",
    ] {
        assert!(ids.contains(id), "{id} missing: {manifest:#}");
    }
    let edges = manifest["edges"].as_array().expect("edges array");
    assert!(
        edges.iter().any(|edge| {
            edge["kind"] == "covers"
                && edge["from"] == "requirements:requirement:REQ-CASE"
                && edge["to"] == "requirements:acceptance:AC-1"
        }),
        "{manifest:#}"
    );
    // Step edges keep their direction, kind split, and step index, with both
    // endpoints carrying the layer prefix.
    for (kind, from, step) in [
        ("starts_with", "requirements:acceptance:AC-1", 0),
        ("starts_with", "requirements:forbidden:FB-1", 0),
        ("precedes", "requirements:forbidden:FB-1", 1),
    ] {
        assert!(
            edges.iter().any(|edge| {
                edge["kind"] == kind
                    && edge["from"] == from
                    && edge["to"] == "requirements:action:finish"
                    && edge["step"] == serde_json::json!(step)
            }),
            "missing {kind} step {step} from {from}: {manifest:#}"
        );
    }
}

/// Layer identity survives enrichment: `REQ-SHARED` is declared in both layer
/// files and must stay two distinct nodes, never merge into one.
#[test]
fn a_requirement_declared_in_two_layers_stays_two_nodes() {
    let (manifest, status) = run(&["analyze", MANIFEST, "--projection", "traceability_graph"]);
    assert_eq!(status, 0, "{manifest:#}");
    let ids = manifest["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(ids.contains("business:requirement:REQ-SHARED"), "{ids:?}");
    assert!(
        ids.contains("requirements:requirement:REQ-SHARED"),
        "{ids:?}"
    );
    assert!(!ids.contains("requirement:REQ-SHARED"), "{ids:?}");
}

/// The graph-wide well-formedness check, over every manifest-layer graph at
/// once: enrichment must not leave an edge pointing at a node it did not also
/// create, and layer prefixing must not break an endpoint.
#[test]
fn no_manifest_edge_dangles() {
    for manifest in [
        MANIFEST,
        "tests/fixtures/chain/fsl-project.toml",
        "tests/fixtures/rust_port/project_gap/fsl-project.toml",
    ] {
        let (graph, status) = run(&["analyze", manifest, "--projection", "traceability_graph"]);
        assert_eq!(status, 0, "{manifest}: {graph:#}");
        let ids = graph["nodes"]
            .as_array()
            .expect("nodes array")
            .iter()
            .filter_map(|node| node["id"].as_str())
            .collect::<BTreeSet<_>>();
        for edge in graph["edges"].as_array().expect("edges array") {
            assert!(
                ids.contains(edge["from"].as_str().unwrap_or_default()),
                "{manifest}: dangling edge.from: {edge:#}"
            );
            assert!(
                ids.contains(edge["to"].as_str().unwrap_or_default()),
                "{manifest}: dangling edge.to: {edge:#}"
            );
        }
    }
}
