# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""DOT and Mermaid exporters for structural analysis graphs."""
from __future__ import annotations

import re


def export_graph(analysis, output_format):
    if output_format == "dot":
        return to_dot(analysis)
    if output_format == "mermaid":
        return to_mermaid(analysis)
    raise ValueError(f"unsupported graph export format: {output_format}")


def to_dot(analysis):
    lines = ["digraph fsl_analysis {"]
    lines.append("  rankdir=LR;")
    for n in sorted(analysis.get("nodes", []), key=lambda item: item["id"]):
        attrs = {
            "label": _dot_escape(n.get("label") or n.get("name") or n["id"]),
            "shape": _dot_shape(n.get("kind")),
        }
        attr_text = ", ".join(f'{k}="{v}"' for k, v in attrs.items())
        lines.append(f'  "{_dot_escape(n["id"])}" [{attr_text}];')
    for e in sorted(analysis.get("edges", []), key=lambda item: (item["from"], item["to"], item["kind"], item["id"])):
        label = _dot_escape(e.get("label") or e.get("kind") or "")
        lines.append(f'  "{_dot_escape(e["from"])}" -> "{_dot_escape(e["to"])}" [label="{label}"];')
    lines.append("}")
    return "\n".join(lines) + "\n"


def to_mermaid(analysis):
    nodes = sorted(analysis.get("nodes", []), key=lambda item: item["id"])
    id_map = _mermaid_ids(nodes)
    lines = ["graph TD"]
    for n in nodes:
        mid = id_map[n["id"]]
        label = _mermaid_label(n.get("label") or n.get("name") or n["id"])
        lines.append(f"  {mid}{_mermaid_node_shape(n.get('kind'), label)}")
    for e in sorted(analysis.get("edges", []), key=lambda item: (item["from"], item["to"], item["kind"], item["id"])):
        if e["from"] not in id_map or e["to"] not in id_map:
            continue
        label = _mermaid_label(e.get("label") or e.get("kind") or "")
        lines.append(f"  {id_map[e['from']]} -->|{label}| {id_map[e['to']]}")
    return "\n".join(lines) + "\n"


def _dot_escape(value):
    return str(value).replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def _dot_shape(kind):
    if kind in {"requirement", "control", "business_spec", "requirements_spec", "design_spec", "impl_spec", "abs_spec"}:
        return "box"
    if kind in {"action", "impl_action", "abs_action", "action_map"}:
        return "ellipse"
    if kind in {"state", "phys_state", "state_map", "abs_state", "map_expr_read"}:
        return "cylinder"
    if kind in {"invariant", "trans", "leadsTo", "reachable", "progress_response"}:
        return "diamond"
    if kind in {"acceptance", "forbidden"}:
        return "note"
    return "plaintext"


def _mermaid_ids(nodes):
    out = {}
    seen = set()
    for idx, n in enumerate(nodes):
        mid = re.sub(r"[^A-Za-z0-9_]", "_", n["id"])
        if not mid or mid[0].isdigit():
            mid = f"n_{mid}"
        candidate = mid
        if candidate in seen:
            candidate = f"{mid}_{idx}"
        seen.add(candidate)
        out[n["id"]] = candidate
    return out


def _mermaid_label(value):
    text = str(value).replace("\n", " ")
    return (
        text.replace("&", "&amp;")
        .replace('"', "&quot;")
        .replace("[", "(")
        .replace("]", ")")
        .replace("|", "/")
    )


def _mermaid_node_shape(kind, label):
    if kind in {"action", "impl_action", "abs_action", "action_map"}:
        return f'((" {label} "))'
    if kind in {"invariant", "trans", "leadsTo", "reachable", "progress_response"}:
        return f'{{{{"{label}"}}}}'
    if kind in {"state", "phys_state", "state_map", "abs_state", "map_expr_read"}:
        return f'[/"{label}"/]'
    return f'["{label}"]'
