# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare bounded leadsTo decisions and cross-replay liveness witnesses."""
from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

from fslc.cli import run_verify
from fslc.runtime import Monitor

from check_rust_bmc_parity import DEFAULT_REPLAY_BIN
from check_rust_cli_snapshot import DEFAULT_RUST_BIN, _invoke


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    "examples/gallery/errors/violated_leads_to_starvation.fsl",
    "examples/gallery/adversarial/simultaneous_leads_to_satisfaction.fsl",
    "examples/gallery/valid/small_tcp_handshake.fsl",
)


def _normalize(value: Any, path: str = "$") -> Any:
    if path == "$.trace":
        return "<cross-replayed-witness>"
    if path == "$.cost.elapsed_s":
        return "<elapsed>"
    if path in {"$.pending_since", "$.loop_start", "$.hint"}:
        return "<witness-derived>"
    if isinstance(value, dict):
        return {
            key: _normalize(item, f"{path}.{key}")
            for key, item in sorted(value.items())
            if key != "fsl"
        }
    if isinstance(value, list):
        return [_normalize(item, f"{path}.{index}") for index, item in enumerate(value)]
    return value


def _canonical(value: Any) -> Any:
    return json.loads(json.dumps(value, ensure_ascii=False, sort_keys=True))


def _replay_rust_in_python(path: Path, trace: list[dict[str, Any]]) -> None:
    monitor = Monitor(path)
    initial = monitor.reset()
    if _canonical(initial) != _canonical(trace[0]["state"]):
        raise RuntimeError("Rust leadsTo witness has a different Python initial state")
    for index, entry in enumerate(trace[1:], start=1):
        action = entry["action"]
        stepped = monitor.step(action["name"], action["params"])
        if not stepped.get("ok"):
            raise RuntimeError(f"Python Monitor rejected Rust step {index}: {stepped}")
    if _canonical(monitor.state) != _canonical(trace[-1]["state"]):
        raise RuntimeError("Python Monitor reached a different Rust witness state")


def _replay_python_in_rust(
    path: Path, trace: list[dict[str, Any]], replay_bin: Path
) -> None:
    monitor = Monitor(path)
    display_names = monitor.spec.get("display_names") or {}
    physical_by_display = {display: physical for physical, display in display_names.items()}
    events = [
        {
            "action": physical_by_display.get(entry["action"]["name"], entry["action"]["name"]),
            "params": entry["action"]["params"],
        }
        for entry in trace[1:]
    ]
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
        raise RuntimeError(f"Rust Monitor rejected Python witness: {proc.stderr.strip()}")
    replayed = json.loads(proc.stdout)
    final = {
        display_names.get(name, name): value
        for name, value in replayed["state"].items()
    }
    if _canonical(final) != _canonical(trace[-1]["state"]):
        raise RuntimeError("Rust Monitor reached a different Python witness state")


def run(root: Path, rust_bin: Path, replay_bin: Path, depth: int) -> dict[str, Any]:
    failures = []
    for relative in CASES:
        path = root / relative
        python = run_verify(path, depth, "warn", use_cache=False)
        rust = _invoke(
            rust_bin,
            ["verify", str(path), "--depth", str(depth), "--deadlock", "warn"],
        )
        try:
            if python.get("trace"):
                _replay_python_in_rust(path, python["trace"], replay_bin)
            if rust.get("trace"):
                _replay_rust_in_python(path, rust["trace"])
        except RuntimeError as error:
            failures.append({"path": relative, "kind": "replay", "error": str(error)})
            continue
        expected = _normalize(python)
        actual = _normalize(rust)
        if expected != actual:
            failures.append(
                {
                    "path": relative,
                    "kind": "envelope",
                    "python": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-leadsto-parity.v1",
        "depth": depth,
        "cases": len(CASES),
        "matched": len(CASES) - len(failures),
        "cross_replay": ["Rust BMC -> Python Monitor", "Python BMC -> Rust Monitor"],
        "allowlist": {
            "$.trace": "nondeterministic lasso witness; cross-replayed in both directions",
            "$.pending_since": "derived from nondeterministic lasso witness",
            "$.loop_start": "derived from nondeterministic lasso witness",
            "$.hint": "contains nondeterministic witness step numbers",
            "$.cost.elapsed_s": "wall-clock timing",
        },
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--replay-bin", type=Path, default=DEFAULT_REPLAY_BIN)
    parser.add_argument("--depth", type=int, default=5)
    args = parser.parse_args(argv)
    result = run(args.root, args.rust_bin, args.replay_bin, args.depth)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
