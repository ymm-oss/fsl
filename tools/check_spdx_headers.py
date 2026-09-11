#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check repository source files for required SPDX headers."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Iterable


SOURCE_SUFFIXES = {".py", ".rs", ".js", ".mjs", ".ts", ".sh"}
SPDX = "SPDX-License-Identifier: Apache-2.0"


def is_source(path: Path) -> bool:
    return path.suffix in SOURCE_SUFFIXES


def findings(paths: Iterable[Path]) -> list[str]:
    result: list[str] = []
    for path in sorted(set(paths)):
        if not is_source(path):
            continue
        try:
            head = path.read_text(encoding="utf-8", errors="ignore")[:1200]
        except OSError as error:
            result.append(f"{path}: cannot read source: {error}")
            continue
        if SPDX not in head:
            result.append(f"{path}: missing SPDX header")
        elif path.suffix == ".py" and "Copyright" not in head:
            result.append(f"{path}: new Python source also needs a copyright line")
    return result


def added_sources(root: Path, base_sha: str, head_sha: str) -> list[Path]:
    result = subprocess.run(
        ["git", "diff", "--diff-filter=A", "--name-only", "-z", f"{base_sha}...{head_sha}"],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace").strip())
    return [
        root / relative
        for name in result.stdout.decode("utf-8").split("\0")
        if name
        for relative in (Path(name),)
        if is_source(relative)
    ]


def changed_sources(root: Path) -> list[Path]:
    commands = (
        ["git", "diff", "--name-only", "-z"],
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
    )
    paths: list[Path] = []
    for command in commands:
        result = subprocess.run(command, cwd=root, check=False, capture_output=True)
        if result.returncode != 0:
            raise RuntimeError(result.stderr.decode("utf-8", errors="replace").strip())
        paths.extend(
            root / relative
            for name in result.stdout.decode("utf-8").split("\0")
            if name
            for relative in (Path(name),)
            if is_source(relative) and (root / relative).is_file()
        )
    return paths


def emit(paths: Iterable[Path]) -> int:
    reported = findings(paths)
    for finding in reported:
        print(f"check-spdx-headers: {finding}", file=sys.stderr)
    if reported:
        return 1
    print("check-spdx-headers: PASS")
    return 0


def selftest() -> int:
    with tempfile.TemporaryDirectory(prefix="fsl-spdx-selftest-") as directory:
        root = Path(directory)
        good = root / "good.py"
        missing = root / "missing.py"
        good.write_text(
            "# SPDX-License-Identifier: Apache-2.0\n# Copyright 2026 FSL Authors\n",
            encoding="utf-8",
        )
        missing.write_text("print('missing')\n", encoding="utf-8")
        good_result = findings([good])
        missing_result = findings([missing])
    if good_result == [] and missing_result == [f"{missing}: missing SPDX header"]:
        print("check-spdx-headers: selftest PASS")
        return 0
    print(
        "check-spdx-headers: selftest FAIL: "
        f"produced_good={good_result!r} produced_missing={missing_result!r}",
        file=sys.stderr,
    )
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("check", "changed", "paths", "selftest"))
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    root = args.root.resolve()
    if args.mode == "selftest":
        return selftest()
    try:
        if args.mode == "check":
            base_sha = os.environ.get("BASE_SHA")
            head_sha = os.environ.get("HEAD_SHA")
            if not base_sha or not head_sha:
                raise RuntimeError("BASE_SHA and HEAD_SHA are required for check")
            return emit(added_sources(root, base_sha, head_sha))
        if args.mode == "changed":
            return emit(changed_sources(root))
    except RuntimeError as error:
        print(f"check-spdx-headers: Git enumeration failed: {error}", file=sys.stderr)
        return 2
    return emit([(root / path).resolve() for path in args.paths])


if __name__ == "__main__":
    sys.exit(main())
