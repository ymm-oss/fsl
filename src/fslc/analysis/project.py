# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Project-level structural traceability analysis."""
from __future__ import annotations

from pathlib import Path

try:
    import tomllib
except ImportError:  # pragma: no cover - Python 3.9/3.10
    import tomli as tomllib

from .graph import connected_components, degree_summary, metrics_summary, representative_cycles, strongly_connected_components
from .schema import GRAPH_SCHEMA_VERSION, edge, node, stable_unique
from .tsg import build_tsg, expr_reads, node_by_id
from ..model import build_spec
from ..parser import parse_refinement, parse_src


LAYER_ORDER = ("business", "requirements", "design")
SPEC_KIND_BY_LAYER = {
    "business": "business_spec",
    "requirements": "requirements_spec",
    "design": "design_spec",
}


def analyze_project_manifest(path, projection="traceability_graph"):
    if projection != "traceability_graph":
        raise ValueError(f"unsupported project projection: {projection}")
    manifest_path = Path(path)
    manifest, base = _load_manifest(manifest_path)
    builder = _ProjectTraceabilityBuilder(manifest_path, manifest, base)
    return builder.build()


def _load_manifest(path):
    with open(path, "rb") as fh:
        data = tomllib.load(fh)
    if not isinstance(data, dict):
        raise ValueError("project manifest must be a TOML table")
    return data, path.parent


def _rel(path, base):
    p = Path(path)
    if not p.is_absolute():
        p = base / p
    return p


