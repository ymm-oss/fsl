# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""PostToolUse hook: run ``fslc check`` on an edited ``.fsl`` file.

This is the fast inner-loop signal the repo relies on (parse + type check), not the
slow full ``pytest`` suite. Reads the Claude Code hook JSON from stdin; if the
edited/written file is a ``.fsl`` spec, it runs the *working-tree* verifier
(``.venv/bin/python -m fslc check``) on it. On failure it forwards the fslc output to
stderr and exits 2 so Claude sees the diagnostics and can repair. On success — or when
there is no working-tree venv (e.g. CI) or the file is not a ``.fsl`` — it exits 0
quietly.

Note: the global ``fslc`` on PATH points at ``~/.fsl``, a different tree, so this hook
deliberately uses the repo venv's interpreter.
"""
import json
import os
import subprocess
import sys


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    path = (data.get("tool_input") or {}).get("file_path") or ""
    if not path.endswith(".fsl"):
        return 0
    root = data.get("cwd") or os.getcwd()
    py = os.path.join(root, ".venv", "bin", "python")
    if not os.path.exists(py):
        return 0  # no working-tree venv; nothing to check against
    proc = subprocess.run(
        [py, "-m", "fslc", "check", path],
        capture_output=True,
        text=True,
        cwd=root,
    )
    if proc.returncode != 0:
        sys.stderr.write(
            "fslc check failed for {}:\n{}{}".format(path, proc.stdout, proc.stderr)
        )
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
