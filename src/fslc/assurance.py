# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Shared assurance-class vocabulary (issue #171).

fslc has several result-producing commands whose "how proven is this" signal
is spread across ad hoc fields: BMC/k-induction's ``completeness``, the
dialect wrappers' nested ``kernel``, runtime replay's ``guarantee_kind`` /
``evidence.kind``, and fsl-stochastic's ``status`` enum. This module is the
**one** place that turns any of those result dicts into one of five classes —
``proved`` / ``bounded`` / ``replay-observed`` / ``statistical`` / ``not_run``
— so ``ledger.py`` and ``html_report.py`` render the same vocabulary instead
of re-deriving it. Presentation/aggregation only: it reads existing result
fields and asserts nothing new.

See ``docs/DESIGN-assurance-classes.md`` for what each class does and does not
guarantee.
"""
from __future__ import annotations

PROVED = "proved"
BOUNDED = "bounded"
REPLAY_OBSERVED = "replay-observed"
STATISTICAL = "statistical"
NOT_RUN = "not_run"

# Strongest first — also the display precedence when a requirement carries
# evidence from more than one class.
ASSURANCE_ORDER = (PROVED, BOUNDED, REPLAY_OBSERVED, STATISTICAL, NOT_RUN)

_REPLAY_RESULTS = {
    "conformant", "nonconformant",
    "replay_conformant", "replay_nonconformant",
    "observed_conformant", "observed_mismatch",
    "conformance_checked",
    "observed_supported",
    "evidence_supported", "evidence_failed",
}
_STATISTICAL_STATUSES = {"statistically_supported", "statistically_unsupported"}
_STATISTICAL_GATE_STATUSES = {
    "dataset_invalid", "evaluator_untrusted", "slice_missing",
    "insufficient_samples", "inconclusive",
}


def classify_result(result) -> str:
    """Classify one command's result dict (envelope or bare) into a token."""
    if not isinstance(result, dict):
        return NOT_RUN

    completeness = result.get("completeness")
    kernel = result.get("kernel")
    if completeness is None and isinstance(kernel, dict):
        completeness = kernel.get("completeness")
    if completeness == "unbounded":
        return PROVED
    if completeness == "bounded":
        return BOUNDED

    evidence = result.get("evidence")
    evidence_kind = evidence.get("kind") if isinstance(evidence, dict) else None
    top = result.get("result")
    if (
        result.get("guarantee_kind") == "runtime_observed"
        or evidence_kind in ("runtime_replay", "runtime_telemetry")
        or top in _REPLAY_RESULTS
    ):
        return REPLAY_OBSERVED

    status = result.get("status", top)
    if status in _STATISTICAL_STATUSES:
        return STATISTICAL
    if status in _STATISTICAL_GATE_STATUSES:
        return NOT_RUN

    return NOT_RUN


def strongest(tokens) -> str:
    tokens = [t for t in tokens if t]
    if not tokens:
        return NOT_RUN
    return min(tokens, key=ASSURANCE_ORDER.index)


def weakest(tokens) -> str:
    tokens = [t for t in tokens if t]
    if not tokens:
        return NOT_RUN
    return max(tokens, key=ASSURANCE_ORDER.index)


def assurance_label(token, *, depth=None, confidence=None, under_assumptions=False) -> str:
    if token == PROVED:
        base = "proved(induction)"
    elif token == BOUNDED:
        base = f"bounded(BMC depth {depth})" if depth is not None else "bounded"
    elif token == REPLAY_OBSERVED:
        base = "replay-observed"
    elif token == STATISTICAL:
        base = f"statistical(Wilson {confidence * 100:g}%)" if confidence is not None else "statistical"
    else:
        base = NOT_RUN
    return base + "※前提付き" if under_assumptions else base


def confidence_of(result) -> float | None:
    """Best-effort Wilson confidence level (0-1) out of a stochastic result
    dict, checking the top-level interval and, failing that, its checks."""
    if not isinstance(result, dict):
        return None
    interval = result.get("interval")
    if isinstance(interval, dict) and interval.get("confidence") is not None:
        return interval["confidence"]
    for check in result.get("checks") or ():
        if isinstance(check, dict):
            interval = check.get("interval")
            if isinstance(interval, dict) and interval.get("confidence") is not None:
                return interval["confidence"]
    return None


def classify_element(group: str, name: str, verification) -> str:
    """Classify a single spec element (an invariant/leadsTo/reachable/... by
    name) against a ``verify``/``prove`` result dict."""
    if not isinstance(verification, dict):
        return NOT_RUN
    result = verification.get("result")
    if result == "proved":
        if group in ("invariants", "transitions"):
            return PROVED
        if group == "leadstos":
            entry = (verification.get("leads_to") or {}).get(name) or {}
            return PROVED if entry.get("completeness") == "unbounded" else BOUNDED
        # reachables and action coverage are base-depth BMC checks even under
        # a k-induction proof (bmc.prove() only ranks invariants/leadsTo).
        return BOUNDED
    completeness = verification.get("completeness")
    if completeness == "unbounded":
        return PROVED
    if completeness == "bounded":
        return BOUNDED
    if result == "error":
        return NOT_RUN
    return BOUNDED


def requirement_assurance(registry: dict, verification, evidence_results=()) -> dict:
    """{req_id: {"assurance", "sources": [...], "under_assumptions"}} for every
    requirement id in ``registry`` (as built by ``ledger._requirement_registry``,
    which tags each id with the element groups/names it labels) plus any id
    named by an ``--evidence`` result's ``requirements`` list."""
    out = {}
    evidence_by_req: dict = {}
    for ev in evidence_results or ():
        req_ids = list(ev.get("requirements") or [])
        req = ev.get("requirement")
        if isinstance(req, dict) and req.get("id"):
            req_ids.append(req["id"])
        for rid in req_ids:
            evidence_by_req.setdefault(rid, []).append(ev)

    all_ids = set(registry) | set(evidence_by_req)
    for rid in all_ids:
        entry = registry.get(rid) or {}
        elements = entry.get("elements") or {}
        sources = []
        formal_classes = [
            classify_element(group, name, verification)
            for group, names in elements.items()
            for name in names
        ]
        if formal_classes:
            sources.append({"kind": "formal", "assurance": weakest(formal_classes)})
        for ev in evidence_by_req.get(rid, ()):
            sources.append({
                "kind": "evidence",
                "assurance": classify_result(ev),
                "producer": ev.get("result"),
                "confidence": confidence_of(ev),
            })
        primary = strongest(s["assurance"] for s in sources) if sources else NOT_RUN
        out[rid] = {
            "assurance": primary,
            "sources": sources,
            "under_assumptions": verification.get("result") == "verified_under_assumptions",
        }
    return out
