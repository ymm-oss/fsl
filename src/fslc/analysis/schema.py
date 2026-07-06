# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Stable schema constants and constructors for structural analysis."""
from __future__ import annotations


TSG_SCHEMA_VERSION = "tsg.v0"
GRAPH_SCHEMA_VERSION = "analysis-graph.v0"
FINDINGS_SCHEMA_VERSION = "analysis-findings.v0"


def node(node_id, kind, name=None, label=None, loc=None, meta=None, **extra):
    out = {"id": node_id, "kind": kind}
    if name is not None:
        out["name"] = name
    if label is not None:
        out["label"] = label
    elif name is not None:
        out["label"] = name
    if loc:
        out["loc"] = loc
    if meta:
        out["meta"] = meta
    for key, value in extra.items():
        if value is not None:
            out[key] = value
    return out


def edge(from_id, kind, to_id, edge_id=None, **extra):
    out = {
        "id": edge_id or f"edge:{from_id}:{kind}:{to_id}",
        "kind": kind,
        "from": from_id,
        "to": to_id,
    }
    for key, value in extra.items():
        if value is not None:
            out[key] = value
    return out


def stable_unique(items, key):
    seen = set()
    out = []
    for item in sorted(items, key=key):
        marker = key(item)
        if marker in seen:
            continue
        seen.add(marker)
        out.append(item)
    return out
