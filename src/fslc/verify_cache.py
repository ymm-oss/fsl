# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Persistent verdict cache for `fslc verify` (issue #169).

fslc sits inside an LLM write->verify->repair loop: an agent re-runs `verify`
on identical or near-identical input many times per session. This module is
the cache layer that lets an unchanged re-run skip the Z3 solve entirely.

**Non-negotiable framing**: the verdict is the product. A cache that ever
serves a stale verdict is a soundness regression, strictly worse than no
cache. Every function here is fail-closed -- ``cli.run_verify`` wraps every
call into this module in a broad ``try/except`` so any bug, corrupt file, or
unexpected input degrades to an ordinary uncached run rather than raising or
(worse) serving a wrong result. See ``docs/DESIGN-incremental-verify.md`` for
the full design and its soundness argument.

Scope (v1 / stage 1 of that design): a whole-verdict cache keyed on every
input that can affect ``run_verify``'s output, plus cross-depth reuse of a
``violated`` result (a counterexample's earliest step does not depend on the
requested search bound). Property-level differential re-verification is
explicitly deferred (stage 2/3 in the design doc).
"""
from __future__ import annotations

import hashlib
import json
import os
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

import lark
import z3

CACHE_SCHEMA_VERSION = 1

# Only verdict-class results are cacheable. Errors/internal failures are
# never stored -- they are not the expensive, deterministic-given-the-same-
# input computation this cache exists to short-circuit.
STORABLE_RESULTS = frozenset({"verified", "proved", "violated", "reachable_failed", "unknown_cti"})

_MAX_ENTRY_BYTES = 5 * 1024 * 1024
_DEFAULT_MAX_MB = 256
_EVICT_PROBABILITY = 1 / 32
_MAX_AGE_SECONDS = 30 * 24 * 3600


class CacheKeyError(Exception):
    """Raised when a value cannot be canonically encoded -- the run becomes
    uncacheable rather than hashing an under-specified representation."""


# ---------------------------------------------------------------------------
# on/off switches and storage location
# ---------------------------------------------------------------------------
def enabled() -> bool:
    return os.environ.get("FSLC_CACHE", "").strip().lower() != "off"


def cache_root() -> Path:
    if os.environ.get("FSLC_CACHE_DIR"):
        return Path(os.environ["FSLC_CACHE_DIR"])
    if os.environ.get("XDG_CACHE_HOME"):
        return Path(os.environ["XDG_CACHE_HOME"]) / "fslc"
    return Path.home() / ".cache" / "fslc"


def _entry_path(key: str) -> Path:
    return cache_root() / "verify" / "v1" / key[:2] / f"{key}.json"


def _xdepth_path(xdepth_key: str) -> Path:
    return cache_root() / "verify" / "v1" / "xdepth" / f"{xdepth_key}.json"


# ---------------------------------------------------------------------------
# canonical encoding + cache key
# ---------------------------------------------------------------------------
def _canon(value: Any) -> Any:
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, tuple):
        return {"$tuple": [_canon(v) for v in value]}
    if isinstance(value, list):
        return {"$list": [_canon(v) for v in value]}
    if isinstance(value, dict):
        return {"$dict": [[str(k), _canon(v)] for k, v in sorted(value.items(), key=lambda kv: str(kv[0]))]}
    if isinstance(value, (set, frozenset)):
        return {"$set": sorted(_canon(v) for v in value)}
    raise CacheKeyError(f"unhashable value in cache key: {type(value)!r}")


def canonical_hash(value: Any) -> str:
    canon = _canon(value)
    blob = json.dumps(canon, sort_keys=True, ensure_ascii=True, separators=(",", ":"))
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()


_fingerprint_cache: Optional[str] = None


def implementation_fingerprint() -> str:
    """fslc version + z3/lark/Python versions + a sha256 over every installed
    fslc/*.py file's (relative path, bytes). The file-content digest is what
    protects an editable install / dev worktree, where the version string
    does not move when bmc.py does. Computed once per process."""
    global _fingerprint_cache
    if _fingerprint_cache is not None:
        return _fingerprint_cache
    import fslc  # local import: avoid any import-cycle risk at module load

    pkg_dir = Path(fslc.__file__).resolve().parent
    digest = hashlib.sha256()
    for path in sorted(pkg_dir.rglob("*.py")):
        digest.update(path.relative_to(pkg_dir).as_posix().encode("utf-8"))
        digest.update(path.read_bytes())
    parts = {
        "fslc_version": getattr(fslc, "__version__", "unknown"),
        "z3_version": z3.get_version_string(),
        "lark_version": lark.__version__,
        "python_version": list(sys.version_info[:2]),
        "package_sha256": digest.hexdigest(),
    }
    _fingerprint_cache = canonical_hash(parts)
    return _fingerprint_cache


def compute_key(
    *,
    ast: Any,
    display_names: Any,
    src: str,
    engine: str,
    depth: int,
    k_ind: int,
    deadlock_mode: str,
    vacuity_mode: str,
    property_name: Optional[str],
    exclude_property_names: Optional[list],
    strict_tags: bool,
    requirements_sha256: Optional[str],
    instances: Optional[list],
    values: Optional[list],
) -> tuple:
    """Returns ``(full_key, depth_agnostic_key)``. Raises ``CacheKeyError`` if
    any component (most likely the kernel AST) contains a value the canonical
    encoder does not recognize -- callers must treat that as "uncacheable",
    never as "cacheable with a partial key"."""
    base = {
        "schema": CACHE_SCHEMA_VERSION,
        "fingerprint": implementation_fingerprint(),
        "ast": ast,
        "display_names": display_names,
        "src_sha256": hashlib.sha256(src.encode("utf-8")).hexdigest(),
        "engine": engine,
        "k_ind": k_ind,
        "deadlock_mode": deadlock_mode,
        "vacuity_mode": vacuity_mode,
        "property_name": property_name,
        "exclude_property_names": sorted(exclude_property_names or []),
        "strict_tags": strict_tags,
        "requirements_sha256": requirements_sha256,
        "instances": sorted(instances or []),
        "values": sorted(values or []),
    }
    xdepth_key = canonical_hash(base)
    full = dict(base)
    full["depth"] = depth
    full_key = canonical_hash(full)
    return full_key, xdepth_key


# ---------------------------------------------------------------------------
# storage
# ---------------------------------------------------------------------------
def _read_json(path: Path) -> Optional[dict]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def _write_json_atomic(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(dir=str(path.parent), prefix=".tmp-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            json.dump(data, fh)
        os.replace(tmp_name, str(path))
    except Exception:
        try:
            os.unlink(tmp_name)
        except OSError:
            pass
        raise


def lookup(key: str, xdepth_key: str, depth: int) -> Optional[tuple]:
    """Returns ``(result_dict, source)`` with ``source`` in
    ``"exact"``/``"cross_depth"``, or ``None`` on a miss (including a
    corrupt/unreadable entry, which is deleted so it doesn't keep missing
    forever without ever being cleaned up)."""
    entry_path = _entry_path(key)
    entry = _read_json(entry_path)
    if entry is not None and isinstance(entry.get("result"), dict):
        try:
            os.utime(entry_path, None)
        except OSError:
            pass
        return entry["result"], "exact"
    if entry_path.exists():
        try:
            entry_path.unlink()
        except OSError:
            pass

    pointer = _read_json(_xdepth_path(xdepth_key))
    if not isinstance(pointer, dict):
        return None
    target_key = pointer.get("entry_key")
    violated_at_step = pointer.get("violated_at_step")
    if not isinstance(target_key, str) or not isinstance(violated_at_step, int):
        return None
    if violated_at_step > depth:
        return None
    target = _read_json(_entry_path(target_key))
    if target is not None and isinstance(target.get("result"), dict):
        return target["result"], "cross_depth"
    return None


def store(key: str, xdepth_key: str, out: dict) -> None:
    result = out.get("result")
    if result not in STORABLE_RESULTS:
        return
    import fslc  # local import: avoid any import-cycle risk at module load

    payload = {
        "schema": CACHE_SCHEMA_VERSION,
        "created": datetime.now(timezone.utc).isoformat(),
        "fslc": getattr(fslc, "__version__", "unknown"),
        "result": out,
    }
    blob = json.dumps(payload)
    if len(blob.encode("utf-8")) > _MAX_ENTRY_BYTES:
        return
    _write_json_atomic(_entry_path(key), payload)
    if result == "violated" and isinstance(out.get("violated_at_step"), int):
        pointer = {"entry_key": key, "violated_at_step": out["violated_at_step"]}
        _write_json_atomic(_xdepth_path(xdepth_key), pointer)
    _maybe_evict()


# ---------------------------------------------------------------------------
# eviction
# ---------------------------------------------------------------------------
def _maybe_evict() -> None:
    import random

    if random.random() > _EVICT_PROBABILITY:
        return
    root = cache_root() / "verify" / "v1"
    if not root.is_dir():
        return
    now = datetime.now(timezone.utc).timestamp()
    max_bytes = int(os.environ.get("FSLC_CACHE_MAX_MB", str(_DEFAULT_MAX_MB))) * 1024 * 1024
    entries = []
    total = 0
    for path in root.rglob("*.json"):
        try:
            st = path.stat()
        except OSError:
            continue
        if now - st.st_mtime > _MAX_AGE_SECONDS:
            try:
                path.unlink()
            except OSError:
                pass
            continue
        entries.append((st.st_mtime, st.st_size, path))
        total += st.st_size
    if total <= max_bytes:
        return
    for _mtime, size, path in sorted(entries):
        if total <= max_bytes:
            break
        try:
            path.unlink()
            total -= size
        except OSError:
            pass
