# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""PostToolUse hook: run native Rust ``fslc check`` on an edited FSL file."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def project_root(data: dict) -> Path:
    configured = os.environ.get("CLAUDE_PROJECT_DIR")
    return Path(configured or data.get("cwd") or os.getcwd()).resolve()


def native_check_command(root: Path, path: str) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(root / "rust" / "Cargo.toml"),
        "-p",
        "fslc-rust",
        "--bin",
        "fslc",
        "--",
        "check",
        path,
    ]


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    path = (data.get("tool_input") or {}).get("file_path") or ""
    if not path.endswith(".fsl"):
        return 0
    root = project_root(data)
    if shutil.which("cargo") is None or not (root / "rust" / "Cargo.toml").is_file():
        sys.stderr.write(
            "Native FSL check unavailable: install Cargo and verify rust/Cargo.toml exists.\n"
        )
        return 2
    proc = subprocess.run(
        native_check_command(root, path),
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        details = (proc.stdout + proc.stderr)[-12000:]
        sys.stderr.write(f"native fslc check failed for {path}:\n{details}")
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
