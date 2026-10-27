#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Require the Windows skip manifest to equal the live long-pole test list."""

import argparse
import sys
from collections import Counter
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list-output", type=Path)
    parser.add_argument("--names", required=True, type=Path)
    parser.add_argument("--emit-skip-args", action="store_true")
    args = parser.parse_args()
    names = Counter(
        line.strip() for line in args.names.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    )
    if args.emit_skip_args:
        sys.stdout.reconfigure(newline="\n")
        print("".join(f"--skip\n{name}\n" for name in names), end="")
        return 0
    if args.list_output is None:
        parser.error("--list-output is required unless --emit-skip-args is used")
    listed = {
        line.removesuffix(": test")
        for line in args.list_output.read_text(encoding="utf-8").splitlines()
        if line.endswith(": test")
    }
    missing, extra = listed - names.keys(), names.keys() - listed
    duplicates = {name for name, count in names.items() if count > 1}
    for label, values in (("missing", missing), ("extra", extra), ("duplicate", duplicates)):
        for name in sorted(values):
            print(f"{label}: {name}")
    return int(bool(missing or extra or duplicates))


if __name__ == "__main__":
    raise SystemExit(main())
