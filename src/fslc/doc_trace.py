# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Canonical Markdown requirement extraction and spec-tag trace checks."""
from __future__ import annotations

import hashlib
import re
from pathlib import Path

from .model import FslError


_REQ_HEADING = re.compile(r"^##\s+([A-Z][A-Z0-9_-]*-\d+):\s*(.+?)\s*$")
_ANY_HEADING = re.compile(r"^#{1,6}\s+")


def _canonical_text(title, body_lines):
    parts = [title.strip()]
    parts.extend(line.strip() for line in body_lines if line.strip())
    return " ".join(" ".join(parts).split())


def parse_canonical_markdown(source):
    """Extract ``REQ-ID -> canonical text`` from level-two normative sections."""
    requirements = {}
    current = None
    title = None
    body = []

    def finish():
        if current is None:
            return
        if current in requirements:
            raise FslError(f"duplicate canonical requirement heading: {current}", kind="semantics")
        requirements[current] = _canonical_text(title, body)

    for line in source.splitlines():
        match = _REQ_HEADING.match(line)
        if match:
            finish()
            current, title = match.group(1), match.group(2)
            body = []
            continue
        if _ANY_HEADING.match(line):
            finish()
            current = title = None
            body = []
            continue
        if current is not None:
            body.append(line)
    finish()
    return requirements


def _formal_requirements(spec):
    refs = {}

    def add(req_id, text, element):
        if not req_id:
            return
        refs.setdefault(req_id, []).append({"text": text, "element": element})

    for kind, key in (
        ("action", "actions"),
        ("invariant", "user_invariants"),
        ("trans", "transitions"),
        ("leadsTo", "leadstos"),
        ("reachable", "reachables"),
    ):
        for item in spec.get(key) or []:
            meta = item.get("meta") or {}
            add(meta.get("id"), meta.get("text"), f"{kind}:{item['name']}")
    for kind, key in (("acceptance", "acceptance"), ("forbidden", "forbidden")):
        for item in spec.get(key) or []:
            add(item.get("id"), item.get("text"), f"{kind}:{item.get('id')}")
    for req_id in spec.get("requirement_ids") or []:
        refs.setdefault(req_id, [])
    return refs


def source_tag_path(spec):
    meta = spec.get("kind")
    if isinstance(meta, dict) and str(meta.get("id") or "").strip().lower() == "source":
        return str(meta.get("text") or "").strip() or None
    return None


def resolve_docs_path(spec_path, spec, explicit=None):
    if explicit:
        return Path(explicit)
    declared = source_tag_path(spec)
    if declared:
        return Path(spec_path).parent / declared
    return None


def check_doc_trace(spec_path, spec, docs_path=None):
    """Return ``(warnings, resolved_path, bytes_sha256)``; no doc means no-op."""
    path = resolve_docs_path(spec_path, spec, docs_path)
    if path is None:
        return [], None, None
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise FslError(f"cannot read canonical docs file '{path}': {exc}", kind="io") from exc
    try:
        source = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise FslError(f"canonical docs file must be UTF-8: {path}", kind="io") from exc
    canonical = parse_canonical_markdown(source)
    formal = _formal_requirements(spec)
    warnings = []
    for req_id in sorted(set(canonical) - set(formal)):
        warnings.append({
            "kind": "missing_formalization",
            "requirement": {"id": req_id, "text": canonical[req_id]},
            "document": str(path),
            "message": f"canonical requirement {req_id} has no formal tag or scenario",
        })
    for req_id in sorted(set(formal) - set(canonical)):
        warnings.append({
            "kind": "ghost_requirement",
            "requirement": {"id": req_id},
            "document": str(path),
            "elements": sorted(item["element"] for item in formal[req_id]),
            "message": f"formal requirement {req_id} is absent from the canonical document",
        })
    for req_id in sorted(set(canonical) & set(formal)):
        expected = canonical[req_id]
        for item in formal[req_id]:
            if item["text"] is None or " ".join(str(item["text"]).split()) == expected:
                continue
            warnings.append({
                "kind": "stale_tag",
                "requirement": {"id": req_id},
                "element": item["element"],
                "document": str(path),
                "old_text": item["text"],
                "new_text": expected,
                "message": f"formal tag text for {req_id} differs from the canonical document",
            })
    return warnings, path, hashlib.sha256(raw).hexdigest()
