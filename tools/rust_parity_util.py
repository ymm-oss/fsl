# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Shared normalization and comparison helpers for Rust parity harnesses."""
from __future__ import annotations

import re
from typing import Any


# Witness choices are not unique. Their semantic equivalence is established by
# check_rust_bmc_parity.py and check_rust_scenarios_parity.py through
# cross-implementation Monitor replay. These are the only witness paths whose
# concrete values are normalized here.
TRACE_PATHS = {
    "$.trace",
    "$.deadlock.trace",
}


def _value_shape(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: _value_shape(item) for key, item in sorted(value.items())}
    if isinstance(value, list):
        return [_value_shape(item) for item in value]
    return f"<{type(value).__name__}>"


def _trace_shape(trace: Any) -> Any:
    if not isinstance(trace, list):
        return _value_shape(trace)
    shaped = []
    for entry in trace:
        if not isinstance(entry, dict):
            shaped.append(_value_shape(entry))
            continue
        item: dict[str, Any] = {"keys": sorted(entry)}
        if "action" in entry:
            action = entry["action"]
            item["action_keys"] = sorted(action) if isinstance(action, dict) else _value_shape(action)
        if "changes" in entry:
            changes = entry["changes"]
            if isinstance(changes, dict):
                item["changes_shape"] = sorted(
                    {
                        tuple(sorted(change)) if isinstance(change, dict) else ("<non-object>",)
                        for change in changes.values()
                    }
                )
            else:
                item["changes_shape"] = _value_shape(changes)
        if "blame" in entry:
            blame = entry["blame"]
            item["blame_keys"] = sorted(blame) if isinstance(blame, dict) else _value_shape(blame)
        shaped.append(item)
    return shaped


def _normalize(value: Any, path: str = "$") -> Any:
    if path in TRACE_PATHS:
        return _trace_shape(value)
    if path.startswith("$.reachables.") and path.endswith(".witness"):
        return _trace_shape(value)
    if path == "$.cost.elapsed_s":
        return "<elapsed>"
    if path in {"$.violating_bindings", "$.last_action.params"}:
        return _value_shape(value)
    if path.startswith("$.blame.conjuncts.") and path.endswith(".violating_bindings"):
        return _value_shape(value)
    if isinstance(value, dict):
        return {
            key: _normalize(item, f"{path}.{key}")
            for key, item in sorted(value.items())
            if key != "fsl"
        }
    if isinstance(value, list):
        return [_normalize(item, f"{path}.{index}") for index, item in enumerate(value)]
    if (
        path.startswith("$.warnings.")
        and path.endswith(".message")
        and isinstance(value, str)
    ):
        return re.sub(r"(deadlock reachable at step \d+) \(state: .*\)$", r"\1 (state: <witness>)", value)
    return value


def _diff(expected: Any, actual: Any, path: str = "$") -> list[dict[str, Any]]:
    if type(expected) is not type(actual):
        return [{"path": path, "python": expected, "rust": actual}]
    if isinstance(expected, dict):
        failures = []
        for key in sorted(set(expected) | set(actual)):
            if key not in expected or key not in actual:
                failures.append(
                    {
                        "path": f"{path}.{key}",
                        "python": expected.get(key, "<missing>"),
                        "rust": actual.get(key, "<missing>"),
                    }
                )
            else:
                failures.extend(_diff(expected[key], actual[key], f"{path}.{key}"))
        return failures
    if isinstance(expected, list):
        if len(expected) != len(actual):
            return [{"path": path, "python": expected, "rust": actual}]
        failures = []
        for index, (left, right) in enumerate(zip(expected, actual)):
            failures.extend(_diff(left, right, f"{path}.{index}"))
        return failures
    return [] if expected == actual else [{"path": path, "python": expected, "rust": actual}]
