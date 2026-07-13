# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""SessionStart hook: inject small, current, reconstructable project state."""

import json
import os
from pathlib import Path
import subprocess
import sys


def project_root(data: dict) -> Path:
    """Resolve the trusted project root without depending on the launch directory."""
    configured = os.environ.get("CLAUDE_PROJECT_DIR")
    return Path(configured or data.get("cwd") or os.getcwd()).resolve()


def git_output(root: Path, *args: str) -> str:
    """Return a bounded Git query result, failing closed to an empty string."""
    try:
        proc = subprocess.run(
            ["git", *args],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return proc.stdout.strip() if proc.returncode == 0 else ""


def build_context(root: Path) -> str:
    """Build concise context; the active packet body remains lazy-loaded."""
    branch = git_output(root, "branch", "--show-current") or "(detached or unavailable)"
    status_lines = git_output(root, "status", "--short").splitlines()[:40]
    status = "\n".join(status_lines) if status_lines else "clean"
    active = root / ".claude" / "work" / "active.md"
    task = (
        "Active task packet: .claude/work/active.md. Read and reconcile it before resuming."
        if active.is_file()
        else "No active task packet. Use /task-start before substantial work."
    )
    return f"Current branch: {branch}\nWorking tree:\n{status}\n{task}"


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        data = {}
    output = {
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": build_context(project_root(data)),
        }
    }
    json.dump(output, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
