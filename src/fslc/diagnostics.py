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
