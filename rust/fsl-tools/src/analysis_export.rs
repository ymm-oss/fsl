// SPDX-License-Identifier: Apache-2.0

//! Deterministic text exports for structural analysis graphs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn label(value: &Value) -> &str {
    for key in ["label", "name", "id"] {
        if let Some(label) = value.get(key).and_then(Value::as_str)
            && !label.is_empty()
        {
            return label;
        }
    }
    ""
}

fn sorted_graph(analysis: &Value) -> (Vec<&Value>, Vec<&Value>) {
    let mut nodes = analysis["nodes"]
        .as_array()
        .map_or_else(Vec::new, |values| values.iter().collect());
    nodes.sort_by_key(|node| field(node, "id"));
    let mut edges = analysis["edges"]
        .as_array()
        .map_or_else(Vec::new, |values| values.iter().collect());
    edges.sort_by_key(|edge| {
        (
            field(edge, "from"),
            field(edge, "to"),
            field(edge, "kind"),
            field(edge, "id"),
        )
    });
    (nodes, edges)
}

fn dot_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn dot_shape(kind: &str) -> &'static str {
    match kind {
        "requirement" | "control" | "business_spec" | "requirements_spec" | "design_spec"
        | "impl_spec" | "abs_spec" => "box",
        "action" | "impl_action" | "abs_action" | "action_map" => "ellipse",
        "state" | "phys_state" | "state_map" | "abs_state" | "map_expr_read" => "cylinder",
        "invariant" | "trans" | "leadsTo" | "reachable" | "progress_response" => "diamond",
        "acceptance" | "forbidden" => "note",
        _ => "plaintext",
    }
}

fn to_dot(analysis: &Value) -> String {
    let (nodes, edges) = sorted_graph(analysis);
    let mut output = "digraph fsl_analysis {\n  rankdir=LR;\n".to_owned();
    for node in nodes {
        let _ = writeln!(
            output,
            "  \"{}\" [label=\"{}\", shape=\"{}\"];",
            dot_escape(field(node, "id")),
            dot_escape(label(node)),
            dot_shape(field(node, "kind"))
        );
    }
    for edge in edges {
        let edge_label = edge
            .get("label")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field(edge, "kind"));
        let _ = writeln!(
            output,
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            dot_escape(field(edge, "from")),
            dot_escape(field(edge, "to")),
            dot_escape(edge_label)
        );
    }
    output.push_str("}\n");
    output
}

fn mermaid_label(value: &str) -> String {
    value
        .replace('\n', " ")
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('[', "(")
        .replace(']', ")")
        .replace('|', "/")
}

fn mermaid_shape(kind: &str, label: &str) -> String {
    match kind {
        "action" | "impl_action" | "abs_action" | "action_map" => format!("((\" {label} \"))"),
        "invariant" | "trans" | "leadsTo" | "reachable" | "progress_response" => {
            format!("{{{{\"{label}\"}}}}")
        }
        "state" | "phys_state" | "state_map" | "abs_state" | "map_expr_read" => {
            format!("[/\"{label}\"/]")
        }
        _ => format!("[\"{label}\"]"),
    }
}

fn to_mermaid(analysis: &Value) -> String {
    let (nodes, edges) = sorted_graph(analysis);
    let mut ids = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let source = field(node, "id");
        let mut id = source
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        if id.is_empty() || id.starts_with(|character: char| character.is_ascii_digit()) {
            id = format!("n_{id}");
        }
        if !seen.insert(id.clone()) {
            id = format!("{id}_{index}");
            seen.insert(id.clone());
        }
        ids.insert(source.to_owned(), id);
    }
    let mut output = "graph TD\n".to_owned();
    for node in nodes {
        let _ = writeln!(
            output,
            "  {}{}",
            ids[field(node, "id")],
            mermaid_shape(field(node, "kind"), &mermaid_label(label(node)))
        );
    }
    for edge in edges {
        let (Some(from), Some(to)) = (ids.get(field(edge, "from")), ids.get(field(edge, "to")))
        else {
            continue;
        };
        let edge_label = edge
            .get("label")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field(edge, "kind"));
        let _ = writeln!(output, "  {from} -->|{}| {to}", mermaid_label(edge_label));
    }
    output
}

/// Export a graph analysis as deterministic DOT or Mermaid text.
///
/// # Errors
///
/// Returns an error when `format` is not a supported graph export format.
pub fn export_analysis_graph(analysis: &Value, format: &str) -> Result<String, String> {
    match format {
        "dot" => Ok(to_dot(analysis)),
        "mermaid" => Ok(to_mermaid(analysis)),
        _ => Err(format!("unsupported analyze format: {format}")),
    }
}
