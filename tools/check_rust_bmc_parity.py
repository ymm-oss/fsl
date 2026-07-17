# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Triangulate Rust BMC against the Rust and Python solver-free BFS oracles."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.check_rust_bfs_parity import (
    DEFAULT_RUST_BIN as DEFAULT_BFS_BIN,
    _python_projection,
)
from tools.check_rust_kernel_parity import kernel_cases
from fslc.cli import run_verify
from fslc.runtime import Monitor
from tests.oracle import can_monitor


DEFAULT_BMC_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-bmc.exe" if os.name == "nt" else "fsl-bmc")
)
DEFAULT_REPLAY_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-replay-actions.exe" if os.name == "nt" else "fsl-replay-actions")
)


def _run_json(binary: Path, path: Path, depth: int, label: str) -> dict[str, Any]:
    proc = subprocess.run(
        [str(binary), str(path), str(depth)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"{label} failed for {path}: {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def _decision_projection(raw: dict[str, Any]) -> dict[str, Any]:
    projection = {
        "spec": raw["spec"],
        "violation": raw["violation"],
    }
    if raw["violation"] is None:
        projection.update(
            {
                "reachables": {
                    name: step
                    for name, step in sorted(raw["reachables"].items())
                    if step is not None
                },
                "deadlock_step": raw["deadlock_step"],
                "action_coverage": raw["action_coverage"],
            }
        )
    return projection


def _json_normalize(value: Any) -> Any:
    return json.loads(json.dumps(value, ensure_ascii=False, sort_keys=True))


def _replay_rust_witnesses(path: Path, raw: dict[str, Any]) -> None:
    witnesses = raw.get("witnesses") or {}
    traces = []
    if witnesses.get("violation"):
        traces.append(("violation", witnesses["violation"], True))
    traces.extend(
        (f"reachable:{name}", trace, False)
        for name, trace in (witnesses.get("reachables") or {}).items()
    )
    if witnesses.get("deadlock"):
        traces.append(("deadlock", witnesses["deadlock"], False))

    for label, trace, final_may_violate in traces:
        monitor = Monitor(path)
        initial = monitor.reset()
        display_names = monitor.spec.get("display_names") or {}
        rust_initial = {
            display_names.get(name, name): value
            for name, value in trace[0]["state"].items()
        }
        if _json_normalize(initial) != _json_normalize(rust_initial):
            raise RuntimeError(f"{label}: initial state differs")
        for index, entry in enumerate(trace[1:], start=1):
            action = entry["action"]
            stepped = monitor.step(action["name"], action["params"])
            is_final_violation = final_may_violate and index == len(trace) - 1
            if not stepped.get("ok") and not is_final_violation:
                raise RuntimeError(
                    f"{label}: Python Monitor rejected step {index}: {stepped}"
                )
        rust_final = {
            display_names.get(name, name): value
            for name, value in trace[-1]["state"].items()
        }
        if _json_normalize(monitor.state) != _json_normalize(rust_final):
            raise RuntimeError(f"{label}: final state differs")


def _python_traces(path: Path, depth: int) -> list[tuple[str, list[dict[str, Any]]]]:
    result = run_verify(str(path), depth, deadlock_mode="warn")
    traces = []
    if result.get("trace"):
        traces.append((result.get("result", "result"), result["trace"]))
    for name, witness in (result.get("reachables") or {}).items():
        if isinstance(witness, dict) and witness.get("witness"):
            traces.append((f"reachable:{name}", witness["witness"]))
    deadlock = result.get("deadlock")
    if isinstance(deadlock, dict) and deadlock.get("trace"):
        traces.append(("deadlock", deadlock["trace"]))
    return traces


def _replay_python_witnesses(
    path: Path, depth: int, replay_bin: Path
) -> None:
    monitor = Monitor(path)
    physical_by_display = {
        display: physical
        for physical, display in (monitor.spec.get("display_names") or {}).items()
    }
    display_names = monitor.spec.get("display_names") or {}
    for label, trace in _python_traces(path, depth):
        events = []
        for entry in trace:
            action = entry.get("action")
            if not isinstance(action, dict):
                continue
            events.append(
                {
                    "action": physical_by_display.get(action["name"], action["name"]),
                    "params": action["params"],
                }
            )
        proc = subprocess.run(
            [str(replay_bin), str(path)],
            cwd=ROOT,
            input=json.dumps(events, ensure_ascii=False),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"{label}: Rust Monitor rejected Python witness: {proc.stderr.strip()}"
            )
        replayed = json.loads(proc.stdout)
        rust_final = {
            display_names.get(name, name): value
            for name, value in replayed["state"].items()
        }
        if _json_normalize(rust_final) != _json_normalize(trace[-1]["state"]):
            raise RuntimeError(f"{label}: Rust replay final state differs")


def run(
    root: Path, bmc_bin: Path, bfs_bin: Path, replay_bin: Path, depth: int
) -> dict[str, Any]:
    cases = []
    unsupported = []
    for path, _ in kernel_cases(root):
        if path.parent != root / "specs":
            continue
        supported, reason = can_monitor(path)
        if supported:
            cases.append(path)
        else:
            unsupported.append(
                {"path": path.relative_to(root).as_posix(), "reason": reason}
            )

    failures = []
    for path in cases:
        relative = path.relative_to(root).as_posix()
        python_raw = _python_projection(path, depth)
        python_raw.pop("states_explored")
        expected = _decision_projection(python_raw)
        try:
            rust_bfs_raw = _run_json(bfs_bin, path, depth, "Rust BFS")
            rust_bmc_raw = _run_json(bmc_bin, path, depth, "Rust BMC")
            _replay_rust_witnesses(path, rust_bmc_raw)
            _replay_python_witnesses(path, depth, replay_bin)
        except RuntimeError as exc:
            failures.append({"path": relative, "kind": "rust_error", "error": str(exc)})
            continue
        rust_bfs_raw.pop("states_explored")
        rust_bfs = _decision_projection(rust_bfs_raw)
        rust_bmc = _decision_projection(rust_bmc_raw)
        if expected != rust_bfs or expected != rust_bmc:
            failures.append(
                {
                    "path": relative,
                    "kind": "oracle_mismatch",
                    "python_bfs": expected,
                    "rust_bfs": rust_bfs,
                    "rust_bmc": rust_bmc,
                }
            )
    return {
        "schema": "fsl-rust-bmc-parity.v1",
        "depth": depth,
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "unsupported": unsupported,
        "cross_replay": [
            "Rust BMC -> Rust Monitor",
            "Rust BMC -> Python Monitor",
            "Python BMC -> Rust Monitor",
        ],
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--bmc-bin", type=Path, default=DEFAULT_BMC_BIN)
    parser.add_argument("--bfs-bin", type=Path, default=DEFAULT_BFS_BIN)
    parser.add_argument("--replay-bin", type=Path, default=DEFAULT_REPLAY_BIN)
    parser.add_argument("--depth", type=int, default=2)
    args = parser.parse_args(argv)
    for label, binary in (
        ("BMC", args.bmc_bin),
        ("BFS", args.bfs_bin),
        ("replay", args.replay_bin),
    ):
        if not binary.is_file():
            parser.error(f"Rust {label} binary not found: {binary}; run cargo build first")
    result = run(
        args.root, args.bmc_bin, args.bfs_bin, args.replay_bin, args.depth
    )
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
