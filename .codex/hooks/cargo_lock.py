# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Serialize Cargo commands for all worktrees of one repository."""

from __future__ import annotations

import argparse
import fcntl
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time


DEFAULT_TIMEOUT_SECONDS = 3_600.0
LOCK_FILENAME = "fsl-cargo.lock"
REENTRANCY_ENV = "FSL_CARGO_LOCK_HELD"


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


def try_acquire(lock_file: Path):
    """Take the lock without waiting.

    Raises BlockingIOError when somebody else holds it and any other OSError when
    locking fails for an unrelated reason (ENOLCK, EBADF, ...).
    """
    handle = lock_file.open("a+", encoding="utf-8")
    try:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BaseException:
        handle.close()
        raise
    return handle


def release(handle) -> None:
    """Give up an acquired lock.

    Unchanged from the pre-#946 behavior: an unlock failure propagates rather
    than being swallowed, exactly as the original inline
    ``fcntl.flock(...); handle.close()`` did.
    """
    fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
    handle.close()


def marker_script(lock_file: Path) -> str:
    """Mark the re-entrant call inside the shell text, after any login profile."""
    return f"export {REENTRANCY_ENV}={shlex.quote(str(lock_file))}\n"


def execute(command: str, cwd: Path, lock_file: Path) -> int:
    """Run the command, handing the re-entrancy marker down to every descendant.

    Returns the shell's returncode unchanged, including a negative value for a
    signalled child -- exactly the pre-#946 behavior. POSIX exit-code
    normalization (128+N) is a separate, out-of-scope concern; see issue #983.
    """
    env = os.environ.copy()
    env[REENTRANCY_ENV] = str(lock_file)
    script = marker_script(lock_file) + command
    result = subprocess.run(["/bin/bash", "-lc", script], cwd=cwd, env=env, check=False)
    return result.returncode


def run(command: str, cwd: Path, timeout_seconds: float) -> int:
    """Run a shell command while holding the repository-wide Cargo lock.

    A call that is already inside this very lock -- marker names lock_path(cwd)
    and somebody really holds it -- runs the command without waiting, otherwise
    the outer holder would be waited on forever by its own descendant.
    """
    lock_file = lock_path(cwd)
    if os.environ.get(REENTRANCY_ENV) == str(lock_file):
        try:
            handle = try_acquire(lock_file)
        except BlockingIOError:
            # Nested inside a live holder (ourselves' parent): pass straight through.
            return execute(command, cwd, lock_file)
        except OSError as error:
            print(f"cargo-lock: {error}", file=sys.stderr)
            return 2
        # Nobody held it: the marker was stale, so take it and become the holder.
        try:
            return execute(command, cwd, lock_file)
        finally:
            release(handle)
    try:
        handle = acquire(lock_file, timeout_seconds)
    except (OSError, RuntimeError, TimeoutError) as error:
        print(f"cargo-lock: {error}", file=sys.stderr)
        return 2
    try:
        return execute(command, cwd, lock_file)
    finally:
        release(handle)


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
