# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Graph projections over the Typed Semantic Graph."""
from __future__ import annotations

from itertools import combinations

from .graph import (
    bfs_distances,
    connected_components,
    degree_summary,
    directed_adjacency,
    metrics_summary,
    representative_cycles,
    reverse_directed_adjacency,
    strongly_connected_components,
)
from .schema import GRAPH_SCHEMA_VERSION
from .tsg import ACTION_NODE_KINDS, PROPERTY_NODE_KINDS, SCENARIO_NODE_KINDS, build_tsg, node_by_id
from ..model import FslError


SUPPORTED_PROJECTIONS = {
    "action_state_graph",
    "action_dependency_graph",
    "impact_graph",
    "requirement_property_graph",
    "property_state_graph",
}


def analyze_projection(spec, projection, focus=None):
    tsg = build_tsg(spec)
    nodes, edges = project_tsg(tsg, projection, focus=focus)
    node_ids = [n["id"] for n in nodes]
    components = connected_components(node_ids, edges)
    sccs = strongly_connected_components(node_ids, edges)
    return {
        "analysis": "structure",
        "projection": projection,
        "schema_version": GRAPH_SCHEMA_VERSION,
        "formal_status": "not_a_violation",
        "nodes": nodes,
        "edges": edges,
        "components": [
            {"id": f"component:{idx}", "nodes": comp}
            for idx, comp in enumerate(components)
        ],
        "sccs": [
            {"id": f"scc:{idx}", "nodes": comp}
            for idx, comp in enumerate(sccs)
        ],
        "cycles": [
            {"id": f"cycle:{idx}", "steps": cycle}
            for idx, cycle in enumerate(representative_cycles(node_ids, edges))
        ],
        "degree": degree_summary(node_ids, edges),
        "metrics": metrics_summary(node_ids, edges, components=components, sccs=sccs),
    }


def project_tsg(tsg, projection, focus=None):
    if focus is not None and projection != "impact_graph":
        raise FslError("--focus is supported only with --projection impact_graph", kind="semantics")
    if projection == "action_state_graph":
        return _action_state_graph(tsg)
    if projection == "action_dependency_graph":
        return _action_dependency_graph(tsg)
    if projection == "impact_graph":
        return _impact_graph(tsg, focus)
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


def build_action_dependency_graph(nodes, edges, include_conflicts=False):
    action_nodes = sorted(n["id"] for n in nodes if n["kind"] == "action")
    state_writers = {}
    state_readers = {}
    for e in edges:
        if e["kind"] == "writes" and e["from"].startswith("action:") and e["to"].startswith("state:"):
            state_writers.setdefault(e["to"], set()).add(e["from"])
        elif e["kind"] == "read_by" and e["from"].startswith("state:") and e["to"].startswith("action:"):
            state_readers.setdefault(e["from"], set()).add(e["to"])

    dep_edges = []
    bridges = {}
    enables = {}
    for state in sorted(set(state_writers) & set(state_readers)):
        for writer in sorted(state_writers[state]):
            for reader in sorted(state_readers[state]):
                if writer == reader:
                    continue
                enables.setdefault((writer, reader), set()).add(state)
    for (writer, reader), states in sorted(enables.items()):
        sorted_states = sorted(states)
        dep_edges.append(_projection_edge(
            writer,
            "enables",
            reader,
            state=sorted_states[0] if len(sorted_states) == 1 else None,
            states=sorted_states,
        ))
        bridges[(writer, reader)] = sorted_states[0]

    if include_conflicts:
        conflicts = {}
        for state, writers in sorted(state_writers.items()):
            for left, right in combinations(sorted(writers), 2):
                conflicts.setdefault((left, right), set()).add(state)
        for (left, right), states in sorted(conflicts.items()):
            sorted_states = sorted(states)
            dep_edges.append(_projection_edge(
                left,
                "conflicts_with",
                right,
                state=sorted_states[0] if len(sorted_states) == 1 else None,
                states=sorted_states,
                symmetric=True,
            ))

    return action_nodes, dep_edges, bridges


def _action_dependency_graph(tsg):
    projection_nodes, projection_edges = _action_state_graph(tsg)
    action_ids, dep_edges, _bridges = build_action_dependency_graph(
        projection_nodes,
        projection_edges,
        include_conflicts=True,
    )
    return _select_nodes_edges(tsg, set(action_ids), dep_edges)


def _impact_graph(tsg, focus):
    if not focus:
        raise FslError("--projection impact_graph requires --focus <node-id>", kind="semantics")
    nodes_by_id = node_by_id(tsg)
    if focus not in nodes_by_id:
        raise FslError(
            f"unknown analyze focus node: {focus}",
            kind="name",
            hint="use a node id from `fslc analyze FILE --projection tsg`, for example state:x or action:checkout",
        )
    node_ids = [n["id"] for n in tsg["nodes"]]
    downstream = bfs_distances(focus, directed_adjacency(node_ids, tsg["edges"]))
    upstream = bfs_distances(focus, reverse_directed_adjacency(node_ids, tsg["edges"]))
    selected_ids = set(upstream) | set(downstream) | {focus}

    nodes = []
    for n in tsg["nodes"]:
        node_id = n["id"]
        if node_id not in selected_ids:
            continue
        item = dict(n)
        if node_id == focus:
            direction = "focus"
            directions = ["focus"]
        elif node_id in upstream and node_id in downstream:
            direction = "upstream" if upstream[node_id] <= downstream[node_id] else "downstream"
            directions = ["upstream", "downstream"]
        elif node_id in upstream:
            direction = "upstream"
            directions = ["upstream"]
        else:
            direction = "downstream"
            directions = ["downstream"]
        distances = [
            d for d in (upstream.get(node_id), downstream.get(node_id))
            if d is not None
        ]
        item["direction"] = direction
        item["directions"] = directions
        item["focus_distance"] = min(distances) if distances else 0
        if node_id in upstream:
            item["upstream_distance"] = upstream[node_id]
        if node_id in downstream:
            item["downstream_distance"] = downstream[node_id]
        nodes.append(item)

    edges = []
    for e in tsg["edges"]:
        if e["from"] not in selected_ids or e["to"] not in selected_ids:
            continue
        item = dict(e)
        item.setdefault("formal_status", "not_a_violation")
        edges.append(item)
    return (
        sorted(nodes, key=lambda n: (n["kind"], n["id"])),
        sorted(edges, key=lambda e: (e["kind"], e["from"], e["to"], e["id"])),
    )


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


def _projection_edge(src, kind, dst, **extra):
    out = {
        "id": f"edge:{src}:{kind}:{dst}",
        "kind": kind,
        "from": src,
        "to": dst,
        "formal_status": "not_a_violation",
    }
    for key, value in extra.items():
        if value is not None:
            out[key] = value
    return out


def _dedupe_edges(edges):
    seen = set()
    out = []
    for e in edges:
        if e["id"] in seen:
            continue
        seen.add(e["id"])
        out.append(e)
    return out
