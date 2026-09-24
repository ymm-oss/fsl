#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Require an adjacent explanation for ``--deadlock ignore`` in examples."""

from __future__ import annotations

import argparse
from pathlib import Path


NEEDLE = "--deadlock ignore"
MARKER = "deadlock-ignore-rationale:"
MINIMUM_RATIONALE_LENGTH = 20


def rationale_for(lines: list[str], line_number: int) -> str | None:
    """Return the immediately preceding rationale, if it is substantive."""
    if line_number == 0:
        return None
    previous = lines[line_number - 1]
    marker_at = previous.find(MARKER)
    if marker_at < 0:
        return None
    rationale = previous[marker_at + len(MARKER) :].strip()
    return rationale if len(rationale) >= MINIMUM_RATIONALE_LENGTH else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to inspect (default: this script's repository)",
    )
    args = parser.parse_args()
    examples = args.root / "examples"
    if not examples.is_dir():
        parser.error(f"examples directory does not exist: {examples}")

    failures: list[str] = []
    occurrences = 0
    for path in sorted(p for p in examples.rglob("*") if p.is_file()):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for index, line in enumerate(lines):
            if NEEDLE not in line:
                continue
            occurrences += 1
            if rationale_for(lines, index) is None:
                relative = path.relative_to(args.root)
                failures.append(
                    f"{relative}:{index + 1}: {NEEDLE!r} needs an immediately preceding "
                    f"{MARKER} explanation of at least {MINIMUM_RATIONALE_LENGTH} characters"
                )

    if failures:
        print("example deadlock-ignore teaching gate failed:")
        print("\n".join(failures))
        return 1
    print(f"example deadlock-ignore teaching gate passed ({occurrences} occurrence(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
