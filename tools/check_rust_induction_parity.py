# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare native and Python k-induction envelopes over focused corpus slices."""
from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

from fslc.cli import run_verify

from check_rust_cli_snapshot import DEFAULT_RUST_BIN
from check_rust_full_envelope import _normalize as _normalize_bmc


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    ("examples/gallery/valid/tiny_turnstile.fsl", 4, 1),
    ("examples/gallery/valid/tiny_traffic_light.fsl", 5, 1),
    ("examples/gallery/valid/tiny_bounded_counter.fsl", 4, 1),
    ("examples/gallery/valid/small_elevator.fsl", 7, 1),
    ("examples/gallery/adversarial/option_struct_set_seq_combo.fsl", 5, 1),
    ("tests/fixtures/rust_port/induction_unknown_cti.fsl", 4, 1),
    ("tests/fixtures/rust_port/induction_unknown_cti.fsl", 4, 3),
    ("tests/fixtures/rust_port/ranked_leadsto.fsl", 5, 1),
    ("tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl", 5, 1),
    ("tests/fixtures/rust_port/ranked_leadsto_unbounded_below.fsl", 5, 1),
    ("specs/cart_buggy.fsl", 5, 1),
)


def _normalize(value: Any, path: str = "$") -> Any:
    if path == "$.cti.states":
        return "<nondeterministic-cti>"
    if path.startswith("$.reachables.") and path.endswith(".witness"):
        return "<replayed-base-witness>"
    if isinstance(value, dict):
        masked = {
            key: _normalize(item, f"{path}.{key}")
            for key, item in value.items()
        }
        return _normalize_bmc(masked, path)
    if isinstance(value, list):
        return [_normalize(item, f"{path}.{index}") for index, item in enumerate(value)]
    return _normalize_bmc(value, path)


def _expected_status(output: dict[str, Any]) -> int:
    if output.get("result") == "error":
        return 2
    if output.get("result") in {"violated", "unknown_cti", "reachable_failed"}:
        return 1
    return 0


def _invoke(binary: Path, path: Path, depth: int, k_ind: int) -> tuple[dict[str, Any], int]:
    proc = subprocess.run(
        [
            str(binary),
            "verify",
            str(path),
            "--depth",
            str(depth),
            "--engine",
            "induction",
            "--k",
            str(k_ind),
            "--deadlock",
            "ignore",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        return json.loads(proc.stdout), proc.returncode
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Rust emitted invalid JSON: {proc.stderr.strip()}") from error


def run(root: Path, binary: Path) -> dict[str, Any]:
    failures = []
    for relative, depth, k_ind in CASES:
        path = root / relative
        python = run_verify(
            path,
            depth,
            "ignore",
            engine="induction",
            k_ind=k_ind,
            use_cache=False,
        )
        try:
            rust, status = _invoke(binary, path, depth, k_ind)
        except RuntimeError as error:
            failures.append({"path": relative, "k": k_ind, "error": str(error)})
            continue
        expected = _normalize(python)
        actual = _normalize(rust)
        expected_status = _expected_status(python)
        if expected != actual or status != expected_status:
            failures.append(
                {
                    "path": relative,
                    "k": k_ind,
                    "python_status": expected_status,
                    "rust_status": status,
                    "python": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-induction-parity.v1",
        "cases": len(CASES),
        "matched": len(CASES) - len(failures),
        "allowlist": {
            "$.cost.elapsed_s": "wall-clock timing",
            "$.cti.states": "nondeterministic induction CTI",
            "$.reachables.*.witness": "nondeterministic replayed BMC base witness",
            "BMC witness paths": "reviewed by the Phase-1 full-envelope and replay gates",
        },
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    result = run(args.root, args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
