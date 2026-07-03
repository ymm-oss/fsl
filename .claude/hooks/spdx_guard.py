# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""PostToolUse hook: require an SPDX header on written ``.py`` files.

Mirrors the repo convention (CONTRIBUTING.md / CLAUDE.md): new Python source files
carry ``# SPDX-License-Identifier: Apache-2.0`` plus a copyright line. Reads the Claude
Code hook JSON from stdin; if a written ``.py`` file lacks the SPDX line near its top,
it exits 2 with a reminder so Claude adds the header. Otherwise exits 0.
"""
import json
import sys


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    path = (data.get("tool_input") or {}).get("file_path") or ""
    if not path.endswith(".py"):
        return 0
    try:
        with open(path, "r", encoding="utf-8", errors="ignore") as fh:
            head = fh.read(1000)
    except OSError:
        return 0
    if "SPDX-License-Identifier" in head:
        return 0
    sys.stderr.write(
        "{}: missing SPDX header. Add these two lines at the very top:\n".format(path)
        + "# SPDX-License-Identifier: Apache-2.0\n"
        + "# Copyright <year> <name>\n"
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
