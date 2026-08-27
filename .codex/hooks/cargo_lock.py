# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Serialize Cargo commands for all worktrees of one repository."""

from __future__ import annotations

import argparse
import fcntl
from pathlib import Path
import subprocess
import sys
import time


DEFAULT_TIMEOUT_SECONDS = 3_600.0
LOCK_FILENAME = "fsl-cargo.lock"


def common_directory(cwd: Path) -> Path:
    """Return the absolute Git common directory shared by every worktree."""
    result = subprocess.run(
        ["git", "-C", str(cwd), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0 or not result.stdout.strip():
        detail = result.stderr.strip() or "no Git common directory returned"
        raise RuntimeError(f"cannot resolve Git common directory: {detail}")
    return Path(result.stdout.strip())


def lock_path(cwd: Path) -> Path:
    """Use one advisory-lock inode in the Git common directory, not per worktree."""
    return common_directory(cwd) / LOCK_FILENAME


def acquire(lock_file: Path, timeout_seconds: float) -> object:
    """Acquire an exclusive fcntl lock, waiting up to the configured timeout."""
    handle = lock_file.open("a+", encoding="utf-8")
    deadline = time.monotonic() + timeout_seconds
    while True:
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            return handle
        except BlockingIOError:
            if time.monotonic() >= deadline:
                handle.close()
                raise TimeoutError(
                    f"timed out after {timeout_seconds:g}s waiting for Cargo lock {lock_file}"
                )
            time.sleep(min(0.1, max(0.0, deadline - time.monotonic())))


def run(command: str, cwd: Path, timeout_seconds: float) -> int:
    """Run a shell command while holding the repository-wide Cargo lock."""
    lock_file = lock_path(cwd)
    try:
        handle = acquire(lock_file, timeout_seconds)
    except (OSError, RuntimeError, TimeoutError) as error:
        print(f"cargo-lock: {error}", file=sys.stderr)
        return 2
    try:
        result = subprocess.run(["/bin/bash", "-lc", command], cwd=cwd, check=False)
        return result.returncode
    finally:
        fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        handle.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cwd", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    command_parts = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command_parts:
        parser.error("a command is required after --")
    command = " ".join(command_parts)
    return run(command, args.cwd.resolve(), args.timeout)


if __name__ == "__main__":
    sys.exit(main())
