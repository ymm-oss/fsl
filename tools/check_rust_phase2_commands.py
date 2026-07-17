# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare Phase-2 sweep and chain command contracts with Python."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from check_rust_cli_snapshot import DEFAULT_RUST_BIN
from check_rust_full_envelope import _diff


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    (
        "sweep",
        [
            "sweep",
            "tests/fixtures/rust_port/sweep_counterexample.fsl",
            "--depth",
            "1..2",
            "--instances",
            "Case=1..2",
            "--property",
            "AtMostOne",
        ],
    ),
    (
        "refine-chain",
        [
            "refine",
            "examples/refinement_chain/bot.fsl",
            "examples/refinement_chain/mid.fsl",
            "examples/refinement_chain/bot_refines_mid.fsl",
            "examples/refinement_chain/top.fsl",
            "examples/refinement_chain/mid_refines_top.fsl",
            "--depth",
            "6",
        ],
    ),
    (
        "project-chain",
        ["chain", "tests/fixtures/chain/fsl-project.toml"],
    ),
    (
        "project-chain-short-circuit",
        ["chain", "tests/fixtures/chain/fsl-project-broken.toml"],
    ),
    (
        "project-chain-keep-going",
        [
            "chain",
            "tests/fixtures/chain/fsl-project-broken.toml",
            "--keep-going",
        ],
    ),
)


def _normalize(value: Any, path: str = "$") -> Any:
    if path.endswith(".cost.elapsed_s"):
        return "<elapsed>"
    if path.endswith(".trace") or path.endswith(".impl_trace") or path.endswith(".witness"):
        return "<replayed-witness>"
    if path.endswith(".last_action"):
        return "<replayed-witness-action>"
    if path.endswith(".blame"):
        return "<witness-blame>"
    if isinstance(value, dict):
        return {
            key: _normalize(item, f"{path}.{key}")
            for key, item in sorted(value.items())
        }
    if isinstance(value, list):
        return [_normalize(item, f"{path}.{index}") for index, item in enumerate(value)]
    return value


def _invoke(executable: list[str], arguments: list[str]) -> tuple[dict[str, Any], int, str]:
    environment = os.environ.copy()
    environment["PYTHONPATH"] = str(ROOT / "src") + os.pathsep + environment.get(
        "PYTHONPATH", ""
    )
    environment["FSLC_CACHE_VERIFY"] = "1"
    process = subprocess.run(
        [*executable, *arguments],
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return json.loads(process.stdout), process.returncode, process.stderr


def run(binary: Path) -> dict[str, Any]:
    failures = []
    for name, arguments in CASES:
        python, python_status, python_stderr = _invoke(
            [sys.executable, "-m", "fslc"], arguments
        )
        rust, rust_status, rust_stderr = _invoke([str(binary)], arguments)
        differences = _diff(_normalize(python), _normalize(rust))
        if python_status != rust_status:
            differences.append(
                {"path": "$.exit_code", "python": python_status, "rust": rust_status}
            )
        if arguments[0] == "chain" and (
            "Layer" not in python_stderr or "Layer" not in rust_stderr
        ):
            differences.append(
                {
                    "path": "$.stderr.table",
                    "python": "Layer" in python_stderr,
                    "rust": "Layer" in rust_stderr,
                }
            )
        if differences:
            failures.append({"case": name, "differences": differences})
    return {
        "schema": "fsl-rust-phase2-command-parity.v1",
        "cases": len(CASES),
        "matched": len(CASES) - len(failures),
        "allowlist": {
            "*.cost.elapsed_s": "wall-clock timing",
            "*.trace|impl_trace|witness": "covered by bidirectional replay gates",
            "*.last_action": "derived from the same replayed nondeterministic witness",
            "*.blame": "derived from nondeterministic witness",
        },
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    result = run(args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