class _ProjectTraceabilityBuilder:
    def __init__(self, manifest_path, manifest, base):
        self.manifest_path = manifest_path
        self.manifest = manifest
        self.base = base
        self.nodes = []
        self.edges = []
        self.layer_data = {}
        self.findings = []

    def build(self):
        self.nodes.append(node(
            f"file:{_display_path(self.manifest_path)}",
            "file",
            _display_path(self.manifest_path),
            _display_path(self.manifest_path),
            path=_display_path(self.manifest_path),
        ))
        for layer in LAYER_ORDER:
            cfg = self.manifest.get(layer)
            if not isinstance(cfg, dict) or not cfg.get("file"):
                continue
            self._add_layer(layer, cfg)
        for layer in LAYER_ORDER:
            cfg = self.manifest.get(layer)
            if not isinstance(cfg, dict) or not cfg.get("refine_against"):
                continue
            target = cfg.get("refine_against")
            if target in self.layer_data:
                self._add_refinement(layer, target, cfg)
        self._add_same_id_anchors()
        self._add_traceability_gap_findings()

        nodes = stable_unique(self.nodes, key=lambda n: n["id"])
        edges = stable_unique(self.edges, key=lambda e: e["id"])
        node_ids = [n["id"] for n in nodes]
        components = connected_components(node_ids, edges)
        sccs = strongly_connected_components(node_ids, edges)
        return {
            "analysis": "structure",
            "projection": "traceability_graph",
            "schema_version": GRAPH_SCHEMA_VERSION,
            "formal_status": "not_a_violation",
            "manifest": _display_path(self.manifest_path),
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
            "findings": self.findings,
        }

    def _add_layer(self, layer, cfg):
        path = _rel(cfg["file"], self.base)
        ast, display_names = parse_src(path.read_text(encoding="utf-8"), str(path.parent))
        spec = build_spec(ast, display_names, semantic_check=False)
        tsg = build_tsg(spec)
        file_id = f"file:{layer}:{_display_path(path)}"
        self.nodes.append(node(file_id, "file", _display_path(path), _display_path(path), path=_display_path(path)))
        data = {
            "path": path,
            "spec": spec,
            "tsg": tsg,
            "nodes_by_id": node_by_id(tsg),
            "covers": {},
        }
        for n in tsg["nodes"]:
            prefixed = self._prefix_node(layer, n)
            self.nodes.append(prefixed)
            if n["kind"] == "spec":
                self.edges.append(edge(file_id, "declares", prefixed["id"]))
        for e in tsg["edges"]:
            self.edges.append(_prefixed_edge(layer, e))
            if e["kind"] == "covers" and e["from"].startswith("requirement:"):
                data["covers"].setdefault(e["to"], set()).add(e["from"])
        self.layer_data[layer] = data

    def _prefix_node(self, layer, n):
        out = dict(n)
        out["id"] = _prefix(layer, n["id"])
        out["layer"] = layer
        if n["kind"] == "spec":
            out["kind"] = SPEC_KIND_BY_LAYER.get(layer, "spec")
        return out

    def _add_refinement(self, layer, target, cfg):
        mapping = cfg.get("mapping")
        if not mapping:
            return
        path = _rel(mapping, self.base)
        ast = parse_refinement(path.read_text(encoding="utf-8"))
        refinement_id = f"refinement:{layer}->{target}:{ast[1]}"
        file_id = f"file:{layer}->{target}:{_display_path(path)}"
        self.nodes.append(node(file_id, "file", _display_path(path), _display_path(path), path=_display_path(path)))
        self.nodes.append(node(refinement_id, "refinement", ast[1], ast[1], path=_display_path(path)))
        self.edges.append(edge(file_id, "declares", refinement_id))
        self.edges.append(edge(refinement_id, "implements", _prefix(layer, f"spec:{self.layer_data[layer]['spec']['name']}")))
        self.edges.append(edge(refinement_id, "abstracts", _prefix(target, f"spec:{self.layer_data[target]['spec']['name']}")))

        impl_state_names = set(self.layer_data[layer]["spec"].get("state") or {})
        for item in ast[2]:
            tag = item[0]
            if tag == "map":
                _, logical, binder, expr, loc = item
                map_id = f"state_map:{layer}->{target}:{logical}"
                self.nodes.append(node(map_id, "state_map", logical, logical, loc, layer=layer, target_layer=target))
                self.edges.append(edge(refinement_id, "declares", map_id))
                self.edges.append(edge(map_id, "maps_state", _prefix(target, f"state:{logical}")))
                bound = {binder[1]} if binder is not None else set()
                for read_name in sorted(expr_reads(expr, impl_state_names, bound)):
                    self.edges.append(edge(map_id, "reads_impl_state", _prefix(layer, f"state:{read_name}")))
            elif tag == "action_map":
                _, impl_action, _params, target_action, loc = item
                map_id = f"action_map:{layer}->{target}:{impl_action}"
                self.nodes.append(node(map_id, "action_map", impl_action, impl_action, loc, layer=layer, target_layer=target))
                self.edges.append(edge(refinement_id, "declares", map_id))
                impl_id = _prefix(layer, f"action:{impl_action}")
                self.edges.append(edge(map_id, "maps_action", impl_id))
                if target_action[0] == "stutter":
                    stutter_id = f"stutter_map:{layer}->{target}:{impl_action}"
                    self.nodes.append(node(stutter_id, "stutter_map", impl_action, impl_action, loc))
                    self.edges.append(edge(map_id, "stutters", stutter_id))
                    continue
                abs_action = target_action[1]
                abs_id = _prefix(target, f"action:{abs_action}")
                self.edges.append(edge(impl_id, "maps_action", abs_id))
                self._add_requirement_lower_anchors(target, layer, f"action:{abs_action}", impl_id)
            elif tag == "preserve_progress":
                progress_id = f"preserve_progress:{layer}->{target}:{ast[1]}"
                self.nodes.append(node(progress_id, "preserve_progress", "preserve progress", "preserve progress", item[2]))
                self.edges.append(edge(refinement_id, "preserves_progress", progress_id))

    def _add_requirement_lower_anchors(self, upper_layer, lower_layer, upper_target_id, lower_id):
        covers = self.layer_data[upper_layer]["covers"]
        for req_id in sorted(covers.get(upper_target_id, set())):
            self.edges.append(edge(
                _prefix(upper_layer, req_id),
                "lower_anchor",
                lower_id,
                formal_status="not_a_violation",
                via="refinement_action_map",
                layer=lower_layer,
            ))

    def _add_same_id_anchors(self):
        pairs = (("business", "requirements"), ("requirements", "design"))
        for upper, lower in pairs:
            if upper not in self.layer_data or lower not in self.layer_data:
                continue
            upper_nodes = self.layer_data[upper]["nodes_by_id"]
            lower_nodes = self.layer_data[lower]["nodes_by_id"]
            for node_id, upper_node in sorted(upper_nodes.items()):
                if upper_node.get("kind") not in {"requirement", "control"}:
                    continue
                lower_node = lower_nodes.get(node_id)
                if lower_node is None:
                    continue
                self.edges.append(edge(
                    _prefix(upper, node_id),
                    "lower_anchor",
                    _prefix(lower, node_id),
                    formal_status="not_a_violation",
                    via="same_id",
                ))

    def _add_traceability_gap_findings(self):
        lower_anchor_sources = {
            e["from"]
            for e in self.edges
            if e.get("kind") == "lower_anchor"
        }
        counters = {}
        for layer in ("business", "requirements"):
            data = self.layer_data.get(layer)
            if not data:
                continue
            for n in sorted(data["tsg"]["nodes"], key=lambda item: item["id"]):
                if n["kind"] not in {"requirement", "control"}:
                    continue
                node_id = _prefix(layer, n["id"])
                if node_id in lower_anchor_sources:
                    continue
                ftype = "traceability_gap"
                counters[ftype] = counters.get(ftype, 0) + 1
                self.findings.append({
                    "finding_id": f"STRUCT-TRACEABILITY-GAP-{counters[ftype]:04d}",
                    "analysis": "structure",
                    "finding_type": ftype,
                    "severity": "review_required",
                    "confidence": 0.74,
                    "formal_status": "not_a_violation",
                    "involved_nodes": [node_id],
                    "witness": {
                        "kind": "missing_lower_anchor",
                        "layer": layer,
                        "node": node_id,
                    },
                    "why_it_matters": (
                        "An upper-layer requirement/control ID has no visible lower-layer "
                        "structural anchor in the project traceability graph."
                    ),
                    "candidate_repairs": [
                        {
                            "kind": "add_lower_anchor",
                            "template": (
                                "Carry the ID into the lower layer, or map an abstract "
                                "action/property to a lower-layer action through refinement."
                            ),
                        }
                    ],
                    "do_not_assume": [
                        "The lower layer violates the upper-layer contract.",
                        "Name similarity proves semantic coverage.",
                    ],
                })


def _prefix(layer, node_id):
    return f"{layer}:{node_id}"


def _prefixed_edge(layer, e):
    return {
        **e,
        "id": f"edge:{layer}:{e['id']}",
        "from": _prefix(layer, e["from"]),
        "to": _prefix(layer, e["to"]),
        "layer": layer,
    }


def _display_path(path):
    try:
        return path.resolve().relative_to(Path.cwd().resolve()).as_posix()
    except ValueError:
        return path.as_posix()
