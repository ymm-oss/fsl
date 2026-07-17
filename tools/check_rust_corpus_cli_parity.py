# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare stable Python/Rust CLI verdicts across the complete FSL corpus."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fslc.exe" if os.name == "nt" else "fslc")
)
STABLE_KEYS = ("result", "spec", "kind", "violation_kind", "invariant")


def corpus(root: Path) -> list[Path]:
    return sorted({*root.glob("specs/**/*.fsl"), *root.glob("examples/**/*.fsl")})


def invoke(command: list[str]) -> tuple[int, dict[str, Any]]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        output = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"invalid JSON from {command!r}: {completed.stderr.strip()}"
        ) from error
    return completed.returncode, output


def projection(output: dict[str, Any]) -> dict[str, Any]:
    return {key: output.get(key) for key in STABLE_KEYS}


def run(root: Path, rust_binary: Path, depth: int) -> dict[str, Any]:
    failures: list[dict[str, Any]] = []
    files = corpus(root)
    for path in files:
        relative = path.relative_to(root).as_posix()
        for operation, trailing in (
            ("check", []),
            ("verify", ["--depth", str(depth)]),
        ):
            arguments = [operation, str(path), *trailing]
            python_status, python_output = invoke(
                [sys.executable, "-m", "fslc", *arguments]
            )
            rust_status, rust_output = invoke([str(rust_binary), *arguments])
            expected = projection(python_output)
            actual = projection(rust_output)
            if (python_status, expected) != (rust_status, actual):
                failures.append(
                    {
                        "file": relative,
                        "operation": operation,
                        "python": {"status": python_status, "projection": expected},
                        "rust": {"status": rust_status, "projection": actual},
                    }
                )
    return {
        "result": "ok" if not failures else "mismatch",
        "files": len(files),
        "commands": len(files) * 2,
        "depth": depth,
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--depth", type=int, default=3)
    arguments = parser.parse_args()
    report = run(ROOT, arguments.rust_bin.resolve(), arguments.depth)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["result"] == "ok" else 1


if __name__ == "__main__":
    raise SystemExit(main())
