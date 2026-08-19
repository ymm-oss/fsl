# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Dialect corpus conformance harness for issue #167.

Every ``.fsl`` under ``specs/`` and ``examples/`` is either driven through the
full dual-evaluator safety net (``parse -> desugar -> build_spec -> Monitor
load -> BMC/Monitor expression agreement -> verify-vs-oracle verdict
agreement``) or excluded **loudly**, with a documented reason
(``tests/dialect_registry.py``) that this file re-asserts on every run. A new
dialect that nobody registers here is a failure of this file, not a silent
skip — see ``docs/DESIGN-conformance-harness.md`` for the full design and the
gap this closes (the 2026-07-08 fsl-db audit: 15/18 ``examples/db/*.fsl``
silently sat outside this net while ``pytest -q`` stayed green).

This file is itself a manual/reference check: no CI workflow and no
``tools/check-native-integration.sh`` lane currently runs it (see the design
doc's "Cost and CI wiring"), so it is not a CI gate — run it yourself when
touching a dialect.

No ``pytest.skip`` anywhere in this file: every non-conformance file is a
*classified* parametrized case whose classification is itself asserted.
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import pytest
from lark.exceptions import UnexpectedInput

from fslc.ai_parser import is_ai_agent_source, is_ai_component_source
from fslc.ai_project import is_ai_project_source
from fslc.cli import run_verify
from fslc.dialect_registry import dialect_keyword
from fslc.parser import parse_src
from fslc.runtime import Monitor

from agreement import assert_expr_agreement
from dialect_registry import (
    DIALECTS,
    EVIDENCE_CONSTRUCTS,
    MONITOR_EXCLUSIONS,
    NATIVE_ONLY_REFINEMENT_SYNTAX,
    SCAN_ROOTS,
    is_causal_source,
)
from oracle import ROOT, VerifyCase, assert_verdict_agrees, bfs_oracle, can_monitor

EXPR_STATES = 40

EXCLUDED = "EXCLUDED"
NATIVE_ONLY_REFINEMENT = "NATIVE_ONLY_REFINEMENT"
REFINEMENT = "REFINEMENT"
DECLARED_ERROR = "DECLARED_ERROR"
INJECTED = "INJECTED"
CONFORMANCE = "CONFORMANCE"
UNKNOWN = "UNKNOWN"

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


def _classify_source(path: Path, src: str, rel: str) -> Classified:
    if rel in MONITOR_EXCLUSIONS:
        return Classified(path, EXCLUDED, reason=rel)
    if is_ai_project_source(src):
        return Classified(path, EXCLUDED, reason="ai-project")
    if is_ai_agent_source(src):
        return Classified(path, EXCLUDED, reason="ai-agent")
    if is_causal_source(src):
        return Classified(path, EXCLUDED, reason="causal")

    native_only = NATIVE_ONLY_REFINEMENT_SYNTAX.get(rel)
    if native_only is not None:
        assert native_only.path == rel, (
            f"native-only refinement entry key {rel!r} disagrees with its "
            f"declared path {native_only.path!r}"
        )
        return Classified(path, NATIVE_ONLY_REFINEMENT, reason=rel)

    construct = dialect_keyword(src)
    if construct == "refinement":
        return Classified(path, REFINEMENT)

    front = _front_matter(path)
    if _declared_error(front):
        return Classified(path, DECLARED_ERROR)
    if _injected(front):
        dialect = _KEYWORD_TO_DIALECT.get(construct)
        return Classified(path, INJECTED, dialect=dialect)

    dialect = _KEYWORD_TO_DIALECT.get(construct)
    # Keep the compatibility predicate coupled to the shared registry result.
    if construct == "ai_component":
        assert is_ai_component_source(src), path
    if dialect is None:
        return Classified(path, UNKNOWN, reason=construct)
    return Classified(path, CONFORMANCE, dialect=dialect)


def classify(path: Path) -> Classified:
    src = path.read_text(encoding="utf-8")
    rel = path.relative_to(ROOT).as_posix()
    return _classify_source(path, src, rel)


def _corpus() -> list[Path]:
    paths: set[Path] = set()
    for root in SCAN_ROOTS:
        paths.update((ROOT / root).rglob("*.fsl"))
    return sorted(paths)


ALL = [classify(p) for p in _corpus()]
EXCLUDED_CASES = [c for c in ALL if c.cls == EXCLUDED]
NATIVE_ONLY_REFINEMENT_CASES = [c for c in ALL if c.cls == NATIVE_ONLY_REFINEMENT]
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


def _assert_native_only_refinement_failure(case: Classified) -> None:
    entry = NATIVE_ONLY_REFINEMENT_SYNTAX[case.reason]
    label = case.reason or str(case.path)
    source = case.path.read_text(encoding="utf-8")
    lines = source.splitlines()
    assert entry.path == case.reason
    assert entry.construct
    assert entry.design_citation
    assert entry.reason
    assert entry.native_owner
    assert entry.line <= len(lines), (
        f"{label}: registered {entry.construct!r} line {entry.line} is beyond EOF"
    )
    assert lines[entry.line - 1][entry.column - 1:].startswith(entry.construct), (
        f"{label}: registered construct {entry.construct!r} is absent at "
        f"{entry.line}:{entry.column}; remove or update the exact-path exclusion"
    )

    # Location is load-bearing: any parse failure elsewhere is a regression,
    # not evidence that this native-only construct still justifies exclusion.
    try:
        parse_src(source, str(case.path.parent))
    except UnexpectedInput as error:
        actual = (error.line, error.column)
        expected = (entry.line, entry.column)
        assert actual == expected, (
            f"{label}: parser failure moved from native-only {entry.construct!r} "
            f"at {expected[0]}:{expected[1]} to {actual[0]}:{actual[1]}"
        )
    else:
        pytest.fail(
            f"{label}: frozen parser now accepts native-only {entry.construct!r}; "
            "remove the stale exclusion"
        )


@pytest.mark.parametrize("case", NATIVE_ONLY_REFINEMENT_CASES, ids=lambda c: c.id)
def test_native_only_refinement_syntax_fails_at_registered_construct(case: Classified):
    _assert_native_only_refinement_failure(case)


def test_native_only_refinement_exclusion_preserves_ordinary_mapping_parse():
    ordinary = next(
        case for case in REFINEMENT_CASES
        if case.path.relative_to(ROOT).as_posix() not in NATIVE_ONLY_REFINEMENT_SYNTAX
    )
    source = ordinary.path.read_text(encoding="utf-8")
    ast, _display_names = parse_src(source, str(ordinary.path.parent))
    assert ordinary.cls == REFINEMENT
    assert ast[0] == "refinement"


def test_native_only_refinement_wrong_location_is_a_regression(tmp_path: Path):
    registered = NATIVE_ONLY_REFINEMENT_CASES[0]
    source = registered.path.read_text(encoding="utf-8")
    broken = source.replace("  impl ReturnImpl", "  invalid ReturnImpl", 1)
    path = tmp_path / "wrong_location.fsl"
    path.write_text(broken, encoding="utf-8")
    case = Classified(path, NATIVE_ONLY_REFINEMENT, reason=registered.reason)

    with pytest.raises(AssertionError, match="parser failure moved"):
        _assert_native_only_refinement_failure(case)


def test_native_only_refinement_construct_is_not_excluded_by_shape(tmp_path: Path):
    registered = NATIVE_ONLY_REFINEMENT_CASES[0]
    source = registered.path.read_text(encoding="utf-8")
    path = tmp_path / "unregistered_native_only.fsl"
    rel = "examples/layers/unregistered_native_only.fsl"
    path.write_text(source, encoding="utf-8")

    assert rel not in NATIVE_ONLY_REFINEMENT_SYNTAX
    assert _classify_source(path, source, rel).cls == REFINEMENT
    with pytest.raises(UnexpectedInput):
        parse_src(source, str(path.parent))


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
    if case.reason == "causal":
        assert is_causal_source(src), (case.id, "no longer a causal source — remove the exclusion")
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
    # EVIDENCE_CONSTRUCTS is documentation of *why* ai-project/ai-agent/causal
    # files are excluded; make sure the corpus still actually contains at
    # least one of each so the exclusion reasons stay exercised, not just
    # declared.
    assert set(EVIDENCE_CONSTRUCTS) == {"ai-project", "ai-agent", "causal"}
    reasons = {c.reason for c in EXCLUDED_CASES}
    for construct in EVIDENCE_CONSTRUCTS:
        assert construct in reasons, f"no corpus file currently exercises the '{construct}' exclusion"
