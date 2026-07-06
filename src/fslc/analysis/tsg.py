# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed Semantic Graph projection for validated FSL specs."""
from __future__ import annotations

from .schema import TSG_SCHEMA_VERSION, edge, node, stable_unique
from ..model import display_label


ACTION_NODE_KINDS = {"action", "guard", "effect", "ensures"}
PROPERTY_NODE_KINDS = {"invariant", "trans", "leadsTo", "reachable"}
SCENARIO_NODE_KINDS = {"acceptance", "forbidden"}


def build_tsg(spec):
    """Build a deterministic JSON-friendly Typed Semantic Graph from `spec`."""
    builder = _TsgBuilder(spec)
    return builder.build()


def expr_reads(expr, state_names, bound=None):
    """Conservatively collect logical state variables read by an expression."""
    bound = set(bound or ())
    reads = set()

    def visit(e, local_bound):
        if not isinstance(e, tuple) or not e:
            return
        tag = e[0]
        if tag == "var":
            name = e[1]
            if name in state_names and name not in local_bound:
                reads.add(name)
            return
        if tag == "index":
            base = e[1]
            if isinstance(base, str):
                if base in state_names and base not in local_bound:
                    reads.add(base)
            else:
                visit(base, local_bound)
            visit(e[2], local_bound)
            return
        if tag in ("field", "field_lv", "old", "some", "not", "neg", "abs"):
            visit(e[1], local_bound)
            return
        if tag == "method":
            visit(e[1], local_bound)
            for arg in e[3]:
                visit(arg, local_bound)
            return
        if tag == "bin":
            visit(e[2], local_bound)
            visit(e[3], local_bound)
            return
        if tag == "ite":
            visit(e[1], local_bound)
            visit(e[2], local_bound)
            visit(e[3], local_bound)
            return
        if tag == "is":
            visit(e[1], local_bound)
            return
        if tag in ("set_lit", "seq_lit"):
            for item in e[1]:
                visit(item, local_bound)
            return
        if tag == "struct_lit":
            for value in e[2].values():
                visit(value, local_bound)
            return
        if tag in ("min", "max"):
            visit(e[1], local_bound)
            visit(e[2], local_bound)
            return
        if tag in ("count", "sum"):
            next_bound = set(local_bound)
            next_bound.add(e[1])
            if tag == "sum":
                visit(e[3], next_bound)
                if e[4] is not None:
                    visit(e[4], next_bound)
            else:
                visit(e[3], next_bound)
            return
        if tag in ("forall", "exists"):
            next_bound = set(local_bound)
            binder = e[1]
            if isinstance(binder, tuple) and len(binder) > 1:
                next_bound.add(binder[1])
                _visit_binder(binder, next_bound)
            visit(e[2], next_bound)
            return
        for part in e[1:]:
            if isinstance(part, tuple):
                visit(part, local_bound)
            elif isinstance(part, list):
                for item in part:
                    visit(item, local_bound)
            elif isinstance(part, dict):
                for item in part.values():
                    visit(item, local_bound)

    def _visit_binder(binder, local_bound):
        if not isinstance(binder, tuple):
            return
        if binder[0] == "binder_range":
            visit(binder[2], local_bound)
            visit(binder[3], local_bound)
        elif binder[0] == "binder_collection":
            visit(binder[2], local_bound)
            if len(binder) > 3 and binder[3] is not None:
                visit(binder[3], local_bound)
        elif len(binder) > 3 and binder[3] is not None:
            visit(binder[3], local_bound)

    visit(expr, bound)
    return reads


def stmt_writes(stmts):
    writes = []

    def visit(stmt):
        if not isinstance(stmt, tuple) or not stmt:
            return
        if stmt[0] == "assign":
            root = lvalue_root(stmt[1])
            if root:
                writes.append((root, stmt))
        elif stmt[0] == "if":
            for child in stmt[2]:
                visit(child)
            for child in stmt[3]:
                visit(child)
        elif stmt[0] == "forall_stmt":
            for child in stmt[2]:
                visit(child)

    for stmt in stmts:
        visit(stmt)
    return writes


