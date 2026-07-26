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
//!
//! The second half covers the remaining documented vocabulary: `control`
//! nodes and the `starts_with`/`precedes` step edges. Their detection-power
//! payload is the mirror image of the first half's — scenario nodes had no
//! outgoing edge at all, so `unanchored_property`'s scenario-anchor
//! suppression could never apply and reported a scenario-anchored `reachable`
//! as unanchored.

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

/// Acceptance/forbidden step ordering must use the direction and the
/// first-step/later-step split the frozen reference fixes
/// (`src/fslc/analysis/tsg.py` `_add_scenario_steps`): the edge runs
/// scenario -> action, step 0 is `starts_with`, every later step is
/// `precedes`. Before the fix no scenario node had *any* outgoing edge, so
/// both documented edge kinds (`docs/DESIGN-analysis.md` §2) were never
/// emitted by native at all.
#[test]
fn scenario_step_edges_use_the_frozen_reference_direction_and_split() {
    let (output, status) = run(&[
        "analyze",
        "examples/e2e/2_requirements.fsl",
        "--projection",
        "tsg",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let edges = output["edges"].as_array().expect("edges array");
    let steps = edges
        .iter()
        .filter(|edge| matches!(edge["kind"].as_str(), Some("starts_with" | "precedes")))
        .collect::<Vec<_>>();
    assert!(!steps.is_empty(), "{output:#}");
    // `acceptance AC-1 { submit(0, 1) auto_approve(0) pay(0) }`.
    let expected = [
        ("starts_with", "action:submit", 0),
        ("precedes", "action:auto_approve", 1),
        ("precedes", "action:pay", 2),
    ];
    for (kind, action, step) in expected {
        assert!(
            steps.iter().any(|edge| {
                edge["kind"] == kind
                    && edge["from"] == "acceptance:AC-1"
                    && edge["to"] == action
                    && edge["step"] == serde_json::json!(step)
            }),
            "missing {kind} step {step} to {action}: {output:#}"
        );
    }
    // Direction is load-bearing: consumers read the TSG as structural truth,
    // and `requirement_property_graph` only keeps an edge whose endpoints are
    // both selected. Never action -> scenario.
    assert!(
        !steps.iter().any(|edge| edge["from"]
            .as_str()
            .unwrap_or_default()
            .starts_with("action:")),
        "{output:#}"
    );
    assert_graph_has_no_dangling_edge(&output);
}

/// A scenario that calls the same action twice must keep one edge per step:
/// the step index is part of the edge id, so the two do not collapse.
#[test]
fn repeated_actions_in_one_scenario_keep_one_edge_per_step() {
    let (output, status) = run(&[
        "analyze",
        "examples/annotations/annotated_claims.fsl",
        "--projection",
        "tsg",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let edges = output["edges"].as_array().expect("edges array");
    // `forbidden NEG-1 { submit(0) submit(0) }` — the same action twice.
    let repeated = edges
        .iter()
        .filter(|edge| edge["from"] == "forbidden:NEG-1" && edge["to"] == "action:submit")
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2, "{output:#}");
    assert_eq!(repeated[0]["kind"], "starts_with", "{output:#}");
    assert_eq!(repeated[0]["step"], serde_json::json!(0), "{output:#}");
    assert_eq!(repeated[1]["kind"], "precedes", "{output:#}");
    assert_eq!(repeated[1]["step"], serde_json::json!(1), "{output:#}");
    let ids = edges
        .iter()
        .filter(|edge| matches!(edge["kind"].as_str(), Some("starts_with" | "precedes")))
        .filter_map(|edge| edge["id"].as_str())
        .collect::<Vec<_>>();
    assert!(!ids.is_empty(), "{output:#}");
    let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        unique.len(),
        "duplicate step edge id: {output:#}"
    );
    assert!(ids.iter().all(|id| id.contains(":step:")), "{output:#}");
}

/// `control` catalog entries have no Kernel-lowered form
/// (`docs/LANGUAGE.md`: "does not generate a property by itself; it is a
/// catalog entry"), so before the fix a governance catalog declaring two
/// controls projected zero `control` nodes even though
/// `requirement_property_graph` already selected that node kind.
#[test]
fn governance_control_catalog_entries_appear_in_the_tsg() {
    let (output, status) = run(&[
        "analyze",
        "examples/consulting/governance_controls.fsl",
        "--projection",
        "tsg",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let nodes = output["nodes"].as_array().expect("nodes array");
    for id in ["control:CTRL-1", "control:CTRL-2"] {
        let node = nodes
            .iter()
            .find(|node| node["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing: {output:#}"));
        assert_eq!(node["kind"], "control", "{output:#}");
    }
    let edges = output["edges"].as_array().expect("edges array");
    assert!(
        edges.iter().any(|edge| {
            edge["kind"] == "declares"
                && edge["from"] == "spec:ExpenseTransformationControls"
                && edge["to"] == "control:CTRL-1"
        }),
        "{output:#}"
    );
    assert_graph_has_no_dangling_edge(&output);
}

/// The detection-power half. `unanchored_property` suppresses a `reachable`
/// that an acceptance scenario anchors by driving an action
/// (`rust/fsl-tools/src/analysis.rs`: `scenario_actions && kind ==
/// "reachable"`). That clause needs a scenario -> action edge, and before the
/// fix no scenario node had any outgoing edge on native, so the clause was
/// structurally dead and the finding was a guaranteed false positive.
#[test]
fn a_scenario_anchored_reachable_is_not_reported_as_unanchored() {
    let (output, status) = run(&[
        "analyze",
        "rust/fslc/tests/fixtures/analyze_scenario_anchored_reachable.fsl",
        "--profile",
        "ai-review",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    assert!(
        !findings.iter().any(|finding| {
            finding["finding_type"] == "unanchored_property"
                && finding["involved_nodes"] == serde_json::json!(["reachable:AllClosed"])
        }),
        "{output:#}"
    );
}

fn assert_graph_has_no_dangling_edge(output: &Value) {
    let node_ids = output["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for edge in output["edges"].as_array().expect("edges array") {
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
