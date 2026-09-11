# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Adapt the common generated-snapshot detector to Codex PreToolUse."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return 0
    command = (payload.get("tool_input") or {}).get("command")
    if not isinstance(command, str):
        return 0
    root = Path(__file__).resolve().parents[2]
    result = subprocess.run(
        [sys.executable, str(root / "tools" / "check_generated_snapshot.py"), command],
        check=False,
        capture_output=True,
        text=True,
        cwd=root,
    )
    if result.returncode == 0:
        return 0
    reason = result.stderr.strip() or "direct generated snapshot edit blocked"
    output = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }
    json.dump(output, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
