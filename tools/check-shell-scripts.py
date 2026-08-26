#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run the repository ShellCheck contract and audit suppression reasons."""

from __future__ import annotations

import argparse
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


def tracked_shell_files(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--", "tools", ".github/scripts"],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"git ls-files failed with exit {result.returncode}: {detail or '(no stderr)'}"
        )
    try:
        names = result.stdout.decode("utf-8").split("\0")
    except UnicodeDecodeError as error:
        raise RuntimeError(f"git returned a non-UTF-8 path: {error}") from error
    allowed_parents = {Path("tools"), Path(".github/scripts")}
    return sorted(
        root / relative
        for name in names
        if name
        for relative in (Path(name),)
        if relative.parent in allowed_parents and relative.suffix == ".sh"
    )


def inventory_findings(discovered: list[Path], tracked: list[Path]) -> list[str]:
    if not tracked:
        return ["tracked shell-script inventory is unexpectedly empty"]
    if not discovered:
        return ["discovered shell-script inventory is unexpectedly empty"]
    missing = sorted(set(tracked) - set(discovered))
    if not missing:
        return []
    return [
        "tracked shell script missing from discovery: " + path.as_posix()
        for path in missing
    ]


def selftest(root: Path) -> int:
    fixture_root = root / "tests/fixtures/shell-script-inventory/accepting"
    tracked = sorted(
        [
            fixture_root / "tools/check-one.sh",
            fixture_root / "tools/run-one.sh",
            fixture_root / ".github/scripts/report-one.sh",
        ]
    )
    discovered = shell_files(fixture_root)
    cases = {
        "complete discovery": (discovered, tracked, []),
        "untracked addition": (
            [*discovered, fixture_root / "tools/untracked-extra.sh"],
            tracked,
            [],
        ),
        "narrowed tools glob": (
            [path for path in discovered if path.name.startswith("check-")],
            tracked,
            ["run-one.sh", "report-one.sh"],
        ),
        "github directory omitted": (
            [path for path in discovered if path.parent.name == "tools"],
            tracked,
            ["report-one.sh"],
        ),
        "empty discovery": ([], tracked, ["unexpectedly empty"]),
        "empty tracked authority": (discovered, [], ["unexpectedly empty"]),
    }
    ok = True
    for label, (case_discovered, case_tracked, expected_fragments) in cases.items():
        produced = inventory_findings(case_discovered, case_tracked)
        fragments_match = all(
            any(fragment in finding for finding in produced)
            for fragment in expected_fragments
        )
        if fragments_match and bool(produced) == bool(expected_fragments):
            continue
        print(
            f"selftest: FAIL: {label}: produced={produced!r} "
            f"expected_fragments={expected_fragments!r}",
            file=sys.stderr,
        )
        ok = False
    if ok:
        print("selftest: PASS: inventory_accepting=2 inventory_rejecting=4")
        return 0
    return 1


def check(root: Path) -> int:
    files = shell_files(root)
    try:
        tracked = tracked_shell_files(root)
    except RuntimeError as error:
        print(f"check-shell-scripts: {error}", file=sys.stderr)
        return 1
    inventory_errors = inventory_findings(files, tracked)
    if inventory_errors:
        print("\n".join(inventory_errors), file=sys.stderr)
        return 1
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command", nargs="?", choices=("check", "selftest"), default="check"
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.command == "selftest":
        return selftest(root)
    return check(root)


if __name__ == "__main__":
    raise SystemExit(main())
