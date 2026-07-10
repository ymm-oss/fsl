# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Dialect corpus conformance harness — the CI gate for issue #167.

Every ``.fsl`` under ``specs/`` and ``examples/`` is either driven through the
full dual-evaluator safety net (``parse -> desugar -> build_spec -> Monitor
load -> BMC/Monitor expression agreement -> verify-vs-oracle verdict
agreement``) or excluded **loudly**, with a documented reason
(``tests/dialect_registry.py``) that this file re-asserts on every run. A new
dialect that nobody registers here is a CI failure, not a silent skip — see
``docs/DESIGN-conformance-harness.md`` for the full design and the gap this
closes (the 2026-07-08 fsl-db audit: 15/18 ``examples/db/*.fsl`` silently sat
outside this net while ``pytest -q`` stayed green).

No ``pytest.skip`` anywhere in this file: every non-conformance file is a
*classified* parametrized case whose classification is itself asserted.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import pytest

from fslc.ai_parser import is_ai_agent_source, is_ai_component_source
from fslc.ai_project import is_ai_project_source
from fslc.cli import run_verify
from fslc.parser import parse_src
from fslc.runtime import Monitor

from agreement import assert_expr_agreement
from dialect_registry import DIALECTS, EVIDENCE_CONSTRUCTS, MONITOR_EXCLUSIONS, SCAN_ROOTS
from oracle import ROOT, VerifyCase, assert_verdict_agrees, bfs_oracle, can_monitor

EXPR_STATES = 40

EXCLUDED = "EXCLUDED"
REFINEMENT = "REFINEMENT"
DECLARED_ERROR = "DECLARED_ERROR"
INJECTED = "INJECTED"
CONFORMANCE = "CONFORMANCE"
UNKNOWN = "UNKNOWN"

_CONSTRUCT_RE = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\b")
_KEYWORD_TO_DIALECT = {d.construct: key for key, d in DIALECTS.items()}


@dataclass(frozen=True)
class Classified:
    path: Path
    cls: str
    dialect: Optional[str] = None
    reason: Optional[str] = None

    @property
    def id(self) -> str:
        rel = self.path.relative_to(ROOT).as_posix()
        return f"{rel}:{self.cls}" + (f":{self.reason}" if self.reason else "")


def _front_matter(path: Path) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()[:16]
    return [ln.strip() for ln in lines if ln.strip().startswith("//")]


def _declared_error(front: list[str]) -> bool:
    return any(ln.startswith("// expected-result:") and "error" in ln for ln in front)


def _injected(front: list[str]) -> bool:
    return any(ln.startswith("// inject:") or ln.startswith("// expect-detector:") for ln in front)


def _top_construct(src: str) -> Optional[str]:
    for line in src.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        m = _CONSTRUCT_RE.match(stripped)
        return m.group(1) if m else None
    return None


def classify(path: Path) -> Classified:
    src = path.read_text(encoding="utf-8")
    rel = path.relative_to(ROOT).as_posix()

    if rel in MONITOR_EXCLUSIONS:
        return Classified(path, EXCLUDED, reason=rel)
    if is_ai_project_source(src):
        return Classified(path, EXCLUDED, reason="ai-project")
    if is_ai_agent_source(src):
        return Classified(path, EXCLUDED, reason="ai-agent")

    construct = _top_construct(src)
    if construct == "refinement":
        return Classified(path, REFINEMENT)

    front = _front_matter(path)
    if _declared_error(front):
        return Classified(path, DECLARED_ERROR)
    if _injected(front):
        dialect = _KEYWORD_TO_DIALECT.get(construct)
        return Classified(path, INJECTED, dialect=dialect)

    dialect = _KEYWORD_TO_DIALECT.get(construct)
    # is_ai_component_source uses a bare startswith("ai_component") check, which
    # is exactly _KEYWORD_TO_DIALECT's "ai_component" -> "ai" lookup too, kept
    # explicit here so a change to either check can't silently diverge.
    if construct == "ai_component":
        assert is_ai_component_source(src), path
    if dialect is None:
        return Classified(path, UNKNOWN, reason=construct)
    return Classified(path, CONFORMANCE, dialect=dialect)


def _corpus() -> list[Path]:
    paths: set[Path] = set()
    for root in SCAN_ROOTS:
        paths.update((ROOT / root).rglob("*.fsl"))
    return sorted(paths)


ALL = [classify(p) for p in _corpus()]
EXCLUDED_CASES = [c for c in ALL if c.cls == EXCLUDED]
REFINEMENT_CASES = [c for c in ALL if c.cls == REFINEMENT]
DECLARED_ERROR_CASES = [c for c in ALL if c.cls == DECLARED_ERROR]
FULL_PIPELINE_CASES = [c for c in ALL if c.cls in (CONFORMANCE, INJECTED)]


def _run_full_pipeline(c: Classified) -> None:
    depth = DIALECTS[c.dialect].depth
    rel = c.path.relative_to(ROOT).as_posix()

    # stage 1: load
    mon = Monitor(c.path)
    mon.reset()
    mon.enabled()

    # stage 2: explore (feeds stages 3 and 4) — any raise fails, including
    # UnsupportedOracle: a conformance-class file must be BFS-explorable.
    oracle = bfs_oracle(c.path, depth, collect_phys=EXPR_STATES)

    # stage 3: expression agreement
    assert_expr_agreement(oracle.phys_snapshots, mon.spec, label=rel)

    # stage 4: verdict agreement
    result = run_verify(str(c.path), depth, deadlock_mode="warn")
    allow = frozenset({"acceptance", "forbidden"}) if c.cls == INJECTED else frozenset()
    assert_verdict_agrees(VerifyCase(path=c.path, depth=depth), oracle, result, allow_error_kinds=allow)


@pytest.mark.parametrize("case", FULL_PIPELINE_CASES, ids=lambda c: c.id)
def test_full_pipeline(case: Classified):
    _run_full_pipeline(case)


@pytest.mark.parametrize("case", REFINEMENT_CASES, ids=lambda c: c.id)
def test_refinement_mapping_parses(case: Classified):
    src = case.path.read_text(encoding="utf-8")
    ast, _display_names = parse_src(src, str(case.path.parent))
    assert ast[0] == "refinement", (case.id, ast[0])


def _declared_verify_flags(front: list[str]) -> dict:
    """Best-effort parse of the ``// expected-command: verify ...`` flags that
    affect whether the declared error actually fires (e.g. ``--vacuity
    error``) — a DECLARED_ERROR fixture must be run the way it declares, not
    with generic defaults, or a real vacuity/deadlock-only error looks stale."""
    flags = {"depth": 4, "deadlock_mode": "warn", "vacuity_mode": "warn"}
    command = next((ln.split(":", 1)[1].strip() for ln in front
                     if ln.startswith("// expected-command:")), "")
    parts = command.split()
    for i, part in enumerate(parts):
        if part == "--depth" and i + 1 < len(parts):
            flags["depth"] = int(parts[i + 1])
        elif part == "--deadlock" and i + 1 < len(parts):
            flags["deadlock_mode"] = parts[i + 1]
        elif part == "--vacuity" and i + 1 < len(parts):
            flags["vacuity_mode"] = parts[i + 1]
    return flags


@pytest.mark.parametrize("case", DECLARED_ERROR_CASES, ids=lambda c: c.id)
def test_declared_error_still_errors(case: Classified):
    flags = _declared_verify_flags(_front_matter(case.path))
    try:
        result = run_verify(str(case.path), **flags)
    except Exception:  # noqa: BLE001 -- a load-time failure also satisfies "errors somewhere"
        return
    assert result.get("result") == "error", (
        f"{case.id}: declared '// expected-result: error' but fslc now accepts it — "
        "the declaration is stale; update or remove the fixture"
    )


@pytest.mark.parametrize("case", EXCLUDED_CASES, ids=lambda c: c.id)
def test_exclusion_still_holds(case: Classified):
    src = case.path.read_text(encoding="utf-8")
    if case.reason == "ai-project":
        assert is_ai_project_source(src), (case.id, "no longer an ai-project source — remove the exclusion")
        return
    if case.reason == "ai-agent":
        assert is_ai_agent_source(src), (case.id, "no longer an ai-agent source — remove the exclusion")
        return
    ok, _reason = can_monitor(case.path)
    assert not ok, (
        f"{case.id}: Monitor can now load this file — the exclusion in "
        "tests/dialect_registry.py is stale and must be removed"
    )


def test_corpus_fully_claimed():
    unknown = [c for c in ALL if c.cls == UNKNOWN]
    assert not unknown, [
        f"{c.path.relative_to(ROOT).as_posix()}: top-level construct "
        f"'{c.reason}' is not registered — add it to tests/dialect_registry.py "
        "(DIALECTS or EVIDENCE_CONSTRUCTS)"
        for c in unknown
    ]


def test_registry_floors():
    counts: dict[str, int] = {}
    for c in ALL:
        if c.dialect:
            counts[c.dialect] = counts.get(c.dialect, 0) + 1
    shortfalls = {
        key: (counts.get(key, 0), d.min_files)
        for key, d in DIALECTS.items()
        if counts.get(key, 0) < d.min_files
    }
    assert not shortfalls, shortfalls

    for rel in MONITOR_EXCLUSIONS:
        assert (ROOT / rel).exists(), f"MONITOR_EXCLUSIONS entry {rel} no longer exists on disk"


def test_registry_covers_ai_evidence_constructs():
    # EVIDENCE_CONSTRUCTS is documentation of *why* ai-project/ai-agent files
    # are excluded; make sure the corpus still actually contains at least one
    # of each so the exclusion reasons stay exercised, not just declared.
    assert set(EVIDENCE_CONSTRUCTS) == {"ai-project", "ai-agent"}
    reasons = {c.reason for c in EXCLUDED_CASES}
    for construct in EVIDENCE_CONSTRUCTS:
        assert construct in reasons, f"no corpus file currently exercises the '{construct}' exclusion"
