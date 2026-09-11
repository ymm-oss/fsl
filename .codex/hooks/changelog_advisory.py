# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Block only confirmed changelog violations after a Codex file edit."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys


def _bash_candidates() -> list[Path]:
    override = os.environ.get("CODEX_CHANGELOG_BASH_CANDIDATES")
    if override is not None:
        if override == "":
            return []
        return [Path(path) for path in override.split(":") if path]
    candidates = [Path("/opt/homebrew/bin/bash"), Path("/usr/local/bin/bash")]
    path_bash = shutil.which("bash")
    if path_bash:
        candidates.append(Path(path_bash))
    return candidates


def _bash_major(bash: Path) -> int | None:
    try:
        result = subprocess.run(
            [str(bash), "-c", "echo ${BASH_VERSINFO[0]}"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    try:
        return int(result.stdout.strip())
    except ValueError:
        return None


def _find_bash4() -> Path | None:
    for bash in _bash_candidates():
        if not bash.is_file():
            continue
        major = _bash_major(bash)
        if major is not None and major >= 4:
            return bash
    return None


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    bash4 = _find_bash4()
    if bash4 is None:
        sys.stderr.write(
            "changelog-advisory-unavailable: Bash 4+ not found; edit not blocked\n"
        )
        return 0
    checker = root / "tools" / "aggregate_changelog.sh"
    try:
        result = subprocess.run(
            [str(bash4), str(checker), "check"],
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        sys.stderr.write(f"changelog-advisory-unavailable: {error}; edit not blocked\n")
        return 0
    if result.returncode == 0:
        return 0
    detail = result.stderr or result.stdout or "checker produced no diagnostic"
    if result.returncode == 1:
        sys.stderr.write(
            "changelog-fragment-violation: fix changelog.d/ fragments and rerun "
            "tools/aggregate_changelog.sh check.\n"
        )
        sys.stderr.write(detail)
        return 2
    sys.stderr.write(
        "changelog-advisory-unavailable: checker failed unexpectedly; edit not blocked.\n"
    )
    sys.stderr.write(detail)
    return 0


if __name__ == "__main__":
    sys.exit(main())
