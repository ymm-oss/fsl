# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Cold vs. warm wall-time benchmark for the verdict cache (issue #169).

Not a pytest assertion -- wall-clock timing is inherently noisy across
machines/CI runners. The mechanical guarantee ("the engine is not called on
a hit") is `tests/test_verify_cache.py`; this script is the human-facing
acceptance check for "同一spec・同一オプションでの再実行が明確に高速化される".

Usage:
    python tools/bench_verify_cache.py [spec.fsl ...] [--depth N] [--runs N]
"""
from __future__ import annotations

import argparse
import shutil
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from fslc.cli import run_verify  # noqa: E402


DEFAULT_SPECS = [
    ROOT / "specs" / "order_workflow.fsl",
    ROOT / "specs" / "bank.fsl",
    ROOT / "specs" / "cart_v1.fsl",
]


def _time_run(path: Path, depth: int, use_cache: bool) -> float:
    start = time.perf_counter()
    out = run_verify(str(path), depth, "ignore", use_cache=use_cache)
    elapsed = time.perf_counter() - start
    assert out.get("result") in {"verified", "proved", "violated", "reachable_failed", "unknown_cti"}, out
    return elapsed


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("specs", nargs="*", type=Path, default=None)
    ap.add_argument("--depth", type=int, default=8)
    ap.add_argument("--runs", type=int, default=3)
    args = ap.parse_args()

    specs = args.specs or DEFAULT_SPECS
    cache_dir = Path(tempfile.mkdtemp(prefix="fslc-bench-cache-"))
    import os
    os.environ["FSLC_CACHE"] = "on"
    os.environ["FSLC_CACHE_DIR"] = str(cache_dir)

    try:
        print(f"{'spec':<30} {'cold (s)':>12} {'warm avg (s)':>14} {'speedup':>10}")
        for spec in specs:
            cold = _time_run(spec, args.depth, use_cache=True)
            warm_times = [_time_run(spec, args.depth, use_cache=True) for _ in range(args.runs)]
            warm_avg = sum(warm_times) / len(warm_times)
            speedup = cold / warm_avg if warm_avg > 0 else float("inf")
            print(f"{spec.name:<30} {cold:>12.4f} {warm_avg:>14.4f} {speedup:>9.1f}x")
    finally:
        shutil.rmtree(cache_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
