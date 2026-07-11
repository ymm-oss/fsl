# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Markdown fenced-FSL sniffing and line-preserving extraction."""
from __future__ import annotations

import re

from .model import FslError


_OPEN = re.compile(r"^[ \t]*```fsl[ \t]*$")
_CLOSE = re.compile(r"^[ \t]*```[ \t]*$")


def is_literate_source(source):
    return any(_OPEN.match(line) for line in source.splitlines())


def extract_literate_source(source):
    """Blank prose/fence lines while preserving code line and column positions."""
    if not is_literate_source(source):
        return source
    output = []
    inside = False
    opening_line = None
    for line_number, line in enumerate(source.splitlines(keepends=True), start=1):
        content = line.rstrip("\r\n")
        newline = line[len(content):]
        if not inside and _OPEN.match(content):
            inside = True
            opening_line = line_number
            output.append(newline)
        elif inside and _CLOSE.match(content):
            inside = False
            opening_line = None
            output.append(newline)
        elif inside:
            output.append(line)
        else:
            output.append(newline)
    if inside:
        raise FslError(
            f"unclosed ```fsl fence opened at line {opening_line}",
            kind="parse",
            loc={"line": opening_line, "column": 1},
        )
    return "".join(output)


class LiterateParser:
    """Parser adapter used by LSP coverage/indexing with original positions."""

    def __init__(self, parser):
        self._parser = parser

    def parse(self, source):
        return self._parser.parse(extract_literate_source(source))
