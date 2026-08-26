#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Require an immediate Bash 4+ guard when repository scripts need it."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


GUARD = re.compile(
    r'^\(\( BASH_VERSINFO\[0\] >= 4 \)\) \|\| \{ echo "[^"]+" >&2; exit 1; \}$'
)
ARRAY_EXPANSION = re.compile(r"\$\{[A-Za-z_][A-Za-z0-9_]*\[@\]\}")
SET_U = re.compile(r"(?m)^\s*set\s+-[^\n#]*u")
MAPFILE = re.compile(r"(?<![A-Za-z0-9_])(mapfile|readarray)(?![A-Za-z0-9_])")
ASSOCIATIVE = re.compile(r"(?m)(?:^|[;&|]\s*)(?:declare|typeset)\s+-[A-Za-z]*A")


def executable_text(source: str) -> str:
    """Erase comments and single-quoted literals while preserving shell structure."""
    out: list[str] = []
    state = "plain"
    escaped = False
    for char in source:
        if state == "comment":
            if char == "\n":
                state = "plain"
                out.append(char)
            else:
                out.append(" ")
            continue
        if state == "single":
            if char == "'":
                state = "plain"
            out.append("\n" if char == "\n" else " ")
            continue
        if escaped:
            escaped = False
            out.append(char)
            continue
        if char == "\\" and state in ("plain", "double"):
            escaped = True
            out.append(char)
        elif char == "'" and state == "plain":
            state = "single"
            out.append(" ")
        elif char == '"':
            state = "plain" if state == "double" else "double"
            out.append(" ")
        elif char == "#" and state == "plain":
            state = "comment"
            out.append(" ")
        else:
            out.append(char)
    return "".join(out)


def required_features(source: str) -> list[str]:
    code = executable_text(source)
    features: list[str] = []
    if MAPFILE.search(code):
        features.append("mapfile/readarray")
    if ASSOCIATIVE.search(code):
        features.append("associative array")
    if SET_U.search(code) and ARRAY_EXPANSION.search(code):
        features.append("array expansion under set -u")
    return features


def audit(root: Path, files: list[Path]) -> list[str]:
    findings: list[str] = []
    for relative in files:
        path = root / relative
        parsed = subprocess.run(
            ["bash", "-n", str(path)], capture_output=True, text=True, check=False
        )
        if parsed.returncode != 0:
            findings.append(
                f"{relative}: bash parser rejected file: {parsed.stderr.strip()}"
            )
            continue
        source = path.read_text(encoding="utf-8")
        features = required_features(source)
        if not features:
            continue
        lines = source.splitlines()
        if len(lines) < 2 or not GUARD.fullmatch(lines[1]):
            findings.append(
                f"{relative}: Bash 4+ feature(s) {', '.join(features)} require "
                "the fail-closed guard immediately after the shebang"
            )
    return findings


def repository_files(root: Path) -> list[Path]:
    return sorted(
        path.relative_to(root)
        for directory in (root / "tools", root / ".github/scripts")
        if directory.is_dir()
        for path in directory.glob("*.sh")
    )


def selftest(root: Path) -> int:
    fixtures = root / "tests/fixtures/bash-version-guards"
    cases = {
        "accepting": [],
        "missing-guard": ["require the fail-closed guard"],
        "late-guard": ["require the fail-closed guard"],
        "quoted-decoy": [],
    }
    ok = True
    for name, expected_fragments in cases.items():
        case_root = fixtures / name
        produced = audit(case_root, [Path("fixture.sh")])
        fragments_match = all(
            any(fragment in item for item in produced)
            for fragment in expected_fragments
        )
        if fragments_match and bool(produced) == bool(expected_fragments):
            continue
        print(
            f"selftest: FAIL: {name}: produced={produced!r} expected_fragments={expected_fragments!r}",
            file=sys.stderr,
        )
        ok = False
    if ok:
        print("selftest: PASS: accepting=2 rejecting=2")
        return 0
    return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("check", "selftest"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.command == "selftest":
        return selftest(root)
    findings = audit(root, repository_files(root))
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("check-bash-version-guards: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
