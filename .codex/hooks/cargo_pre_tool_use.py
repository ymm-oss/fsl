# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Rewrite Bash commands mentioning Cargo through the shared lock wrapper."""

from __future__ import annotations

import json
from pathlib import Path
import re
import shlex
import sys


CARGO_WORD = re.compile(r"(?<![A-Za-z0-9_./-])cargo(?![A-Za-z0-9_./-])")
WRAPPER_TIMEOUT_SECONDS = 3_600


def mentions_cargo(command: str) -> bool:
    """Conservatively recognize a Cargo word in shell text before it executes."""
    return bool(CARGO_WORD.search(command))


def rewritten_command(command: str, cwd: Path) -> str:
    """Return a shell-safe invocation of the wrapper around the original command."""
    wrapper = Path(__file__).with_name("cargo_lock.py")
    return " ".join(
        [
            "python3",
            shlex.quote(str(wrapper)),
            "--cwd",
            shlex.quote(str(cwd)),
            "--timeout",
            str(WRAPPER_TIMEOUT_SECONDS),
            "--",
            shlex.quote(command),
        ]
    )


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return 0
    command = (payload.get("tool_input") or {}).get("command")
    if not isinstance(command, str) or not mentions_cargo(command):
        return 0
    cwd = Path(payload.get("cwd") or Path.cwd()).resolve()
    output = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": {"command": rewritten_command(command, cwd)},
        }
    }
    json.dump(output, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
