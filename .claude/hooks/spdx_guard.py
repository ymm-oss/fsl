# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""PostToolUse hook: require repository SPDX headers on newly written source."""

import json
from pathlib import Path
import sys


SOURCE_SUFFIXES = {".py", ".rs", ".js", ".mjs", ".ts", ".sh"}


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    raw_path = (data.get("tool_input") or {}).get("file_path") or ""
    path = Path(raw_path)
    if path.suffix not in SOURCE_SUFFIXES:
        return 0
    try:
        head = path.read_text(encoding="utf-8", errors="ignore")[:1200]
    except OSError:
        return 0
    if "SPDX-License-Identifier: Apache-2.0" not in head:
        prefix = "#" if path.suffix in {".py", ".sh"} else "//"
        sys.stderr.write(
            f"{path}: missing SPDX header. Add near the top:\n"
            f"{prefix} SPDX-License-Identifier: Apache-2.0\n"
        )
        return 2
    if path.suffix == ".py" and "Copyright" not in head:
        sys.stderr.write(f"{path}: new Python source also needs a copyright line.\n")
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
