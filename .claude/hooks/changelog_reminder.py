# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Stop hook: nudge if src/fslc changed without a CHANGELOG entry.

Non-blocking. When a turn ends, if the working tree has changes under ``src/fslc/`` but
``CHANGELOG.md`` is untouched, it prints a reminder — the repo convention is one bullet
per change under ``## [Unreleased]``. Always exits 0: this nudges, it never blocks the
stop (so it cannot loop).
"""
import json
import os
import subprocess
import sys


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        data = {}
    # Avoid re-entrancy if a prior stop hook already kept the turn alive.
    if data.get("stop_hook_active"):
        return 0
    root = data.get("cwd") or os.getcwd()
    try:
        out = subprocess.run(
            ["git", "status", "--porcelain"],
            capture_output=True,
            text=True,
            cwd=root,
        ).stdout
    except Exception:
        return 0
    files = [line[3:].strip() for line in out.splitlines() if line.strip()]
    touched_src = any(f.startswith("src/fslc/") for f in files)
    touched_changelog = any(f.endswith("CHANGELOG.md") for f in files)
    if touched_src and not touched_changelog:
        sys.stderr.write(
            "Reminder: src/fslc changed but CHANGELOG.md was not. "
            "Add a bullet under ## [Unreleased] (one topic per change).\n"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
