# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare Rust expression ASTs with the authoritative Python parser."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, Sequence

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.export_ast import export_expression  # noqa: E402


DEFAULT_CASES = ROOT / "tests" / "fixtures" / "rust_port" / "expressions.json"
DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-parse-expr.exe" if os.name == "nt" else "fsl-parse-expr")
)


def compare_expression(
    source: str,
    rust_command: Path | Sequence[str],
) -> dict[str, Any] | None:
    expected = export_expression(source)["ast"]
    command = [str(rust_command)] if isinstance(rust_command, Path) else list(rust_command)
    proc = subprocess.run(
        [*command, source],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        env={
            **os.environ,
            "PYTHONPATH": os.pathsep.join(
                filter(None, (str(ROOT), str(ROOT / "src"), os.environ.get("PYTHONPATH")))
            ),
        },
    )
    if proc.returncode != 0:
        return {
            "source": source,
            "kind": "rust_parse_error",
            "returncode": proc.returncode,
            "stderr": proc.stderr.strip(),
        }
    try:
        actual = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return {
            "source": source,
            "kind": "rust_json_error",
            "error": str(exc),
            "stdout": proc.stdout,
        }
    if actual != expected:
        return {
            "source": source,
            "kind": "ast_mismatch",
            "python": expected,
            "rust": actual,
        }
    return None


def run(cases_path: Path, rust_command: Path | Sequence[str]) -> dict[str, Any]:
    cases = json.loads(cases_path.read_text(encoding="utf-8"))
    if not isinstance(cases, list) or not all(isinstance(case, str) for case in cases):
        raise ValueError("expression cases must be a JSON array of strings")
    failures = [
        failure
        for source in cases
        if (failure := compare_expression(source, rust_command)) is not None
    ]
    return {
        "schema": "fsl-rust-ast-parity.v1",
        "scope": "expression",
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, default=DEFAULT_CASES)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    if not args.rust_bin.is_file():
        parser.error(f"Rust parser binary not found: {args.rust_bin}; run cargo build first")
    result = run(args.cases, args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
