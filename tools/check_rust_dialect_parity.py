# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare native and Python business/requirements/governance envelopes."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from fslc.cli import run_check, run_verify

from check_rust_cli_snapshot import DEFAULT_RUST_BIN, _invoke
from check_rust_full_envelope import _diff, _normalize as _normalize_bmc
from check_rust_induction_parity import _normalize as _normalize_induction


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    "examples/e2e/1_business.fsl",
    "examples/e2e/2_requirements.fsl",
    "examples/consulting/governance_controls.fsl",
)
DEPTH = 8


def _normalize(value: Any, *, induction: bool) -> Any:
    if induction:
        return _normalize_induction(value)
    return _normalize_bmc(value)


def run(root: Path, binary: Path) -> dict[str, Any]:
    failures = []
    comparisons = 0
    for relative in CASES:
        path = root / relative
        python_check = _normalize(run_check(path), induction=False)
        rust_check = _normalize(
            _invoke(binary, ["check", str(path)]), induction=False
        )
        comparisons += 1
        differences = _diff(python_check, rust_check)
        if differences:
            failures.append(
                {"path": relative, "command": "check", "differences": differences}
            )

        python_verify = _normalize(
            run_verify(
                path,
                DEPTH,
                "warn",
                engine="induction",
                k_ind=1,
                use_cache=False,
            ),
            induction=True,
        )
        rust_verify = _normalize(
            _invoke(
                binary,
                [
                    "verify",
                    str(path),
                    "--depth",
                    str(DEPTH),
                    "--engine",
                    "induction",
                    "--k",
                    "1",
                    "--deadlock",
                    "warn",
                ],
            ),
            induction=True,
        )
        comparisons += 1
        differences = _diff(python_verify, rust_verify)
        if differences:
            failures.append(
                {
                    "path": relative,
                    "command": "verify --engine induction",
                    "differences": differences,
                }
            )

    return {
        "schema": "fsl-rust-dialect-parity.v1",
        "scope": ["business", "requirements", "governance"],
        "depth": DEPTH,
        "cases": len(CASES),
        "comparisons": comparisons,
        "matched": comparisons - len(failures),
        "allowlist": {
            "$.cost.elapsed_s": "wall-clock timing",
            "induction witnesses": "covered by the induction and replay gates",
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
