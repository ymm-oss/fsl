# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Stable argparse help rendering for generated public contracts."""
from __future__ import annotations

import argparse
import re


class StableHelpFormatter(argparse.HelpFormatter):
    """Freeze argparse presentation across supported Python releases."""

    def __init__(self, prog: str):
        super().__init__(prog, width=80, max_help_position=24)

    def _format_action_invocation(self, action):
        if not action.option_strings:
            return super()._format_action_invocation(action)
        if action.nargs == 0:
            return ", ".join(action.option_strings)
        default = self._get_default_metavar_for_optional(action)
        arguments = self._format_args(action, default)
        aliases = [*action.option_strings[:-1], f"{action.option_strings[-1]} {arguments}"]
        return ", ".join(aliases)


def normalize_argparse_help(text: str) -> str:
    """Normalize known cosmetic argparse differences across Python 3.9+."""
    text = text.replace("optional arguments:", "options:")
    lines = text.splitlines()
    try:
        usage_end = lines.index("")
    except ValueError:
        usage_end = len(lines)
    if usage_end:
        usage = " ".join(line.strip() for line in lines[:usage_end])
        # Python 3.14 appends an explicit `...` to subparser choices while
        # earlier supported versions do not. The choices already express the
        # same positional command slot, so omit this cosmetic suffix.
        usage = re.sub(r"(\}) \.\.\.(?= |$)", r"\1", usage)
        lines[:usage_end] = [usage]
    alias = re.compile(
        r"^(\s+)(-[A-Za-z0-9]) (\S+), (--[A-Za-z0-9-]+) \3(\s{2,}.*)?$"
    )
    normalized = []
    for line in lines:
        match = alias.match(line)
        if match:
            indent, short, metavar, long, description = match.groups()
            line = f"{indent}{short}, {long} {metavar}{description or ''}"
        normalized.append(line)
    return "\n".join(normalized) + ("\n" if text.endswith("\n") else "")


def canonical_help(parser: argparse.ArgumentParser) -> str:
    """Return stable help for one parser without changing its CLI semantics."""
    parser.formatter_class = StableHelpFormatter
    return normalize_argparse_help(parser.format_help())
