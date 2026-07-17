# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare native Rust and Python scenario identities and replay every scenario."""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from fslc.cli import run_scenarios  # noqa: E402
from fslc.runtime import Monitor  # noqa: E402
from tests.oracle import can_monitor  # noqa: E402
from tools.check_rust_bmc_parity import (  # noqa: E402
    DEFAULT_REPLAY_BIN,
    _json_normalize,
)
from tools.check_rust_cli_snapshot import (  # noqa: E402
    DEFAULT_RUST_BIN,
    _invoke,
    _project_verify,
)


def _identity(output: dict[str, Any]) -> dict[str, Any]:
    if output.get("result") != "scenarios":
        return _project_verify(output)
    return {
        "result": "scenarios",
        "spec": output.get("spec"),
        "scenarios": sorted(
            (
                scenario.get("name"),
                scenario.get("kind"),
                scenario.get("property"),
                scenario.get("action"),
            )
            for scenario in output.get("scenarios", [])
        ),
        "warning_kinds": sorted(
            warning.get("kind") or "none"
            for warning in output.get("warnings", [])
        ),
    }


def _replay_with_python(path: Path, scenarios: list[dict[str, Any]]) -> None:
    for scenario in scenarios:
        monitor = Monitor(path)
        monitor.reset()
        for index, event in enumerate(scenario.get("steps", []), start=1):
            result = monitor.step(event["action"], event["params"])
            if not result.get("ok"):
                raise RuntimeError(
                    f"{scenario['name']}: Python rejected step {index}: {result}"
                )
        expected = scenario.get("expected_states") or []
        if expected and _json_normalize(monitor.state) != _json_normalize(expected[-1]):
            raise RuntimeError(f"{scenario['name']}: Python final state differs")


def _replay_with_rust(
    path: Path, scenarios: list[dict[str, Any]], replay_bin: Path
) -> None:
    monitor = Monitor(path)
    physical_by_display = {
        display: physical
        for physical, display in (monitor.spec.get("display_names") or {}).items()
    }
    display_names = monitor.spec.get("display_names") or {}
    for scenario in scenarios:
        events = [
            {
                "action": physical_by_display.get(event["action"], event["action"]),
                "params": event["params"],
            }
            for event in scenario.get("steps", [])
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
            raise RuntimeError(
                f"{scenario['name']}: Rust rejected scenario: {proc.stderr.strip()}"
            )
        replayed = json.loads(proc.stdout)
        final = {
            display_names.get(name, name): value
            for name, value in replayed["state"].items()
        }
        expected = scenario.get("expected_states") or []
        if expected and _json_normalize(final) != _json_normalize(expected[-1]):
            raise RuntimeError(f"{scenario['name']}: Rust final state differs")


def run(root: Path, rust_bin: Path, replay_bin: Path, depth: int) -> dict[str, Any]:
    cases = []
    for path in sorted((root / "specs").glob("*.fsl")):
        supported, _ = can_monitor(path)
        if supported:
            cases.append(path)
    failures = []
    scenario_count = 0
    for path in cases:
        relative = path.relative_to(root).as_posix()
        python = run_scenarios(str(path), depth, deadlock_mode="warn")
        rust = _invoke(
            rust_bin,
            ["scenarios", str(path), "--depth", str(depth), "--deadlock", "warn"],
        )
        try:
            if python.get("result") == "scenarios":
                _replay_with_rust(path, python.get("scenarios", []), replay_bin)
            if rust.get("result") == "scenarios":
                _replay_with_python(path, rust.get("scenarios", []))
                scenario_count += len(rust.get("scenarios", []))
        except RuntimeError as exc:
            failures.append({"path": relative, "kind": "replay", "error": str(exc)})
            continue
        expected = _identity(python)
        actual = _identity(rust)
        if expected != actual:
            failures.append(
                {
                    "path": relative,
                    "kind": "identity_mismatch",
                    "python": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-scenarios-parity.v1",
        "depth": depth,
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "rust_scenarios_replayed": scenario_count,
        "cross_replay": ["Python scenarios -> Rust Monitor", "Rust scenarios -> Python Monitor"],
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--replay-bin", type=Path, default=DEFAULT_REPLAY_BIN)
    parser.add_argument("--depth", type=int, default=5)
    args = parser.parse_args(argv)
    for binary in (args.rust_bin, args.replay_bin):
        if not binary.is_file():
            parser.error(f"Rust binary not found: {binary}; run cargo build first")
    result = run(args.root, args.rust_bin, args.replay_bin, args.depth)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
