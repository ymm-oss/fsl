# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Block only confirmed changelog violations after a Codex file edit."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys


CHECKER_UNAVAILABLE = 3


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    try:
        result = subprocess.run(
            [str(root / "tools" / "aggregate_changelog.sh"), "check"],
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
    if result.returncode == CHECKER_UNAVAILABLE:
        sys.stderr.write("changelog-advisory-unavailable: checker could not run; edit not blocked.\n")
    else:
        sys.stderr.write(
            "changelog-advisory-unavailable: checker failed unexpectedly; edit not blocked.\n"
        )
    sys.stderr.write(detail)
    return 0


if __name__ == "__main__":
    sys.exit(main())
