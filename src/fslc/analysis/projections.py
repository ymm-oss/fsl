# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Graph projections over the Typed Semantic Graph."""
from __future__ import annotations

from .graph import connected_components, degree_summary, representative_cycles, strongly_connected_components
from .schema import GRAPH_SCHEMA_VERSION
from .tsg import ACTION_NODE_KINDS, PROPERTY_NODE_KINDS, SCENARIO_NODE_KINDS, build_tsg, node_by_id


SUPPORTED_PROJECTIONS = {
    "action_state_graph",
    "requirement_property_graph",
    "property_state_graph",
}


def analyze_projection(spec, projection):
    tsg = build_tsg(spec)
    nodes, edges = project_tsg(tsg, projection)
    node_ids = [n["id"] for n in nodes]
    return {
        "analysis": "structure",
        "projection": projection,
        "schema_version": GRAPH_SCHEMA_VERSION,
        "formal_status": "not_a_violation",
        "nodes": nodes,
        "edges": edges,
        "components": [
            {"id": f"component:{idx}", "nodes": comp}
            for idx, comp in enumerate(connected_components(node_ids, edges))
        ],
        "sccs": [
            {"id": f"scc:{idx}", "nodes": comp}
            for idx, comp in enumerate(strongly_connected_components(node_ids, edges))
        ],
        "cycles": [
            {"id": f"cycle:{idx}", "steps": cycle}
            for idx, cycle in enumerate(representative_cycles(node_ids, edges))
        ],
        "degree": degree_summary(node_ids, edges),
    }


def project_tsg(tsg, projection):
    if projection == "action_state_graph":
        return _action_state_graph(tsg)
    if projection == "requirement_property_graph":
        return _requirement_property_graph(tsg)
    if projection == "property_state_graph":
        return _property_state_graph(tsg)
    raise ValueError(f"unsupported projection: {projection}")


def _action_state_graph(tsg):
    nodes_by_id = node_by_id(tsg)
    action_ids = {n["id"] for n in tsg["nodes"] if n["kind"] == "action"}
    state_ids = {n["id"] for n in tsg["nodes"] if n["kind"] == "state"}
    selected_edges = []

    def owner_action(node_id):
        if node_id in action_ids:
            return node_id
        node = nodes_by_id.get(node_id) or {}
        action = node.get("action")
        return action if action in action_ids else None

    for e in tsg["edges"]:
        if e["kind"] == "writes" and e["to"] in state_ids:
            action = owner_action(e["from"])
            if action:
                selected_edges.append(_projection_edge(action, "writes", e["to"]))
        elif e["kind"] == "reads" and e["to"] in state_ids:
            action = owner_action(e["from"])
            if action:
                selected_edges.append(_projection_edge(e["to"], "read_by", action))
    selected_ids = set(action_ids) | set(state_ids)
    return _select_nodes_edges(tsg, selected_ids, selected_edges)


def _requirement_property_graph(tsg):
    selected_ids = {
        n["id"]
        for n in tsg["nodes"]
        if n["kind"] in PROPERTY_NODE_KINDS | SCENARIO_NODE_KINDS | {"requirement", "action", "kpi", "control"}
    }
    selected_edges = [
        _projection_edge(e["from"], e["kind"], e["to"])
        for e in tsg["edges"]
        if e["kind"] in {"covers", "precedes", "starts_with"}
        and e["from"] in selected_ids
        and e["to"] in selected_ids
    ]
    return _select_nodes_edges(tsg, selected_ids, selected_edges)


def _property_state_graph(tsg):
    selected_ids = {
        n["id"]
        for n in tsg["nodes"]
        if n["kind"] in PROPERTY_NODE_KINDS | {"state"}
    }
    selected_edges = [
        _projection_edge(e["from"], e["kind"], e["to"])
        for e in tsg["edges"]
        if e["kind"] in {"reads", "checks"}
        and e["from"] in selected_ids
        and e["to"] in selected_ids
    ]
    return _select_nodes_edges(tsg, selected_ids, selected_edges)


def _select_nodes_edges(tsg, selected_ids, selected_edges):
    nodes = sorted(
        [n for n in tsg["nodes"] if n["id"] in selected_ids],
        key=lambda n: (n["kind"], n["id"]),
    )
    edges = sorted(
        _dedupe_edges(selected_edges),
        key=lambda e: (e["kind"], e["from"], e["to"], e["id"]),
    )
    return nodes, edges


def _projection_edge(src, kind, dst):
    return {
        "id": f"edge:{src}:{kind}:{dst}",
        "kind": kind,
        "from": src,
        "to": dst,
        "formal_status": "not_a_violation",
    }


def _dedupe_edges(edges):
    seen = set()
    out = []
    for e in edges:
        if e["id"] in seen:
            continue
        seen.add(e["id"])
        out.append(e)
    return out
