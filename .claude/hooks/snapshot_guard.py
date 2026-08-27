# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Adapt the shared generated-snapshot detector to Claude PreToolUse."""
import json
import os
from pathlib import Path
import subprocess
import sys


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    path = (data.get("tool_input") or {}).get("file_path") or ""
    if not path:
        return 0
    root = Path(os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()).resolve()
    result = subprocess.run(
        [sys.executable, str(root / "tools" / "check_generated_snapshot.py"), path],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.stderr:
        sys.stderr.write(result.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
