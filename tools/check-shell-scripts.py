#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run the repository ShellCheck contract and audit suppression reasons."""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path


SUPPRESSION = re.compile(r"#\s*shellcheck\s+disable=[^#\n]+(?P<reason>#.*)?$")


def shell_files(root: Path) -> list[Path]:
    return sorted((root / "tools").glob("*.sh")) + sorted(
        (root / ".github/scripts").glob("*.sh")
    )


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    files = shell_files(root)
    findings: list[str] = []
    for path in files:
        lines = path.read_text(encoding="utf-8").splitlines()
        for line_number, line in enumerate(lines, 1):
            match = SUPPRESSION.search(line)
            if match and not (match.group("reason") or "").lstrip("# ").strip():
                findings.append(
                    f"{path.relative_to(root)}:{line_number}: "
                    "shellcheck suppression needs an inline reason"
                )
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1

    executable = shutil.which("shellcheck")
    if executable is None:
        print("check-shell-scripts: shellcheck not found", file=sys.stderr)
        return 1
    result = subprocess.run(
        [executable, "--enable=check-extra-masked-returns", *map(str, files)],
        cwd=root,
        check=False,
    )
    if result.returncode != 0:
        return result.returncode
    print(f"check-shell-scripts: PASS -- {len(files)} script(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
