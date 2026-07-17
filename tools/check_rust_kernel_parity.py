# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare direct Rust kernel lowering with Python ``parse_src`` output."""
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

from tools.check_rust_surface_parity import first_difference, surface_cases  # noqa: E402
from tools.export_ast import export_file  # noqa: E402


DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-parse-kernel.exe" if os.name == "nt" else "fsl-parse-kernel")
)


def direct_spec_cases(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    cases = []
    for path, _surface in surface_cases(root, frozenset({"spec"})):
        exported = export_file(path, root=root, stage="kernel")
        if exported["status"] != "ok":
            raise AssertionError(
                f"direct spec left the valid kernel corpus: {exported['path']} "
                f"({exported.get('error')})"
            )
        cases.append((path, exported))
    return cases


def kernel_cases(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    cases = []
    for path, _surface in surface_cases(root, frozenset({"spec", "compose"})):
        exported = export_file(path, root=root, stage="kernel")
        if exported["status"] != "ok":
            raise AssertionError(
                f"kernel input left the valid corpus: {exported['path']} "
                f"({exported.get('error')})"
            )
        cases.append((path, exported))
    return cases


def compare(path: Path, expected: dict[str, Any], rust_bin: Path) -> dict[str, Any] | None:
    proc = subprocess.run(
        [str(rust_bin), str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    rel = path.relative_to(ROOT).as_posix()
    if proc.returncode != 0:
        return {
            "path": rel,
            "kind": "rust_lowering_error",
            "returncode": proc.returncode,
            "stderr": proc.stderr.strip(),
        }
    try:
        actual = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return {"path": rel, "kind": "rust_json_error", "error": str(exc)}
    if actual != expected["ast"]:
        return {
            "path": rel,
            "kind": "kernel_ast_mismatch",
            **first_difference(expected["ast"], actual),
        }
    return None


def run(root: Path, rust_bin: Path) -> dict[str, Any]:
    cases = kernel_cases(root)
    failures = [
        failure
        for path, expected in cases
        if (failure := compare(path, expected, rust_bin)) is not None
    ]
    return {
        "schema": "fsl-rust-kernel-parity.v1",
        "scope": "spec-and-compose-kernel-lowering",
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    if not args.rust_bin.is_file():
        parser.error(f"Rust kernel binary not found: {args.rust_bin}; run cargo build first")
    result = run(args.root, args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
