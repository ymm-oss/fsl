"""Corpus differential snapshot — a behavior-preservation safety net for refactors.

This test pins the *verdict-level* output of ``fslc check`` / ``fslc verify``
across the whole ``.fsl`` corpus to a golden JSON file.  Any behavior-changing
refactor of the evaluator surfaces immediately as a snapshot diff.

We deliberately snapshot only the **stable verdict tier** (result, violation
kind/step, declaration name, invariant/transition counts, reachable
pass/fail, action coverage, warning kinds) and NOT the concrete counterexample
trace: which specific witness Z3 returns can depend on constraint ordering, so
a behavior-preserving refactor that reorders constraints could perturb the
trace without changing semantics.  Fine-grained state semantics are pinned
separately by ``test_evaluator_agreement.py`` (bmc vs Monitor, step by step).

Regenerate after an *intended* behavior change with::

    FSLC_SNAPSHOT_UPDATE=1 .venv/bin/python -m pytest tests/test_corpus_snapshot.py -q
"""
from __future__ import annotations

import json
import os
import shlex
from pathlib import Path

import pytest

from fslc.cli import run_check, run_verify
from fslc.model import FslError

ROOT = Path(__file__).resolve().parents[1]
SPECS = ROOT / "specs"
EXAMPLES = ROOT / "examples"
GALLERY = EXAMPLES / "gallery"
SNAPSHOT = Path(__file__).resolve().parent / "snapshots" / "corpus_snapshot.json"

UPDATE = os.environ.get("FSLC_SNAPSHOT_UPDATE") == "1"


# --------------------------------------------------------------------------
# corpus enumeration
# --------------------------------------------------------------------------
def _all_specs() -> list[Path]:
    return sorted({*SPECS.glob("*.fsl"), *EXAMPLES.rglob("*.fsl")})


def _rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def _declared_verify(path: Path) -> tuple[int, str] | None:
    """Return (depth, deadlock) if the file declares a `verify` command."""
    command = None
    for line in path.read_text(encoding="utf-8").splitlines()[:16]:
        stripped = line.strip()
        if stripped.startswith("// expected-command:"):
            command = stripped.split(":", 1)[1].strip()
    if not command:
        return None
    parts = shlex.split(command)
    if not parts or parts[0] != "verify":
        return None
    depth, deadlock = 4, "warn"
    for i, part in enumerate(parts):
        if part == "--depth":
            depth = int(parts[i + 1])
        elif part == "--deadlock":
            deadlock = parts[i + 1]
    return depth, deadlock


def _verify_plan(path: Path) -> tuple[int, str] | None:
    """(depth, deadlock) to verify this spec under, or None to skip verify."""
    if path.parent == SPECS:
        if "refines" in path.stem:
            return None
        return 5, "warn"
    if path.parent in (GALLERY / "valid", GALLERY / "errors", GALLERY / "adversarial"):
        return _declared_verify(path)
    return None


# --------------------------------------------------------------------------
# stable verdict projection
# --------------------------------------------------------------------------
def _warn_kinds(out: dict) -> list[str]:
    return sorted((w.get("kind") or "none") for w in out.get("warnings", []))


def _project_check(out: dict) -> dict:
    proj = {"result": out.get("result")}
    if out.get("result") == "ok":
        proj["warnings"] = _warn_kinds(out)
    else:
        proj["kind"] = out.get("kind")
    if "implements" in out:
        impl = out["implements"]
        proj["implements"] = impl.get("result") if isinstance(impl, dict) else impl
    return proj


def _project_verify(out: dict) -> dict:
    res = out.get("result")
    proj: dict = {"result": res}
    if res in ("verified", "proved"):
        proj["invariants_checked"] = out.get("invariants_checked")
        proj["transitions_checked"] = out.get("transitions_checked")
        dl = out.get("deadlock")
        proj["deadlock_found"] = bool(dl.get("found")) if isinstance(dl, dict) else None
        reach = out.get("reachables") or {}
        proj["reachables"] = {
            name: (info.get("witnessed_at_step") if isinstance(info, dict) else info)
            for name, info in sorted(reach.items())
        }
        cov = out.get("action_coverage") or {}
        proj["action_coverage"] = {k: cov[k] for k in sorted(cov)}
        proj["warnings"] = _warn_kinds(out)
    elif res == "violated":
        proj["violation_kind"] = out.get("violation_kind")
        proj["violated_at_step"] = out.get("violated_at_step")
        proj["name"] = (
            out.get("invariant")
            or out.get("leadsTo")
            or out.get("trans")
            or out.get("name")
        )
        la = out.get("last_action")
        proj["last_action"] = la.get("name") if isinstance(la, dict) else la
    elif res == "reachable_failed":
        reach = out.get("reachables") or {}
        proj["unreachable"] = sorted(
            name
            for name, info in reach.items()
            if isinstance(info, dict) and not info.get("witnessed_at_step")
        )
    else:  # error / unknown
        proj["kind"] = out.get("kind")
    return proj


def _live_snapshot() -> dict:
    snap: dict = {}
    for path in _all_specs():
        rid = _rel(path)
        entry: dict = {}
        try:
            entry["check"] = _project_check(run_check(str(path)))
        except FslError as exc:
            entry["check"] = {"result": "error", "kind": exc.kind}
        plan = _verify_plan(path)
        if plan is not None and entry["check"].get("result") == "ok":
            depth, deadlock = plan
            try:
                entry["verify"] = _project_verify(
                    run_verify(str(path), depth, deadlock_mode=deadlock)
                )
            except FslError as exc:
                entry["verify"] = {"result": "error", "kind": exc.kind}
        snap[rid] = entry
    return snap


_SNAPSHOT_CACHE: dict | None = None


def live_snapshot() -> dict:
    global _SNAPSHOT_CACHE
    if _SNAPSHOT_CACHE is None:
        _SNAPSHOT_CACHE = _live_snapshot()
    return _SNAPSHOT_CACHE


def _golden() -> dict:
    if not SNAPSHOT.exists():
        return {}
    return json.loads(SNAPSHOT.read_text(encoding="utf-8"))


# --------------------------------------------------------------------------
# update mode + comparison
# --------------------------------------------------------------------------
@pytest.mark.skipif(not UPDATE, reason="set FSLC_SNAPSHOT_UPDATE=1 to regenerate")
def test_regenerate_snapshot():
    SNAPSHOT.parent.mkdir(parents=True, exist_ok=True)
    SNAPSHOT.write_text(
        json.dumps(live_snapshot(), indent=2, sort_keys=True, ensure_ascii=True) + "\n",
        encoding="utf-8",
    )


@pytest.mark.skipif(UPDATE, reason="regenerating snapshot")
def test_snapshot_has_no_missing_or_extra_specs():
    golden = _golden()
    assert golden, "snapshot missing — run with FSLC_SNAPSHOT_UPDATE=1 first"
    live = live_snapshot()
    assert set(live) == set(golden), {
        "added": sorted(set(live) - set(golden)),
        "removed": sorted(set(golden) - set(live)),
    }


@pytest.mark.skipif(UPDATE, reason="regenerating snapshot")
@pytest.mark.parametrize("rid", sorted(_golden().keys()) or [pytest.param("__missing__", marks=pytest.mark.skip)])
def test_corpus_verdict_matches_snapshot(rid):
    expected = _golden()[rid]
    actual = live_snapshot().get(rid)
    assert actual == expected, {"spec": rid, "expected": expected, "actual": actual}