def stmt_reads(stmts, state_names):
    reads = set()

    def visit(stmt):
        if not isinstance(stmt, tuple) or not stmt:
            return
        if stmt[0] == "assign":
            reads.update(expr_reads(stmt[2], state_names))
            reads.update(lvalue_reads(stmt[1], state_names))
        elif stmt[0] == "if":
            reads.update(expr_reads(stmt[1], state_names))
            for child in stmt[2]:
                visit(child)
            for child in stmt[3]:
                visit(child)
        elif stmt[0] == "forall_stmt":
            binder = stmt[1]
            if isinstance(binder, tuple):
                bound = {binder[1]} if len(binder) > 1 else set()
                if binder[0] == "binder_range":
                    reads.update(expr_reads(binder[2], state_names, bound))
                    reads.update(expr_reads(binder[3], state_names, bound))
                elif binder[0] == "binder_collection":
                    reads.update(expr_reads(binder[2], state_names, bound))
                    if len(binder) > 3 and binder[3] is not None:
                        reads.update(expr_reads(binder[3], state_names, bound))
            for child in stmt[2]:
                visit(child)

    for stmt in stmts:
        visit(stmt)
    return reads


def lvalue_root(lvalue):
    if not isinstance(lvalue, tuple) or not lvalue:
        return None
    tag = lvalue[0]
    if tag == "var":
        return lvalue[1]
    if tag == "index":
        return lvalue[1] if isinstance(lvalue[1], str) else lvalue_root(lvalue[1])
    if tag == "field_lv":
        return lvalue_root(lvalue[1])
    return None


def lvalue_reads(lvalue, state_names):
    reads = set()
    if not isinstance(lvalue, tuple) or not lvalue:
        return reads
    if lvalue[0] == "index":
        base = lvalue[1]
        if not isinstance(base, str):
            reads.update(expr_reads(base, state_names))
        reads.update(expr_reads(lvalue[2], state_names))
    elif lvalue[0] == "field_lv":
        reads.update(lvalue_reads(lvalue[1], state_names))
    return reads


def node_by_id(tsg):
    return {n["id"]: n for n in tsg.get("nodes", [])}


