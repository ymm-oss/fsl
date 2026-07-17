# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Stop hook: remind when product source changed without the changelog."""

import json
import os
from pathlib import Path
import subprocess
import sys


def needs_reminder(files: list[str]) -> bool:
    product_changed = any(
        path.startswith("rust/") or path.startswith("src/fslc/") for path in files
    )
    changelog_changed = any(path == "CHANGELOG.md" for path in files)
    return product_changed and not changelog_changed


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        data = {}
    if data.get("stop_hook_active"):
        return 0
    root = Path(
        os.environ.get("CLAUDE_PROJECT_DIR") or data.get("cwd") or os.getcwd()
    ).resolve()
    try:
        proc = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return 0
    files = [line[3:].strip() for line in proc.stdout.splitlines() if line.strip()]
    if needs_reminder(files):
        sys.stderr.write(
            "Reminder: product source changed but CHANGELOG.md did not. "
            "Add a focused entry under ## [Unreleased].\n"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
