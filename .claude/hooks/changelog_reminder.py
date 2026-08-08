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
    # A notable change now lands as a new changelog.d/ fragment
    # (docs/DESIGN-changelog-fragments.md), not a direct CHANGELOG.md edit;
    # CHANGELOG.md itself is aggregated from fragments only at release time,
    # so treating it as the only signal here would make this reminder fire
    # on every fragment-only change -- the routine false positive the
    # decision's reversal condition (a) treats as grounds for no-go.
    changelog_changed = any(
        path == "CHANGELOG.md" or path.startswith("changelog.d/") for path in files
    )
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
            "Reminder: product source changed but no changelog.d/ fragment or "
            "CHANGELOG.md change was found. Add a focused "
            "changelog.d/<id>-<slug>.<category>.md fragment (see "
            "changelog.d/README.md).\n"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
