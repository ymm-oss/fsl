# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Explicit-state engine vs. BMC wall-time benchmark (issue #212).

Not a pytest assertion -- wall-clock timing is inherently noisy across
machines/CI runners. The mechanical guarantees (verdict agreement with the
BMC engine, trace replay) live in the Rust integration tests; this script is
the human-facing acceptance check for "explicit is orders of magnitude
faster than Z3-based BMC on the small-state-space corpus".

Times the *native* fslc binary end-to-end (process startup included -- that
is what an LLM write/verify/repair loop actually pays). The verdict cache is
pointed at a throwaway directory so every timed run is cold.

Usage:
    python tools/bench_explicit.py [spec.fsl ...] [--depth N] [--runs N]
                                   [--fslc PATH]
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

TERMINAL = {"verified", "proved", "violated", "reachable_failed", "unknown_budget"}


def _default_binary() -> Path:
    for candidate in (
        ROOT / "rust" / "target" / "release" / "fslc",
        ROOT / "rust" / "target" / "debug" / "fslc",
    ):
        if candidate.exists():
            return candidate
    sys.exit(
        "native fslc binary not found -- build it first:\n"
        "    cargo build --release -p fslc --manifest-path rust/Cargo.toml"
    )


def _run_once(binary: Path, spec: Path, depth: int, engine: str) -> tuple[float, dict]:
    with tempfile.TemporaryDirectory(prefix="fslc-bench-explicit-") as cache_dir:
        env = dict(os.environ, FSLC_CACHE_DIR=cache_dir)
        start = time.perf_counter()
        proc = subprocess.run(
            [str(binary), "verify", str(spec), "--depth", str(depth), "--engine", engine],
            capture_output=True,
            text=True,
            env=env,
        )
        elapsed = time.perf_counter() - start
    try:
        out = json.loads(proc.stdout)
    except json.JSONDecodeError:
        sys.exit(f"{spec.name} ({engine}): non-JSON output:\n{proc.stdout}\n{proc.stderr}")
    return elapsed, out


def _time_engine(binary: Path, spec: Path, depth: int, engine: str, runs: int):
    best = float("inf")
    best_engine_s = float("inf")
    result = "?"
    for _ in range(runs):
        elapsed, out = _run_once(binary, spec, depth, engine)
        result = out.get("result", "?")
        if result == "error":
            return None, None, f"error:{out.get('kind', '?')}"
        assert result in TERMINAL, out
        best = min(best, elapsed)
        engine_s = (out.get("cost") or {}).get("elapsed_s")
        if isinstance(engine_s, (int, float)):
            best_engine_s = min(best_engine_s, engine_s)
    return best, best_engine_s, result


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("specs", nargs="*", type=Path, default=None)
    ap.add_argument("--depth", type=int, default=8)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--fslc", type=Path, default=None, help="native fslc binary")
    args = ap.parse_args()

    binary = args.fslc or _default_binary()
    specs = args.specs or sorted((ROOT / "specs").glob("*.fsl"))
    print(
        f"{'spec':<30} {'bmc (s)':>10} {'explicit (s)':>13} {'speedup':>9} "
        f"{'bmc engine(s)':>14} {'expl engine(s)':>15} {'engine x':>9} "
        f"{'bmc':>16} {'explicit':>16}"
    )
    for spec in specs:
        exp_t, exp_e, exp_r = _time_engine(binary, spec, args.depth, "explicit", args.runs)
        if exp_t is None:
            print(f"{spec.name:<30} skipped ({exp_r})")
            continue
        bmc_t, bmc_e, bmc_r = _time_engine(binary, spec, args.depth, "bmc", args.runs)
        speedup = bmc_t / exp_t if exp_t > 0 else float("inf")
        engine_speedup = bmc_e / exp_e if exp_e > 0 else float("inf")
        print(
            f"{spec.name:<30} {bmc_t:>10.4f} {exp_t:>13.4f} {speedup:>8.1f}x "
            f"{bmc_e:>14.4f} {exp_e:>15.6f} {engine_speedup:>8.1f}x "
            f"{bmc_r:>16} {exp_r:>16}"
        )


if __name__ == "__main__":
    main()
