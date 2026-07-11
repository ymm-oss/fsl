# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Approval records bound to normalized specification digests (issue #190)."""
from __future__ import annotations

import hashlib
import json
import re
import tempfile
from datetime import datetime, timezone
from pathlib import Path

from .model import FslError, build_spec
from .parser import parse_src
from .semantic_diff import semantic_diff
from .verify_cache import canonical_hash


SCHEMA_VERSION = "fslc-approval.v0"
_FROM_PATH = re.compile(r'\bfrom\s+"([^"]+)"')


def _without_locations(value):
    if isinstance(value, dict):
        if set(value) <= {"line", "column"} and value:
            return None
        return {key: _without_locations(item) for key, item in sorted(value.items())}
    if isinstance(value, tuple):
        return tuple(_without_locations(item) for item in value)
    if isinstance(value, list):
        return [_without_locations(item) for item in value]
    return value


def normalized_spec_digest(path):
    path = Path(path)
    source = path.read_text(encoding="utf-8")
    ast, display_names = parse_src(source, str(path.parent))
    spec = build_spec(ast, display_names)
    digest = canonical_hash({
        "ast": _without_locations(ast),
        "display_names": display_names,
    })
    return digest, spec, ast, display_names


def _source_snapshot(entry):
    root = Path(entry).resolve()
    base = root.parent
    pending = [root]
    seen = set()
    files = {}
    while pending:
        path = pending.pop()
        if path in seen:
            continue
        seen.add(path)
        try:
            relative = path.relative_to(base).as_posix()
        except ValueError as exc:
            raise FslError(
                f"approval source dependency is outside the spec directory: {path}",
                kind="io",
            ) from exc
        source = path.read_text(encoding="utf-8")
        files[relative] = source
        for imported in _FROM_PATH.findall(source):
            dependency = (path.parent / imported).resolve()
            if not dependency.is_file():
                raise FslError(f"approval source dependency not found: {dependency}", kind="io")
            pending.append(dependency)
    return root.name, {name: files[name] for name in sorted(files)}


def _sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def create_approval_record(spec_path, rendered_path, rendering_kind, approver,
                           approved_at=None, command=None):
    if rendering_kind not in {"html", "ledger", "scenarios"}:
        raise FslError("approval rendering kind must be html, ledger, or scenarios", kind="semantics")
    digest, spec, _ast, _display_names = normalized_spec_digest(spec_path)
    entry, files = _source_snapshot(spec_path)
    rendered = Path(rendered_path).read_bytes()
    timestamp = approved_at or datetime.now(timezone.utc).isoformat()
    requirements = sorted(set(spec.get("requirement_ids") or []) | {
        item["meta"]["id"]
        for key in ("actions", "user_invariants", "transitions", "leadstos", "reachables")
        for item in spec.get(key) or []
        if item.get("meta") and item["meta"].get("id")
    })
    return {
        "schema": SCHEMA_VERSION,
        "approved_at": timestamp,
        "approver": approver,
        "spec": {
            "name": spec["name"],
            "entry": entry,
            "digest": digest,
            "digest_algorithm": "sha256_canonical_kernel_ast_without_locations",
            "requirements": requirements,
        },
        "rendering": {
            "kind": rendering_kind,
            "path": str(rendered_path),
            "sha256": _sha256_bytes(rendered),
            "generation": {
                "spec_entry": entry,
                "command": command,
            },
        },
        "baseline": {
            "digest": digest,
            "entry": entry,
            "files": files,
        },
    }


def load_approval_record(path):
    try:
        record = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise FslError(f"cannot read approval record: {exc}", kind="io") from exc
    if record.get("schema") != SCHEMA_VERSION:
        raise FslError(f"unsupported approval record schema: {record.get('schema')}", kind="semantics")
    if not isinstance(record.get("baseline", {}).get("files"), dict):
        raise FslError("approval record has no baseline source snapshot", kind="semantics")
    return record


def check_approval_record(spec_path, record_path, rendered_path=None):
    record = load_approval_record(record_path)
    current_digest, spec, _ast, _display_names = normalized_spec_digest(spec_path)
    approved_digest = record["spec"]["digest"]
    spec_status = "approved" if current_digest == approved_digest else "drifted"
    rendering_status = "not_checked"
    current_rendering_sha256 = None
    if rendered_path is not None:
        current_rendering_sha256 = _sha256_bytes(Path(rendered_path).read_bytes())
        rendering_status = (
            "approved"
            if current_rendering_sha256 == record["rendering"]["sha256"]
            else "drifted"
        )
    status = "approved" if spec_status == "approved" and rendering_status != "drifted" else "drifted"
    requirement_ids = sorted(set(record["spec"].get("requirements") or []) | set(spec.get("requirement_ids") or []))
    return {
        "result": "approval_checked",
        "status": status,
        "spec_status": spec_status,
        "rendering_status": rendering_status,
        "approved_digest": approved_digest,
        "current_digest": current_digest,
        "approved_at": record["approved_at"],
        "approver": record["approver"],
        "record": str(record_path),
        "rendering": {
            "approved_sha256": record["rendering"]["sha256"],
            "current_sha256": current_rendering_sha256,
        },
        "requirements": [
            {"id": req_id, "status": status, "approved_digest": approved_digest}
            for req_id in requirement_ids
        ],
        "semantic_diff": {
            "command": f"fslc diff --approval {record_path} {spec_path}",
            "baseline_digest": approved_digest,
        },
    }


def semantic_diff_approval(record_path, current_path, depth=8, mapping_path=None, forbid=None):
    record = load_approval_record(record_path)
    with tempfile.TemporaryDirectory(prefix="fslc-approved-") as tmp:
        root = Path(tmp)
        for relative, source in record["baseline"]["files"].items():
            target = (root / relative).resolve()
            if root.resolve() not in target.parents:
                raise FslError("approval record contains an unsafe source path", kind="io")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source, encoding="utf-8")
        old_path = root / record["baseline"]["entry"]
        embedded_digest, _spec, _ast, _display_names = normalized_spec_digest(old_path)
        if embedded_digest != record["spec"]["digest"]:
            raise FslError(
                "approval record baseline source does not match its approved digest",
                kind="semantics",
            )
        result = semantic_diff(old_path, current_path, depth, mapping_path, forbid)
    result["old"]["file"] = f"approval:{record['spec']['digest']}:{record['baseline']['entry']}"
    result["approval"] = {
        "record": str(record_path),
        "baseline_digest": record["spec"]["digest"],
        "approved_at": record["approved_at"],
        "approver": record["approver"],
        "materialization": "embedded_source_snapshot",
    }
    return result
