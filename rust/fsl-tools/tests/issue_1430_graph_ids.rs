// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1430 public-function detectors. Hand-built JSON expresses identifiers that
//! the ASCII-only causal CLI cannot accept, and deliberately triggers the
//! analysis exporter's sorted-index suffix collision without parser constraints.

use std::collections::{BTreeMap, BTreeSet};

use fsl_tools::{causal_dot, causal_mermaid, export_analysis_graph};
use serde_json::json;

/// Detector (fails on 9ad07c7a): sorted ids a!, a!_2, a? give indices 0,1,2;
/// the last node's fallback `a__2` is already the second node's sanitized id.
/// Every edge is checked against the id assigned to its own labeled node.
#[test]
fn analysis_suffix_collision_keeps_nodes_and_edges_distinct() {
    let graph = json!({
        "nodes": [
            {"id": "a?", "label": "third"},
            {"id": "a!_2", "label": "second"},
            {"id": "a!", "label": "first"}
        ],
        "edges": [
            {"from": "a!", "to": "a!_2", "kind": "one"},
            {"from": "a!_2", "to": "a?", "kind": "two"},
            {"from": "a?", "to": "a!", "kind": "three"}
        ]
    });
    let text = export_analysis_graph(&graph, "mermaid").expect("supported format");
    let ids: BTreeMap<_, _> = text
        .lines()
        .filter_map(|line| {
            let (id, label) = line.trim().split_once("[\"")?;
            Some((label.strip_suffix("\"]")?, id))
        })
        .collect();
    let distinct: BTreeSet<_> = ids.values().collect();
    assert_eq!(ids.len(), 3, "produced Mermaid={text}");
    assert_eq!(distinct.len(), 3, "produced ids={ids:?}; Mermaid={text}");
    // Previously unique nodes retain their exact ids.
    assert_eq!(ids["first"], "a_", "produced Mermaid={text}");
    assert_eq!(ids["second"], "a__2", "produced Mermaid={text}");
    for (from, to, label) in [
        ("first", "second", "one"),
        ("second", "third", "two"),
        ("third", "first", "three"),
    ] {
        let expected = format!("  {} -->|{label}| {}", ids[from], ids[to]);
        assert!(
            text.lines().any(|line| line == expected),
            "missing {expected:?}; produced Mermaid={text}"
        );
    }
}

/// Control (passes before/after): collision-free sorted export is byte-identical,
/// including its sanitized ids, node shapes, edge labels and final newline.
#[test]
fn analysis_unique_ids_keep_exact_bytes() {
    let graph = json!({
        "nodes": [{"id": "node:2", "label": "two"}, {"id": "node:1", "label": "one"}],
        "edges": [{"from": "node:2", "to": "node:1", "kind": "uses"}]
    });
    let text = export_analysis_graph(&graph, "mermaid").expect("supported format");
    assert_eq!(
        text, "graph TD\n  node_1[\"one\"]\n  node_2[\"two\"]\n  node_2 -->|uses| node_1\n",
        "produced Mermaid={text}"
    );
}

/// Detector (fails on 9ad07c7a): non-injective sanitization must retain first
/// occurrence `a_b` and allocate later suffixes, skipping already-used `a_b_2`.
/// The exact full string also pins every source/target remapping.
#[test]
fn causal_collisions_skip_used_suffixes_and_remap_edges() {
    let projection = json!({
        "nodes": [{"id": "a:b"}, {"id": "a_b_2"}, {"id": "a-b"}, {"id": "a/b"}],
        "edges": [
            {"source": "a:b", "target": "a-b", "kind": "first"},
            {"source": "a-b", "target": "a_b_2", "kind": "second"},
            {"source": "a/b", "target": "a:b", "kind": "third"}
        ]
    });
    let text = causal_mermaid(&projection);
    let expected = concat!(
        "graph LR\n",
        "  a_b[\"a:b\"]\n",
        "  a_b_2[\"a_b_2\"]\n",
        "  a_b_3[\"a-b\"]\n",
        "  a_b_4[\"a/b\"]\n",
        "  a_b -->|\"first\"| a_b_3\n",
        "  a_b_3 -->|\"second\"| a_b_2\n",
        "  a_b_4 -->|\"third\"| a_b\n"
    );
    assert_eq!(text, expected, "produced Mermaid={text}");
}

/// Detector (fails on 9ad07c7a): quoted ids, role labels, claim ids and generic
/// edge labels must escape BOTH quotes and backslashes in every DOT string.
#[test]
fn causal_dot_escapes_ids_and_all_label_sources() {
    let projection = json!({
        "nodes": [{"id": "variable:a\"\\x", "role": "r\"\\s"}, {"id": "variable:b"}],
        "edges": [
            {"source": "variable:a\"\\x", "target": "variable:b", "kind": "claim", "id": "claim:c\"\\d"},
            {"source": "variable:b", "target": "variable:a\"\\x", "kind": "k\"\\l"}
        ]
    });
    let text = causal_dot(&projection);
    let expected = concat!(
        "digraph causal {\n",
        r#"  "variable:a\"\\x" [label="variable:a\"\\x\n(r\"\\s)"];"#,
        "\n",
        r#"  "variable:b";"#,
        "\n",
        r#"  "variable:a\"\\x" -> "variable:b" [label="claim:c\"\\d"];"#,
        "\n",
        r#"  "variable:b" -> "variable:a\"\\x" [label="k\"\\l"];"#,
        "\n",
        "}\n"
    );
    assert_eq!(text, expected, "produced DOT={text}");
}

/// Control (passes before/after): collision-free ASCII causal Mermaid/DOT
/// outputs preserve all bytes, including existing role-newline escaping.
#[test]
fn causal_ascii_outputs_keep_exact_bytes() {
    let projection = json!({
        "nodes": [{"id": "variable:a", "role": "intervention"}, {"id": "variable:b"}],
        "edges": [{"source": "variable:a", "target": "variable:b", "kind": "claim", "id": "claim:AB", "polarity": "positive"}]
    });
    let mermaid = causal_mermaid(&projection);
    assert_eq!(
        mermaid,
        concat!(
            "graph LR\n  variable_a[\"variable:a (intervention)\"]\n",
            "  variable_b[\"variable:b\"]\n",
            "  variable_a -->|\"claim:AB positive\"| variable_b\n"
        ),
        "produced Mermaid={mermaid}"
    );
    let dot = causal_dot(&projection);
    assert_eq!(
        dot,
        concat!(
            "digraph causal {\n",
            "  \"variable:a\" [label=\"variable:a\\n(intervention)\"];\n",
            "  \"variable:b\";\n",
            "  \"variable:a\" -> \"variable:b\" [label=\"claim:AB\"];\n}\n"
        ),
        "produced DOT={dot}"
    );
}