class _TsgBuilder:
    def __init__(self, spec):
        self.spec = spec
        self.state_names = set(spec.get("state") or {})
        self.nodes = []
        self.edges = []
        self.requirement_ids = set(spec.get("requirement_ids") or [])

    def build(self):
        self._collect_requirement_ids()
        self._add_spec()
        self._add_state()
        self._add_requirements()
        self._add_actions()
        self._add_properties()
        self._add_scenarios()
        self._add_kpis_and_controls()
        nodes = stable_unique(self.nodes, key=lambda n: n["id"])
        edges = stable_unique(self.edges, key=lambda e: e["id"])
        return {
            "analysis": "structure",
            "projection": "tsg",
            "schema_version": TSG_SCHEMA_VERSION,
            "nodes": nodes,
            "edges": edges,
        }

    def _collect_requirement_ids(self):
        for collection in (
            self.spec.get("actions") or [],
            self.spec.get("user_invariants") or [],
            self.spec.get("transitions") or [],
            self.spec.get("leadstos") or [],
            self.spec.get("reachables") or [],
        ):
            for item in collection if isinstance(collection, list) else []:
                meta = item.get("meta")
                if meta and meta.get("id"):
                    self.requirement_ids.add(meta["id"])
        for item in self.spec.get("kpis") or []:
            meta = item.get("meta")
            if meta and meta.get("id"):
                self.requirement_ids.add(meta["id"])

    def _add_spec(self):
        spec_id = f"spec:{self.spec['name']}"
        self.nodes.append(node(spec_id, "spec", self.spec["name"], self.spec["name"]))

    def _add_state(self):
        for name in sorted(self.spec.get("state") or {}):
            self._add_declared_node(node(
                f"state:{name}",
                "state",
                name,
                display_label(name, self.spec),
                type=_jsonable_type(self.spec["state"][name]),
            ))
        for entry in sorted(self.spec.get("phys_vars") or [], key=lambda e: e.get("phys", "")):
            phys = entry.get("phys")
            logical = entry.get("logical")
            self._add_declared_node(node(
                f"phys_state:{phys}",
                "phys_state",
                phys,
                display_label(phys, self.spec),
                logical=logical,
            ))
            if logical:
                self.edges.append(edge(
                    f"state:{logical}",
                    "expands_to",
                    f"phys_state:{phys}",
                ))

    def _add_requirements(self):
        for req_id in sorted(self.requirement_ids):
            self._add_declared_node(node(f"requirement:{req_id}", "requirement", req_id, req_id))

    def _add_actions(self):
        for action in sorted(self.spec.get("actions") or [], key=lambda a: a["name"]):
            action_id = f"action:{action['name']}"
            action_node = node(
                action_id,
                "action",
                action["name"],
                display_label(action["name"], self.spec),
                action.get("loc"),
                action.get("meta"),
                fair=bool(action.get("fair")),
                sync=bool(action.get("sync")),
                generated=action.get("generated"),
            )
            self._add_declared_node(action_node)
            self._cover_meta(action.get("meta"), action_id)

            action_reads = set()
            for idx, req in enumerate(action.get("requires") or []):
                guard_id = f"guard:{action['name']}:{idx}"
                self.nodes.append(node(
                    guard_id,
                    "guard",
                    f"{action['name']}:{idx}",
                    f"{display_label(action['name'], self.spec)} requires {idx}",
                    req.get("loc"),
                    expr=req.get("expr"),
                    action=action_id,
                ))
                self.edges.append(edge(action_id, "has_guard", guard_id))
                reads = expr_reads(req.get("expr"), self.state_names)
                action_reads.update(reads)
                self._add_read_edges(guard_id, reads)

            writes = stmt_writes(action.get("stmts") or [])
            stmt_read_names = stmt_reads(action.get("stmts") or [], self.state_names)
            action_reads.update(stmt_read_names)
            for idx, (root, stmt) in enumerate(writes):
                effect_id = f"effect:{action['name']}:{idx}"
                self.nodes.append(node(
                    effect_id,
                    "effect",
                    f"{action['name']}:{idx}",
                    f"{display_label(action['name'], self.spec)} effect {idx}",
                    stmt[3] if len(stmt) > 3 else None,
                    expr=stmt[2],
                    action=action_id,
                    target=root,
                ))
                self.edges.append(edge(action_id, "has_effect", effect_id))
                self._add_write_edges(action_id, root)
                self._add_write_edges(effect_id, root)
                self._add_read_edges(effect_id, expr_reads(stmt[2], self.state_names))

            for idx, ens in enumerate(action.get("ensures") or []):
                ensures_id = f"ensures:{action['name']}:{idx}"
                self.nodes.append(node(
                    ensures_id,
                    "ensures",
                    f"{action['name']}:{idx}",
                    f"{display_label(action['name'], self.spec)} ensures {idx}",
                    ens.get("loc"),
                    expr=ens.get("expr"),
                    action=action_id,
                ))
                self.edges.append(edge(action_id, "has_ensures", ensures_id))
                reads = expr_reads(ens.get("expr"), self.state_names)
                action_reads.update(reads)
                self._add_read_edges(ensures_id, reads)

            self._add_read_edges(action_id, action_reads)

    def _add_properties(self):
        self._add_property_collection("invariant", self.spec.get("user_invariants") or [])
        self._add_property_collection("trans", self.spec.get("transitions") or [])
        self._add_property_collection("reachable", self.spec.get("reachables") or [])
        for item in sorted(self.spec.get("leadstos") or [], key=lambda p: p["name"]):
            node_id = f"leadsTo:{item['name']}"
            self._add_declared_node(node(
                node_id,
                "leadsTo",
                item["name"],
                display_label(item["name"], self.spec),
                item.get("loc"),
                item.get("meta"),
                within=item.get("within"),
                decreases=item.get("decreases"),
                P=item.get("P"),
                Q=item.get("Q"),
            ))
            self._cover_meta(item.get("meta"), node_id)
            reads = expr_reads(item.get("P"), self.state_names)
            reads.update(expr_reads(item.get("Q"), self.state_names))
            if item.get("decreases") is not None:
                reads.update(expr_reads(item.get("decreases"), self.state_names))
            self._add_read_edges(node_id, reads)
            self._add_check_edges(node_id, reads)

    def _add_property_collection(self, kind, items):
        for item in sorted(items, key=lambda p: p["name"]):
            node_id = f"{kind}:{item['name']}"
            self._add_declared_node(node(
                node_id,
                kind,
                item["name"],
                display_label(item["name"], self.spec),
                item.get("loc"),
                item.get("meta"),
                expr=item.get("expr"),
            ))
            self._cover_meta(item.get("meta"), node_id)
            reads = expr_reads(item.get("expr"), self.state_names)
            self._add_read_edges(node_id, reads)
            self._add_check_edges(node_id, reads)

    def _add_scenarios(self):
        for item in sorted(self.spec.get("acceptance") or [], key=lambda s: s["id"]):
            node_id = f"acceptance:{item['id']}"
            self._add_declared_node(node(
                node_id,
                "acceptance",
                item["id"],
                item.get("text") or item["id"],
                item.get("loc"),
                text=item.get("text"),
            ))
            self._cover_id(item["id"], node_id)
            self._add_scenario_steps(node_id, item)
            reads = expr_reads(item.get("expect"), self.state_names)
            self._add_read_edges(node_id, reads)
            self._add_check_edges(node_id, reads)
        for item in sorted(self.spec.get("forbidden") or [], key=lambda s: s["id"]):
            node_id = f"forbidden:{item['id']}"
            self._add_declared_node(node(
                node_id,
                "forbidden",
                item["id"],
                item.get("text") or item["id"],
                item.get("loc"),
                text=item.get("text"),
            ))
            self._cover_id(item["id"], node_id)
            self._add_scenario_steps(node_id, item)

    def _add_scenario_steps(self, scenario_id, item):
        for idx, step in enumerate(item.get("steps") or []):
            if not isinstance(step, tuple) or len(step) < 2:
                continue
            action_name = step[1]
            action_id = f"action:{action_name}"
            self.edges.append(edge(
                scenario_id,
                "precedes" if idx else "starts_with",
                action_id,
                edge_id=f"edge:{scenario_id}:step:{idx}:{action_id}",
                step=idx,
            ))
            for arg in step[2] if len(step) > 2 else []:
                self._add_read_edges(scenario_id, expr_reads(arg, self.state_names))

    def _add_kpis_and_controls(self):
        for kpi in sorted(self.spec.get("kpis") or [], key=lambda k: k.get("name", "")):
            name = kpi.get("name")
            if not name:
                continue
            self._add_declared_node(node(f"kpi:{name}", "kpi", name, name, meta=kpi.get("meta")))
            self._cover_meta(kpi.get("meta"), f"kpi:{name}")
        for control in sorted(self.spec.get("controls") or [], key=lambda c: c.get("id", "")):
            cid = control.get("id")
            if not cid:
                continue
            self._add_declared_node(node(
                f"control:{cid}",
                "control",
                cid,
                control.get("text") or cid,
                control.get("loc"),
                text=control.get("text"),
            ))

    def _add_declared_node(self, n):
        self.nodes.append(n)
        self.edges.append(edge(f"spec:{self.spec['name']}", "declares", n["id"]))

    def _cover_meta(self, meta, target_id):
        if meta and meta.get("id"):
            self._cover_id(meta["id"], target_id)

    def _cover_id(self, req_id, target_id):
        if req_id in self.requirement_ids:
            self.edges.append(edge(f"requirement:{req_id}", "covers", target_id))

    def _add_read_edges(self, source_id, reads):
        for name in sorted(reads):
            self.edges.append(edge(source_id, "reads", f"state:{name}"))

    def _add_check_edges(self, source_id, reads):
        for name in sorted(reads):
            self.edges.append(edge(source_id, "checks", f"state:{name}"))

    def _add_write_edges(self, source_id, root):
        if root in self.state_names:
            self.edges.append(edge(source_id, "writes", f"state:{root}"))


def _jsonable_type(value):
    if isinstance(value, tuple):
        return [_jsonable_type(v) for v in value]
    if isinstance(value, list):
        return [_jsonable_type(v) for v in value]
    if isinstance(value, dict):
        return {k: _jsonable_type(v) for k, v in sorted(value.items()) if k != "ty"}
    return value
