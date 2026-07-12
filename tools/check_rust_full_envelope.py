# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare Python and Rust CLI envelopes with a narrow reviewed allowlist."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

from fslc.cli import run_check, run_verify

from check_rust_cli_snapshot import DEFAULT_RUST_BIN, _invoke


ROOT = Path(__file__).resolve().parents[1]

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


def run(root: Path, binary: Path, depth: int) -> dict[str, Any]:
    failures = []
    cases = sorted((root / "specs").glob("*.fsl"))
    comparisons = 0
    for path in cases:
        relative = path.relative_to(root).as_posix()
        python_check = _normalize(run_check(path))
        rust_check = _normalize(_invoke(binary, ["check", str(path)]))
        comparisons += 1
        differences = _diff(python_check, rust_check)
        if differences:
            failures.append({"path": relative, "command": "check", "differences": differences})
        if python_check.get("result") != "ok":
            continue
        python_verify = _normalize(
            run_verify(path, depth, "warn", use_cache=False)
        )
        rust_verify = _normalize(
            _invoke(binary, ["verify", str(path), "--depth", str(depth), "--deadlock", "warn"])
        )
        comparisons += 1
        differences = _diff(python_verify, rust_verify)
        if differences:
            failures.append({"path": relative, "command": "verify", "differences": differences})
    return {
        "schema": "fsl-rust-full-envelope-parity.v1",
        "scope": "specs",
        "depth": depth,
        "cases": len(cases),
        "comparisons": comparisons,
        "matched": comparisons - len(failures),
        "failures": failures,
        "allowlist": {
            "$.cost.elapsed_s": "wall-clock timing",
            "$.trace": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.deadlock.trace": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.reachables.*.witness": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.violating_bindings": "derived from nondeterministic witness",
            "$.last_action.params": "derived from nondeterministic witness",
            "$.blame.conjuncts.*.violating_bindings": "derived from nondeterministic witness",
            "$.warnings.*.message[state]": "derived from replayed deadlock witness",
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--depth", type=int, default=5)
    args = parser.parse_args(argv)
    result = run(args.root, args.rust_bin, args.depth)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
