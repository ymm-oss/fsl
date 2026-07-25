// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #498, two independent defects:
//!
//! AN-6: the internal db-dialect separator sentinel `QqDbSepqQ` leaked into
//! `analyze` node labels instead of the display name `verify` reports for
//! the same target, and `--focus` only accepted the raw sentinel form.
//!
//! AN-4: `action_dependency_graph` deduplicated `enables`/`conflicts_with`
//! edges by `(from, kind, to)` alone, so an action pair connected through
//! more than one shared read/write state bridge silently kept only the
//! alphabetically-last bridge — a presentation change (rename a state
//! variable) could flip which bridge survives.

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

const DB_SPEC: &str = "examples/db/unsafe_not_null_before_backfill.fsl";

/// A db-dialect invariant's TSG `label` must match the display name
/// `verify` reports for the same violation, not the raw internal
/// `QqDbSepqQ` separator sentinel.
#[test]
fn db_dialect_labels_use_the_display_name_not_the_internal_sentinel() {
    let (verify, verify_status) = run(&["verify", DB_SPEC, "--depth", "3"]);
    assert_eq!(verify_status, 1, "{verify:#}");
    assert_eq!(verify["result"], "violated");
    let display_invariant = verify["invariant"]
        .as_str()
        .expect("verify invariant")
        .to_owned();
    assert!(
        !display_invariant.contains("QqDbSepqQ"),
        "fixture drifted: {display_invariant}"
    );

    let (analyze, status) = run(&["analyze", DB_SPEC, "--projection", "tsg"]);
    assert_eq!(status, 0, "{analyze:#}");
    let nodes = analyze["nodes"].as_array().expect("nodes array");
    let node = nodes
        .iter()
        .find(|node| node["label"] == display_invariant)
        .unwrap_or_else(|| {
            panic!("no node labeled {display_invariant}, sentinel leaked?: {analyze:#}")
        });
    // The id itself keeps its raw, guaranteed-unique internal form by
    // design (only the label is sanitized) — `--focus` accepting the
    // displayed name too is covered separately below.
    assert!(
        node["id"]
            .as_str()
            .unwrap_or_default()
            .contains("QqDbSepqQ"),
        "{node:#}"
    );
}

/// `--focus` must accept the canonical displayed node name (what a caller
/// would copy from a `verify` violation), not only the raw internal id
/// carrying the `QqDbSepqQ` sentinel.
#[test]
fn focus_accepts_the_displayed_name_as_well_as_the_raw_sentinel_id() {
    let (verify, _) = run(&["verify", DB_SPEC, "--depth", "3"]);
    let display_invariant = verify["invariant"].as_str().expect("verify invariant");
    let focus_display = format!("invariant:{display_invariant}");

    let (by_display, status) = run(&[
        "analyze",
        DB_SPEC,
        "--projection",
        "impact_graph",
        "--focus",
        &focus_display,
    ]);
    assert_eq!(status, 0, "{by_display:#}");
    assert_ne!(by_display["kind"], "name");
    let nodes_via_display = by_display["nodes"].as_array().expect("nodes").len();

    // Regression control: the raw sentinel id must keep working too.
    let (tsg, _) = run(&["analyze", DB_SPEC, "--projection", "tsg"]);
    let raw_id = tsg["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["label"] == display_invariant)
        .and_then(|node| node["id"].as_str())
        .expect("raw id")
        .to_owned();
    let (by_raw, raw_status) = run(&[
        "analyze",
        DB_SPEC,
        "--projection",
        "impact_graph",
        "--focus",
        &raw_id,
    ]);
    assert_eq!(raw_status, 0, "{by_raw:#}");
    assert_eq!(
        by_raw["nodes"].as_array().expect("nodes").len(),
        nodes_via_display,
        "display and raw focus must resolve to the same node"
    );
}

/// Regression control: an unknown focus id (neither a real node id nor a
/// display name that resolves to one) must still be a real error.
#[test]
fn focus_still_rejects_a_genuinely_unknown_node() {
    let (output, status) = run(&[
        "analyze",
        DB_SPEC,
        "--projection",
        "impact_graph",
        "--focus",
        "invariant:totally_bogus",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "name");
}

/// The actual deliverable for AN-4: an action pair connected through two
/// shared read/write state bridges must keep *both* bridges in the
/// `enables` edge's `states` field, not just one.
#[test]
fn action_dependency_graph_keeps_every_shared_bridge_state() {
    let (output, status) = run(&[
        "analyze",
        "rust/fslc/tests/fixtures/analyze_multi_bridge.fsl",
        "--projection",
        "action_dependency_graph",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let edges = output["edges"].as_array().expect("edges array");
    let enabling_edge = edges
        .iter()
        .find(|edge| {
            edge["kind"] == "enables"
                && edge["from"] == "action:producer"
                && edge["to"] == "action:consumer"
        })
        .unwrap_or_else(|| panic!("producer->consumer enables edge missing: {output:#}"));
    let bridge_states = enabling_edge["states"]
        .as_array()
        .expect("states array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        bridge_states,
        std::collections::BTreeSet::from(["state:audit", "state:stock"]),
        "both shared bridges must survive: {enabling_edge:#}"
    );
}
