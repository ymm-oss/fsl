# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Shared diagnostic post-processing helpers."""
from __future__ import annotations


FAITHFULNESS_ACTIONS = {
    "partial_op_unguarded": "add the missing guard / run bounded Monitor (replay)",
    "frozen_only_invariant": "run mutate to check kill-rate",
    "liveness_not_refined": "re-prove liveness at each layer or add preserve progress to the refinement mapping",
    "intent_unexercised": "add a single-shot reachable for the action / raise --depth",
}


def faithfulness_class_for(diagnostic):
    """Derive a faithfulness routing class from existing diagnostic fields."""
    if not isinstance(diagnostic, dict):
        return None

    kind = diagnostic.get("kind")
    violation_kind = diagnostic.get("violation_kind")
    result = diagnostic.get("result")

    if kind == "partial_op" or violation_kind == "partial_op":
        return "partial_op_unguarded"
    if kind == "tautology_over_frozen":
        return "frozen_only_invariant"
    if (
            result == "reachable_failed"
            or kind == "reachable_failed"
            or diagnostic.get("covered") is False
            or diagnostic.get("classification") in {
                "insufficient_depth",
                "over_constrained",
            }):
        return "intent_unexercised"
    if kind in {
            "leadsTo_refinement_failed",
            "leadsto_refinement_failed",
            "liveness_not_refined",
            "progress_lost",
    }:
        return "liveness_not_refined"

    return None


def recommended_action_for(faithfulness_class):
    return FAITHFULNESS_ACTIONS.get(faithfulness_class)


# Vacuity finding kinds that surface as `{"result": "error", "kind": <here>}`
# under `--vacuity error` (see bmc._finalize_vacuity_findings).
_VACUITY_KINDS = frozenset({
    "vacuous_implication", "vacuous_leadsto", "tautology_over_frozen",
    "urgency_freeze", "always_true_requires",
})


def trace_type_for(diagnostic):
    """Unified repair-routing discriminator for a top-level result.

    Derived from fields the result already carries — no engine change — so an
    agent can route a fix by channel (and tell an SLA deadline from a structural
    invariant). Returns None for non-counterexample results (verified/ok/spec
    errors), so it is only set where there is something to repair.
    """
    if not isinstance(diagnostic, dict):
        return None
    result = diagnostic.get("result")
    if result == "refinement_failed":
        return "refinement"
    if result == "reachable_failed":
        return "reachable"
    if result == "nonconformant":
        return "conformance"
    if result == "unknown_cti":
        return "induction_cti"
    if result == "violated":
        vk = diagnostic.get("violation_kind")
        # SLA deadlines are generated invariants named `_deadline_<req>_<age>_<n>`
        # (dialects._deadline_invariant_name); the `_` prefix is reserved, so the
        # match is unambiguous.
        if vk == "invariant" and str(diagnostic.get("invariant", "")).startswith("_deadline_"):
            return "sla"
        return vk or "invariant"
    if result == "error":
        kind = diagnostic.get("kind")
        if kind == "acceptance":
            return "acceptance"
        if kind in ("forbidden", "forbidden_setup"):
            return "forbidden"
        if kind in _VACUITY_KINDS:
            return "vacuity"
    return None


def with_faithfulness(value):
    """Return value with additive faithfulness routing fields on diagnostics."""
    if isinstance(value, list):
        return [with_faithfulness(v) for v in value]
    if not isinstance(value, dict):
        return value

    out = {k: with_faithfulness(v) for k, v in value.items()}
    cls = faithfulness_class_for(out)
    if cls is not None:
        out.setdefault("faithfulness_class", cls)
        action = recommended_action_for(cls)
        if action is not None:
            out.setdefault("recommended_action", action)
    return out
