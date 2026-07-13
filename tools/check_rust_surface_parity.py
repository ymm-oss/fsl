# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare Rust shared surface ASTs with the Python parser corpus."""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from functools import lru_cache
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.export_ast import corpus_paths, export_file  # noqa: E402


DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fsl-parse-surface.exe" if os.name == "nt" else "fsl-parse-surface")
)


def first_difference(expected: Any, actual: Any, path: str = "$") -> dict[str, Any]:
    if type(expected) is not type(actual):
        return {"json_path": path, "python": expected, "rust": actual}
    if isinstance(expected, list):
        if len(expected) != len(actual):
            return {
                "json_path": path,
                "python_length": len(expected),
                "rust_length": len(actual),
            }
        for index, (left, right) in enumerate(zip(expected, actual)):
            if left != right:
                return first_difference(left, right, f"{path}[{index}]")
    elif isinstance(expected, dict):
        if expected.keys() != actual.keys():
            return {
                "json_path": path,
                "python_keys": sorted(expected),
                "rust_keys": sorted(actual),
            }
        for key in expected:
            if expected[key] != actual[key]:
                return first_difference(expected[key], actual[key], f"{path}.{key}")
    return {"json_path": path, "python": expected, "rust": actual}


SUPPORTED_TOP_LEVELS = frozenset(
    {"business", "compose", "governance", "refinement", "requirements", "spec"}
)
SUPPORTED_SPECIALIZED_FRONTENDS = frozenset({"ai-component", "db", "domain"})


@lru_cache(maxsize=4)
def _surface_entries(root: Path) -> tuple[tuple[Path, dict[str, Any]], ...]:
    return tuple(
        (path, export_file(path, root=root, stage="surface"))
        for path in corpus_paths(root)
    )


@lru_cache(maxsize=8)
def surface_cases(
    root: Path,
    top_levels: frozenset[str] = SUPPORTED_TOP_LEVELS,
) -> list[tuple[Path, dict[str, Any]]]:
    cases = []
    for path, exported in _surface_entries(root):
        if (
            exported["status"] == "ok"
            and exported["frontend"] == "shared"
            and exported["ast"][0] in top_levels
        ):
            cases.append((path, exported))
    return cases


@lru_cache(maxsize=4)
def spec_cases(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    return surface_cases(root, frozenset({"spec"}))


@lru_cache(maxsize=8)
def specialized_cases(
    root: Path,
    frontends: frozenset[str] = SUPPORTED_SPECIALIZED_FRONTENDS,
) -> list[tuple[Path, dict[str, Any]]]:
    return [
        (path, exported)
        for path, exported in _surface_entries(root)
        if exported["status"] == "ok" and exported["frontend"] in frontends
    ]


@lru_cache(maxsize=4)
def surface_error_cases(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    cases = []
    for path, exported in _surface_entries(root):
        if exported["status"] == "error":
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
            "kind": "rust_parse_error",
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
            "kind": "ast_mismatch",
            **first_difference(expected["ast"], actual),
        }
    return None


def compare_error(
    path: Path,
    expected: dict[str, Any],
    rust_bin: Path,
) -> dict[str, Any] | None:
    proc = subprocess.run(
        [str(rust_bin), str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    rel = path.relative_to(ROOT).as_posix()
    if proc.returncode != 2:
        return {
            "path": rel,
            "kind": "expected_rust_parse_error",
            "returncode": proc.returncode,
            "stdout": proc.stdout.strip(),
            "stderr": proc.stderr.strip(),
        }
    match = re.search(r" at (\d+):(\d+)$", proc.stderr.strip())
    if match is None:
        return {"path": rel, "kind": "missing_rust_error_location", "stderr": proc.stderr.strip()}
    actual_loc = {"line": int(match.group(1)), "column": int(match.group(2))}
    if expected["error"]["kind"] != "parse" or actual_loc != expected["error"]["loc"]:
        return {
            "path": rel,
            "kind": "parse_error_mismatch",
            "python": expected["error"],
            "rust": {"kind": "parse", "loc": actual_loc},
        }
    return None


def run(root: Path, rust_bin: Path) -> dict[str, Any]:
    cases = [*surface_cases(root), *specialized_cases(root)]
    error_cases = surface_error_cases(root)
    failures = [
        failure
        for path, expected in cases
        if (failure := compare(path, expected, rust_bin)) is not None
    ]
    failures.extend(
        failure
        for path, expected in error_cases
        if (failure := compare_error(path, expected, rust_bin)) is not None
    )
    return {
        "schema": "fsl-rust-ast-parity.v1",
        "scope": "surface",
        "top_levels": sorted(SUPPORTED_TOP_LEVELS),
        "specialized_frontends": sorted(SUPPORTED_SPECIALIZED_FRONTENDS),
        "cases": len(cases) + len(error_cases),
        "valid_cases": len(cases),
        "error_cases": len(error_cases),
        "matched": len(cases) + len(error_cases) - len(failures),
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    if not args.rust_bin.is_file():
        parser.error(f"Rust parser binary not found: {args.rust_bin}; run cargo build first")
    result = run(args.root, args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
