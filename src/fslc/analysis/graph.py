# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Deterministic graph primitives for structural analysis."""
from __future__ import annotations


def directed_adjacency(nodes, edges):
    return _directed_adjacency(nodes, edges)


def reverse_directed_adjacency(nodes, edges):
    adjacency = {str(n): [] for n in nodes}
    for src, dst in _edge_pairs(edges):
        adjacency.setdefault(src, [])
        adjacency.setdefault(dst, [])
        adjacency[dst].append(src)
    return {node: sorted(set(nbrs)) for node, nbrs in adjacency.items()}


def bfs_distances(focus, adjacency):
    focus = str(focus)
    if focus not in adjacency:
        return {}
    distances = {focus: 0}
    queue = [focus]
    idx = 0
    while idx < len(queue):
        cur = queue[idx]
        idx += 1
        for nxt in adjacency.get(cur, []):
            if nxt in distances:
                continue
            distances[nxt] = distances[cur] + 1
            queue.append(nxt)
    return distances


def connected_components(nodes, edges):
    adjacency = _undirected_adjacency(nodes, edges)
    seen = set()
    components = []
    for start in sorted(adjacency):
        if start in seen:
            continue
        stack = [start]
        comp = []
        seen.add(start)
        while stack:
            cur = stack.pop()
            comp.append(cur)
            for nxt in reversed(adjacency[cur]):
                if nxt not in seen:
                    seen.add(nxt)
                    stack.append(nxt)
        components.append(sorted(comp))
    return sorted(components, key=lambda c: (c[0] if c else "", len(c), c))


def strongly_connected_components(nodes, edges, include_singletons=False):
    adjacency = _directed_adjacency(nodes, edges)
    index = 0
    stack = []
    on_stack = set()
    indexes = {}
    lowlinks = {}
    out = []

    def visit(v):
        nonlocal index
        indexes[v] = index
        lowlinks[v] = index
        index += 1
        stack.append(v)
        on_stack.add(v)
        for w in adjacency[v]:
            if w not in indexes:
                visit(w)
                lowlinks[v] = min(lowlinks[v], lowlinks[w])
            elif w in on_stack:
                lowlinks[v] = min(lowlinks[v], indexes[w])
        if lowlinks[v] == indexes[v]:
            comp = []
            while True:
                w = stack.pop()
                on_stack.remove(w)
                comp.append(w)
                if w == v:
                    break
            comp = sorted(comp)
            has_self_loop = any(src == dst == comp[0] for src, dst in _edge_pairs(edges)) if len(comp) == 1 else False
            if include_singletons or len(comp) > 1 or has_self_loop:
                out.append(comp)

    for node in sorted(adjacency):
        if node not in indexes:
            visit(node)
    return sorted(out, key=lambda c: (c[0] if c else "", len(c), c))


def representative_cycle(nodes, edges):
    cycles = representative_cycles(nodes, edges)
    return cycles[0] if cycles else None


def representative_cycles(nodes, edges):
    adjacency = _directed_adjacency(nodes, edges)
    sccs = strongly_connected_components(nodes, edges, include_singletons=False)
    cycles = []
    for comp in sccs:
        allowed = set(comp)
        cycle = _cycle_in_component(adjacency, allowed)
        if cycle:
            cycles.append(cycle)
    return sorted(cycles, key=lambda c: (c[0] if c else "", len(c), c))


def degree_summary(nodes, edges):
    summary = {
        n: {"in": 0, "out": 0, "total": 0}
        for n in sorted(str(n) for n in nodes)
    }
    for src, dst in _edge_pairs(edges):
        if src not in summary:
            summary[src] = {"in": 0, "out": 0, "total": 0}
        if dst not in summary:
            summary[dst] = {"in": 0, "out": 0, "total": 0}
        summary[src]["out"] += 1
        summary[src]["total"] += 1
        summary[dst]["in"] += 1
        summary[dst]["total"] += 1
    return [
        {"node": node, **counts}
        for node, counts in sorted(summary.items())
    ]


def metrics_summary(nodes, edges, components=None, sccs=None, top_k=5):
    pairs = list(_edge_pairs(edges))
    node_ids = set(str(n) for n in nodes)
    for src, dst in pairs:
        node_ids.add(src)
        node_ids.add(dst)
    node_ids = sorted(node_ids)
    components = components if components is not None else connected_components(node_ids, edges)
    sccs = sccs if sccs is not None else strongly_connected_components(node_ids, edges)
    degrees = degree_summary(node_ids, edges)
    cycle_rank = max(0, len(pairs) - len(node_ids) + len(components))

    def top(direction):
        return [
            {"node": item["node"], direction: item[direction]}
            for item in sorted(degrees, key=lambda d: (-d[direction], d["node"]))[:top_k]
        ]

    def max_nodes(direction):
        max_value = max((item[direction] for item in degrees), default=0)
        nodes_at_max = sorted(item["node"] for item in degrees if item[direction] == max_value)
        return {
            "value": max_value,
            "nodes": nodes_at_max[:top_k],
            "truncated": len(nodes_at_max) > top_k,
        }

    return {
        "node_count": len(node_ids),
        "edge_count": len(pairs),
        "component_count": len(components),
        "scc_count": len(sccs),
        "cycle_rank": cycle_rank,
        "max_fan_in": max_nodes("in"),
        "max_fan_out": max_nodes("out"),
        "top_fan_in": top("in"),
        "top_fan_out": top("out"),
    }


def _cycle_in_component(adjacency, allowed):
    for start in sorted(allowed):
        path = []
        positions = {}

        def dfs(cur):
            positions[cur] = len(path)
            path.append(cur)
            for nxt in adjacency.get(cur, []):
                if nxt not in allowed:
                    continue
                if nxt in positions:
                    return path[positions[nxt]:] + [nxt]
                found = dfs(nxt)
                if found:
                    return found
            path.pop()
            positions.pop(cur, None)
            return None

        found = dfs(start)
        if found:
            return found
    return None


def _directed_adjacency(nodes, edges):
    adjacency = {str(n): [] for n in nodes}
    for src, dst in _edge_pairs(edges):
        adjacency.setdefault(src, [])
        adjacency.setdefault(dst, [])
        adjacency[src].append(dst)
    return {node: sorted(set(nbrs)) for node, nbrs in adjacency.items()}


def _undirected_adjacency(nodes, edges):
    adjacency = {str(n): [] for n in nodes}
    for src, dst in _edge_pairs(edges):
        adjacency.setdefault(src, [])
        adjacency.setdefault(dst, [])
        adjacency[src].append(dst)
        adjacency[dst].append(src)
    return {node: sorted(set(nbrs)) for node, nbrs in adjacency.items()}


def _edge_pairs(edges):
    for edge in edges:
        if isinstance(edge, dict):
            src, dst = edge.get("from"), edge.get("to")
        else:
            src, dst = edge[0], edge[1]
        if src is None or dst is None:
            continue
        yield str(src), str(dst)
