"""Concrete bounded oracle for fslc tests.

This module intentionally avoids Z3 and the BMC/refinement checkers.  It
enumerates bounded reachable states by driving ``fslc.runtime.Monitor`` with
``enabled()`` and ``step()`` and hashes normalized ``Monitor.state`` snapshots.

Limitation: bugs in the BMC encoding are detectable only when they disagree
with Monitor's concrete single-step semantics.  Bugs shared by the BMC and
Monitor step semantics are not detectable by this oracle.
"""
from __future__ import annotations
import sys

import copy
import json
import subprocess
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from fslc.cli import exit_code, run_verify
from fslc.model import FslError, domain_range
from fslc.runtime import Monitor, _as_bool, eval_concrete


ROOT = Path(__file__).resolve().parents[1]
PYTHON = sys.executable


@dataclass(frozen=True)
class VerifyCase:
    path: Path
    depth: int
    deadlock: str = "warn"
    engine: str = "bmc"

    @property
    def id(self) -> str:
        return self.path.relative_to(ROOT).as_posix()


@dataclass
class OracleResult:
    spec: str
    depth: int
    violations: dict[str, dict[str, Any]] = field(default_factory=dict)
    reachables: dict[str, dict[str, Any]] = field(default_factory=dict)
    deadlock: dict[str, Any] | None = None
    states_explored: int = 0


class UnsupportedOracle(RuntimeError):
    pass


def normalize(value: Any) -> Any:
    if isinstance(value, dict):
        return tuple((str(k), normalize(v)) for k, v in sorted(value.items(), key=lambda item: str(item[0])))
    if isinstance(value, (list, tuple)):
        return tuple(normalize(v) for v in value)
    if isinstance(value, set):
        return tuple(sorted(normalize(v) for v in value))
    return value


def state_key(mon: Monitor) -> Any:
    return normalize(mon.state)


def action_key(enabled: dict[str, Any]) -> str:
    return json.dumps(enabled, sort_keys=True, ensure_ascii=True)


def trace_event(step: int, mon: Monitor, result: dict[str, Any]) -> dict[str, Any]:
    event = {
        "step": step,
        "state": result.get("state", mon.state),
        "action": {"name": result["action"], "params": result.get("params", {})},
        "changes": result.get("changes", {}),
    }
    return event


def _reachable_now(mon: Monitor, reach: dict[str, Any]) -> bool:
    return _as_bool(eval_concrete(reach["expr"], mon._phys, {}, mon.spec))  # noqa: SLF001


def _record_reachables(mon: Monitor, out: OracleResult, trace: list[dict[str, Any]], depth: int) -> None:
    for reach in mon.spec["reachables"]:
        if reach["name"] in out.reachables:
            continue
        if _reachable_now(mon, reach):
            out.reachables[reach["name"]] = {"depth": depth, "trace": list(trace)}


def _record_violation(
    out: OracleResult,
    result: dict[str, Any],
    depth: int,
    trace: list[dict[str, Any]],
) -> None:
    kind = result.get("kind", "unknown")
    name = result.get("name") or result.get("action") or kind
    key = f"{kind}:{name}"
    old = out.violations.get(key)
    if old is None or depth < old["depth"]:
        out.violations[key] = {
            "kind": kind,
            "name": name,
            "depth": depth,
            "trace": list(trace),
            "result": dict(result),
        }


def bfs_oracle(source_or_path: str | Path, depth: int) -> OracleResult:
    mon0 = Monitor(source_or_path)
    initial = mon0.reset()
    out = OracleResult(spec=mon0.spec["name"], depth=depth)
    initial_trace = [{"step": 0, "state": initial}]
    _record_reachables(mon0, out, initial_trace, 0)

    queue = deque([(mon0, initial_trace, 0)])
    visited = {state_key(mon0)}

    while queue:
        mon, trace, d = queue.popleft()
        out.states_explored += 1

        try:
            enabled = sorted(mon.enabled(), key=action_key)
        except Exception as exc:
            raise UnsupportedOracle(f"Monitor.enabled() could not enumerate this state: {exc}") from exc
        if not enabled and (out.deadlock is None or d < out.deadlock["depth"]):
            out.deadlock = {"depth": d, "trace": list(trace), "state": mon.state}
        if d >= depth:
            continue

        for enabled_action in enabled:
            child = copy.deepcopy(mon)
            result = child.step(enabled_action["action"], enabled_action.get("params", {}))
            next_trace = list(trace)
            next_trace.append(trace_event(d + 1, child if result.get("ok") else mon, result))
            if not result.get("ok"):
                _record_violation(out, result, d + 1, next_trace)
                continue

            _record_reachables(child, out, next_trace, d + 1)
            key = state_key(child)
            if key in visited:
                continue
            visited.add(key)
            queue.append((child, next_trace, d + 1))

    return out


def run_verify_case(case: VerifyCase) -> dict[str, Any]:
    return run_verify(
        str(case.path),
        case.depth,
        deadlock_mode=case.deadlock,
        engine=case.engine,
    )


def run_cli_json(args: list[str]) -> tuple[int, dict[str, Any]]:
    proc = subprocess.run(
        [str(PYTHON), "-m", "fslc", *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        return proc.returncode, json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise AssertionError(f"non-JSON fslc output: rc={proc.returncode} stderr={proc.stderr}") from exc


def trace_actions(trace: list[dict[str, Any]]) -> list[tuple[str, dict[str, Any]]]:
    actions = []
    for item in trace:
        action = item.get("action")
        if action:
            actions.append((action["name"], dict(action.get("params", {}))))
    return actions


def replay_actions(source_or_path: str | Path, actions: list[tuple[str, dict[str, Any]]]) -> tuple[Monitor, list[dict[str, Any]]]:
    mon = Monitor(source_or_path)
    mon.reset()
    results = []
    for name, params in actions:
        result = mon.step(name, params)
        results.append(result)
        if not result.get("ok"):
            break
    return mon, results


def replay_trace(source_or_path: str | Path, trace: list[dict[str, Any]]) -> tuple[Monitor, list[dict[str, Any]]]:
    return replay_actions(source_or_path, trace_actions(trace))


def verify_exit_code_matches(result: dict[str, Any]) -> bool:
    return exit_code(result) in (0, 1, 2, 3)


def expr_holds_in_monitor(mon: Monitor, expr: Any) -> bool:
    return _as_bool(eval_concrete(expr, mon._phys, {}, mon.spec))  # noqa: SLF001


def domain_values(ty: tuple[Any, ...], types: dict[str, Any]) -> range:
    lo, hi = domain_range(ty, types)
    return range(lo, hi + 1)


def can_monitor(path: Path) -> tuple[bool, str | None]:
    try:
        Monitor(path).reset()
    except FslError as exc:
        return False, f"{exc.kind}: {exc}"
    except Exception as exc:  # parse/type errors are not oracle candidates.
        return False, type(exc).__name__ + f": {exc}"
    return True, None
