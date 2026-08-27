# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Return canonical changelog-checker feedback after a Codex file edit."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    result = subprocess.run(
        [str(root / "tools" / "aggregate_changelog.sh"), "check"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return 0
    sys.stderr.write(result.stderr or result.stdout)
    return 2


if __name__ == "__main__":
    sys.exit(main())
