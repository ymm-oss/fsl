# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Metadata convention for intentionally undecided specification behavior."""
from __future__ import annotations


PREFIX = "undecided:"


def undecided_meta(meta):
    """Return normalized undecided metadata, or ``None`` for an ordinary tag."""
    if not isinstance(meta, dict):
        return None
    ident = str(meta.get("id") or "").strip()
    text = str(meta.get("text") or "").strip()
    if ident.lower() == "undecided":
        return {"text": text, "requirements": []}
    if text.lower().startswith(PREFIX):
        return {
            "text": text[len(PREFIX):].strip(),
            "requirements": [ident] if ident else [],
        }
    return None


def undecided_declarations(spec):
    """List intentional-undecision markers from user-visible declarations."""
    entries = []
    for kind, key in (
        ("action", "actions"),
        ("invariant", "user_invariants"),
        ("trans", "transitions"),
        ("leadsTo", "leadstos"),
        ("reachable", "reachables"),
    ):
        for item in spec.get(key) or []:
            if item.get("generated"):
                continue
            marker = undecided_meta(item.get("meta"))
            if marker is None:
                continue
            entries.append({
                "kind": kind,
                "name": item["name"],
                "node_id": f"{kind}:{item['name']}",
                "text": marker["text"],
                "requirements": marker["requirements"],
                "loc": item.get("loc"),
                "verification_semantics": "metadata_only",
            })
    return sorted(entries, key=lambda entry: (entry["kind"], entry["name"]))
