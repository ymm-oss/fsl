# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Emit concise, repository-backed context for Codex SessionStart hooks."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


MAX_TASK_BYTES = 12_000
MAX_TASK_LINES = 220
MAX_STATUS_LINES = 40


def git(root: Path, *args: str) -> str:
    """Run a read-only Git command and return trimmed output."""
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else "unavailable"


def repository_root(cwd: Path) -> Path:
    """Resolve the repository root, falling back to the hook's project root."""
    result = subprocess.run(
        ["git", "-C", str(cwd), "rev-parse", "--show-toplevel"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return Path(result.stdout.strip())
    return Path(__file__).resolve().parents[2]


def task_context(root: Path) -> str:
    """Read the active task while bounding injected context."""
    task_file = root / "tasks" / "active.md"
    if not task_file.exists():
        return "No active task packet. Invoke `$task-start` before substantial work."

    raw = task_file.read_bytes()
    text = raw.decode("utf-8", errors="replace")
    if len(raw) <= MAX_TASK_BYTES:
        return text.rstrip()

    excerpt = "\n".join(text.splitlines()[:MAX_TASK_LINES])
    return (
        f"{excerpt}\n\n"
        "[Task packet truncated by SessionStart hook; inspect tasks/active.md directly.]"
    )


def main() -> int:
    """Render the session source, Git state, and durable task packet."""
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError):
        payload = {}

    cwd = Path(payload.get("cwd") or Path.cwd()).resolve()
    root = repository_root(cwd)
    source = payload.get("source", "unknown")
    branch = git(root, "branch", "--show-current") or "detached HEAD"
    status_lines = git(root, "status", "--short").splitlines()
    status = "\n".join(status_lines[:MAX_STATUS_LINES]) or "clean"
    if len(status_lines) > MAX_STATUS_LINES:
        status += f"\n... {len(status_lines) - MAX_STATUS_LINES} more entries"

    print("# Runtime Context")
    print(f"Session source: {source}")
    print(f"Branch: {branch}")
    print("\n## Working Tree")
    print(status)
    print("\n## Active Task State")
    print(task_context(root))
    return 0


if __name__ == "__main__":
    sys.exit(main())
