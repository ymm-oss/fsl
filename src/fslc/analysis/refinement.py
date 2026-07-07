# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Structural graph projection for standalone refinement mapping files."""
from __future__ import annotations

from .graph import connected_components, degree_summary, representative_cycles, strongly_connected_components
from .schema import GRAPH_SCHEMA_VERSION, edge, node, stable_unique


def analyze_refinement_ast(ast, projection="refinement_graph"):
    """Build a deterministic review graph from a ``refinement`` AST.

    This intentionally does not call ``build_refinement``: the projection is a
    mapping-structure view and should not require loading the referenced impl/abs
    specs unless a future type-aware projection needs them.
    """
    if ast[0] != "refinement":
        raise ValueError("expected refinement AST")
    if projection not in ("tsg", "refinement_graph"):
        raise ValueError(f"unsupported refinement projection: {projection}")

    _, name, items = ast
    builder = _RefinementGraphBuilder(name, items)
    return builder.build()


class _RefinementGraphBuilder:
    def __init__(self, name, items):
        self.name = name
        self.items = items
        self.nodes = []
        self.edges = []
        self.ref_id = f"refinement:{name}"
        self.impl_name = None
        self.abs_name = None

    def build(self):
        self.nodes.append(node(self.ref_id, "refinement", self.name, self.name))
        for item in self.items:
            tag = item[0]
            if tag == "impl":
                self.impl_name = item[1]
                impl_id = f"impl_spec:{item[1]}"
                self.nodes.append(node(impl_id, "impl_spec", item[1], item[1]))
                self.edges.append(edge(self.ref_id, "implements", impl_id))
            elif tag == "abs":
                self.abs_name = item[1]
                abs_id = f"abs_spec:{item[1]}"
                self.nodes.append(node(abs_id, "abs_spec", item[1], item[1]))
                self.edges.append(edge(self.ref_id, "abstracts", abs_id))
            elif tag == "maps_auto":
                auto_id = f"maps_auto:{self.name}"
                self.nodes.append(node(auto_id, "maps_auto", "maps auto", "maps auto"))
                self.edges.append(edge(self.ref_id, "declares", auto_id))
            elif tag == "map":
                self._add_state_map(item)
            elif tag == "action_map":
                self._add_action_map(item)
            elif tag == "preserve_progress":
                self._add_progress(item)

        nodes = stable_unique(self.nodes, key=lambda n: n["id"])
        edges = stable_unique(self.edges, key=lambda e: e["id"])
        node_ids = [n["id"] for n in nodes]
        return {
            "analysis": "structure",
            "projection": "refinement_graph",
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

    def _add_state_map(self, item):
        _, logical, binder, expr, loc = item
        map_id = f"state_map:{logical}"
        if binder is not None:
            map_id = f"{map_id}:{binder[1]}"
        self.nodes.append(node(
            map_id,
            "state_map",
            logical,
            logical,
            loc,
            binder=_binder_name(binder),
            expr=expr,
        ))
        self.edges.append(edge(self.ref_id, "declares", map_id))
        abs_state = f"abs_state:{logical}"
        self.nodes.append(node(abs_state, "abs_state", logical, logical))
        self.edges.append(edge(map_id, "maps_state", abs_state))
        bound = {_binder_name(binder)} if binder is not None else set()
        for read_name in sorted(_expr_vars(expr) - bound):
            read_id = f"map_expr_read:{read_name}"
            self.nodes.append(node(read_id, "map_expr_read", read_name, read_name))
            self.edges.append(edge(map_id, "reads_impl_state", read_id))

    def _add_action_map(self, item):
        _, impl_action, params, target, loc = item
        map_id = f"action_map:{impl_action}"
        self.nodes.append(node(
            map_id,
            "action_map",
            impl_action,
            impl_action,
            loc,
            params=[p[1] if isinstance(p, tuple) else p for p in params],
        ))
        self.edges.append(edge(self.ref_id, "declares", map_id))
        impl_id = f"impl_action:{impl_action}"
        self.nodes.append(node(impl_id, "impl_action", impl_action, impl_action))
        self.edges.append(edge(map_id, "maps_action", impl_id))
        param_names = {p[1] if isinstance(p, tuple) else p for p in params}
        if target[0] == "stutter":
            stutter_id = f"stutter_map:{impl_action}"
            self.nodes.append(node(stutter_id, "stutter_map", impl_action, impl_action, loc))
            self.edges.append(edge(map_id, "stutters", stutter_id))
            return
        _, abs_action, args = target
        abs_id = f"abs_action:{abs_action}"
        self.nodes.append(node(abs_id, "abs_action", abs_action, abs_action))
        self.edges.append(edge(map_id, "abstracts", abs_id))
        for read_name in sorted(set().union(*(_expr_vars(arg) for arg in args)) - param_names):
            read_id = f"map_expr_read:{read_name}"
            self.nodes.append(node(read_id, "map_expr_read", read_name, read_name))
            self.edges.append(edge(map_id, "reads_impl_state", read_id))

    def _add_progress(self, item):
        _, progress_items, loc = item
        progress_id = f"preserve_progress:{self.name}"
        self.nodes.append(node(progress_id, "preserve_progress", "preserve progress", "preserve progress", loc))
        self.edges.append(edge(self.ref_id, "preserves_progress", progress_id))
        for progress in progress_items:
            if progress[0] != "progress_respond":
                continue
            _, leadsto, actions, item_loc = progress
            response_id = f"progress_response:{leadsto}"
            self.nodes.append(node(response_id, "progress_response", leadsto, leadsto, item_loc))
            self.edges.append(edge(progress_id, "responds_by", response_id))
            abs_lt = f"abs_leadsTo:{leadsto}"
            self.nodes.append(node(abs_lt, "abs_leadsTo", leadsto, leadsto))
            self.edges.append(edge(response_id, "preserves_progress", abs_lt))
            for action_name in sorted(actions):
                impl_action = f"impl_action:{action_name}"
                self.nodes.append(node(impl_action, "impl_action", action_name, action_name))
                self.edges.append(edge(response_id, "responds_by", impl_action))


def _binder_name(binder):
    if not binder:
        return None
    return binder[1] if len(binder) > 1 else None


def _expr_vars(expr):
    names = set()

    def visit(e):
        if not isinstance(e, tuple) or not e:
            return
        tag = e[0]
        if tag == "var":
            names.add(e[1])
            return
        if tag in ("forall", "exists"):
            before = set(names)
            binder = e[1]
            visit(e[2])
            bound = _binder_name(binder)
            if bound is not None and bound not in before:
                names.discard(bound)
            return
        for part in e[1:]:
            if isinstance(part, tuple):
                visit(part)
            elif isinstance(part, list):
                for item in part:
                    visit(item)
            elif isinstance(part, dict):
                for item in part.values():
                    visit(item)

    visit(expr)
    return names
