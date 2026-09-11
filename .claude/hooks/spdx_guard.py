# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Adapt the shared SPDX detector to Claude PostToolUse."""

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
    raw_path = (data.get("tool_input") or {}).get("file_path") or ""
    if not raw_path:
        return 0
    root = Path(os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()).resolve()
    result = subprocess.run(
        [sys.executable, str(root / "tools" / "check_spdx_headers.py"), "paths", raw_path],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.stderr:
        sys.stderr.write(result.stderr)
    return 2 if result.returncode else 0


if __name__ == "__main__":
    sys.exit(main())
