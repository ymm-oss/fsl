# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare bounded native refinement envelopes with the Python reference."""
from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

from fslc.cli import run_refine

from check_rust_cli_snapshot import DEFAULT_RUST_BIN


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    ("specs/cart_impl.fsl", "specs/cart_v1.fsl", "specs/cart_refines.fsl", 6),
    ("specs/seat_booking_impl.fsl", "specs/seat_booking.fsl", "specs/seat_refines.fsl", 6),
    (
        "examples/gallery/errors/refinement_failed_impl.fsl",
        "examples/gallery/errors/refinement_failed_abs.fsl",
        "examples/gallery/errors/refinement_failed_map.fsl",
        4,
    ),
    (
        "examples/gallery/adversarial/refine_mapping_boundary_impl.fsl",
        "examples/gallery/adversarial/refine_mapping_boundary_abs.fsl",
        "examples/gallery/adversarial/refine_mapping_boundary_map.fsl",
        2,
    ),
    (
        "examples/refinement_liveness/design_drops_liveness.fsl",
        "examples/refinement_liveness/policy.fsl",
        "examples/refinement_liveness/design_drops_liveness_progress_refines.fsl",
        8,
    ),
    (
        "examples/refinement_liveness/design_keeps_liveness.fsl",
        "examples/refinement_liveness/policy.fsl",
        "examples/refinement_liveness/design_keeps_liveness_progress_refines.fsl",
        8,
    ),
)


def _normalize(value: Any, path: str = "$") -> Any:
    if path == "$.impl_trace":
        return "<replayed-implementation-witness>"
    if isinstance(value, dict):
        return {
            key: _normalize(item, f"{path}.{key}")
            for key, item in sorted(value.items())
            if key != "fsl"
        }
    if isinstance(value, list):
        return [_normalize(item, f"{path}.{index}") for index, item in enumerate(value)]
    return value


def _expected_status(output: dict[str, Any]) -> int:
    if output.get("result") == "error":
        return 2
    return 1 if output.get("result") == "refinement_failed" else 0


def _invoke(
    binary: Path, implementation: Path, abstraction: Path, mapping: Path, depth: int
) -> tuple[dict[str, Any], int]:
    proc = subprocess.run(
        [
            str(binary),
            "refine",
            str(implementation),
            str(abstraction),
            str(mapping),
            "--depth",
            str(depth),
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
    for impl_name, abs_name, mapping_name, depth in CASES:
        implementation = root / impl_name
        abstraction = root / abs_name
        mapping = root / mapping_name
        python = run_refine(implementation, abstraction, mapping, depth)
        try:
            rust, status = _invoke(binary, implementation, abstraction, mapping, depth)
        except RuntimeError as error:
            failures.append({"mapping": mapping_name, "error": str(error)})
            continue
        expected = _normalize(python)
        actual = _normalize(rust)
        expected_status = _expected_status(python)
        if expected != actual or status != expected_status:
            failures.append(
                {
                    "mapping": mapping_name,
                    "python_status": expected_status,
                    "rust_status": status,
                    "python": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-refinement-parity.v1",
        "cases": len(CASES),
        "matched": len(CASES) - len(failures),
        "allowlist": {
            "$.impl_trace": "nondeterministic bounded implementation witness",
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
