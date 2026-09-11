#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Reject direct edits to the generated frozen-Python corpus snapshot."""

from __future__ import annotations

import argparse
import os
from pathlib import PurePath
import sys


TARGET = PurePath("tests/snapshots/corpus_snapshot.json")


def is_protected_path(raw_path: str) -> bool:
    """Recognize the repository-relative generated snapshot on any platform."""
    normalized = raw_path.replace("\\", "/")
    return PurePath(normalized).as_posix().endswith(TARGET.as_posix())


def rejection_message() -> str:
    return (
        f"Refusing to hand-edit the corpus snapshot ({TARGET.as_posix()}).\n"
        "Regenerate it only for an intended compatibility-contract change (review the diff first):\n"
        "  FSLC_SNAPSHOT_UPDATE=1 .venv/bin/python -m pytest tests/test_corpus_snapshot.py -q"
    )


def check_paths(paths: list[str]) -> int:
    if any(is_protected_path(raw_path) for raw_path in paths):
        print(rejection_message(), file=sys.stderr)
        return 2
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()
    return check_paths(args.paths)


if __name__ == "__main__":
    sys.exit(main())
