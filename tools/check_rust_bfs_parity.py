# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare the solver-free Rust BFS oracle with Python Monitor BFS."""
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

from tests.oracle import bfs_oracle, can_monitor  # noqa: E402
from fslc.runtime import Monitor  # noqa: E402
from tools.check_rust_kernel_parity import kernel_cases  # noqa: E402


DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-bfs.exe" if os.name == "nt" else "fsl-bfs")
)


def _python_projection(path: Path, depth: int) -> dict[str, Any]:
    oracle = bfs_oracle(path, depth)
    violation = None
    if oracle.violations:
        entry = min(oracle.violations.values(), key=lambda item: item["depth"])
        violation = {
            "kind": entry["kind"],
            "name": entry["name"],
            "step": entry["depth"],
        }
    monitor = Monitor(path)
    action_names = {action["name"] for action in monitor.spec["actions"]}
    physical_by_display = {
        display: physical for physical, display in monitor.spec["display_names"].items()
    }
    covered = {
        physical_by_display.get(name, name) for name in oracle.action_coverage
    }
    return {
        "spec": oracle.spec,
        "states_explored": oracle.states_explored,
        "violation": violation,
        "reachables": {
            name: entry["depth"] for name, entry in sorted(oracle.reachables.items())
        },
        "deadlock_step": oracle.deadlock["depth"] if oracle.deadlock else None,
        "action_coverage": {
            name: name in covered for name in sorted(action_names)
        },
    }


def _rust_projection(path: Path, depth: int, rust_bin: Path) -> dict[str, Any]:
    proc = subprocess.run(
        [str(rust_bin), str(path), str(depth)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"Rust BFS failed for {path}: {proc.stderr.strip()}")
    raw = json.loads(proc.stdout)
    return {
        "spec": raw["spec"],
        "states_explored": raw["states_explored"],
        "violation": raw["violation"],
        "reachables": {
            name: step for name, step in sorted(raw["reachables"].items()) if step is not None
        },
        "deadlock_step": raw["deadlock_step"],
        "action_coverage": raw["action_coverage"],
    }


def run(root: Path, rust_bin: Path, depth: int) -> dict[str, Any]:
    cases = []
    unsupported = []
    for path, _ in kernel_cases(root):
        if path.parent != root / "specs":
            continue
        supported, reason = can_monitor(path)
        if not supported:
            unsupported.append({"path": path.relative_to(root).as_posix(), "reason": reason})
            continue
        cases.append(path)
    failures = []
    for path in cases:
        expected = _python_projection(path, depth)
        try:
            actual = _rust_projection(path, depth, rust_bin)
        except RuntimeError as exc:
            failures.append(
                {"path": path.relative_to(root).as_posix(), "kind": "rust_error", "error": str(exc)}
            )
            continue
        if expected != actual:
            failures.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "kind": "oracle_mismatch",
                    "python": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-bfs-parity.v1",
        "depth": depth,
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "unsupported": unsupported,
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--depth", type=int, default=2)
    args = parser.parse_args(argv)
    if not args.rust_bin.is_file():
        parser.error(f"Rust BFS binary not found: {args.rust_bin}; run cargo build first")
    result = run(args.root, args.rust_bin, args.depth)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
