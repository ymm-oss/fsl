#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Closed-schema adapter for the opt-in Referance domain-generation probe."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


INPUT_VERSION = "referance.transpilation.input.v1"
OUTPUT_VERSION = "referance.transpilation.output.v1"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--implementation", choices=("python", "rust"), required=True)
    parser.add_argument("--mutation", choices=("none", "exit-code"), default="none")
    return parser


def _load_cases() -> list[dict[str, str]]:
    payload = json.load(sys.stdin)
    if set(payload) != {"schema_version", "cases"}:
        raise ValueError("probe input must contain exactly schema_version and cases")
    if payload["schema_version"] != INPUT_VERSION:
        raise ValueError("unsupported probe input schema")
    cases = payload["cases"]
    if not isinstance(cases, list) or not cases:
        raise ValueError("cases must be a non-empty array")
    for case in cases:
        if not isinstance(case, dict) or set(case) != {"id", "spec", "target"}:
            raise ValueError("each case must contain exactly id, spec, and target")
        if not all(isinstance(case[key], str) and case[key] for key in case):
            raise ValueError("case fields must be non-empty strings")
    return cases


def _command(implementation: str, case: dict[str, str]) -> list[str]:
    arguments = [
        "domain",
        "generate",
        case["spec"],
        "--target",
        case["target"],
    ]
    if implementation == "python":
        # The adapter itself is launched with Referance's private venv via
        # ``{python}``.  The frozen FSL reference belongs to this repository,
        # so run it with the repository's ordinary Python instead of leaking
        # Referance's dependency environment across the comparison boundary.
        return [os.environ.get("FSL_PYTHON", "python3"), "-m", "fslc", *arguments]
    return [
        "cargo",
        "run",
        "--quiet",
        "--locked",
        "--manifest-path",
        "rust/Cargo.toml",
        "-p",
        "fslc-rust",
        "--bin",
        "fslc",
        "--",
        *arguments,
    ]


def _observation(
    implementation: str, case: dict[str, str], mutation: str
) -> dict[str, object]:
    environment = dict(os.environ)
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    if implementation == "python":
        source = str(Path.cwd() / "src")
        current = environment.get("PYTHONPATH")
        environment["PYTHONPATH"] = source if not current else f"{source}{os.pathsep}{current}"
    completed = subprocess.run(
        _command(implementation, case),
        capture_output=True,
        check=False,
        env=environment,
        text=True,
    )
    stdout = _encode_stdout(completed.stdout)
    exit_code = completed.returncode + (1 if mutation == "exit-code" else 0)
    return {"exit_code": exit_code, "stdout": stdout}


def _encode_stdout(raw: str) -> dict[str, object]:
    """Retain the complete public stdout value without a field projection."""
    try:
        return {"format": "json", "value": json.loads(raw)}
    except json.JSONDecodeError:
        return {"format": "raw", "value": raw}


def main() -> int:
    args = _parser().parse_args()
    cases = _load_cases()
    output = {
        "schema_version": OUTPUT_VERSION,
        "results": [
            {
                "id": case["id"],
                "observation": _observation(args.implementation, case, args.mutation),
            }
            for case in cases
        ],
    }
    json.dump(output, sys.stdout, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
