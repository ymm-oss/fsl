# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compatibility adapter for the native token-based dialect registry.

The Rust ``fsl-syntax`` registry is authoritative.  The retained Python LSP
uses this small scanner only to select its frozen Lark frontend; parity tests
lock the ordered keys and significant-token rules to the native contract.
"""
from __future__ import annotations

from dataclasses import dataclass


SUPPORTED_DIALECT_KEYWORDS = (
    "spec",
    "refinement",
    "compose",
    "business",
    "governance",
    "requirements",
    "domain",
    "dbsystem",
    "ai_component",
    "agent",
)


@dataclass(frozen=True)
class DialectDispatch:
    keyword: str
    declaration_offset: int


def classify_lsp_dialect(source: str) -> DialectDispatch | None:
    """Return the first significant declaration without inspecting raw lines."""
    cursor = _skip_trivia(source, 0)
    while cursor < len(source) and source[cursor] == "@":
        cursor = _skip_annotation(source, cursor)
        if cursor is None:
            return None
        cursor = _skip_trivia(source, cursor)
    start = cursor
    cursor = _skip_ident(source, cursor)
    if cursor == start:
        return None
    keyword = source[start:cursor]
    if keyword not in SUPPORTED_DIALECT_KEYWORDS:
        return None
    return DialectDispatch(keyword=keyword, declaration_offset=start)


def mask_dispatch_prefix(source: str, declaration_offset: int) -> str:
    """Hide BOM/annotations from frozen grammars while preserving locations."""
    prefix = "".join("\n" if char == "\n" else " " for char in source[:declaration_offset])
    return prefix + source[declaration_offset:]


def _skip_trivia(source: str, cursor: int) -> int:
    while True:
        while cursor < len(source) and (source[cursor].isspace() or source[cursor] == "\ufeff"):
            cursor += 1
        if source.startswith("//", cursor):
            newline = source.find("\n", cursor + 2)
            cursor = len(source) if newline < 0 else newline + 1
            continue
        return cursor


def _skip_annotation(source: str, cursor: int) -> int | None:
    cursor += 1
    cursor = _skip_trivia(source, cursor)
    cursor = _skip_symbol_path(source, cursor)
    if cursor is None:
        return None
    cursor = _skip_trivia(source, cursor)
    if cursor >= len(source) or source[cursor] != "(":
        return cursor
    cursor = _skip_trivia(source, cursor + 1)
    if cursor < len(source) and source[cursor] == ")":
        return cursor + 1
    while cursor < len(source):
        cursor = _skip_annotation_value(source, cursor)
        if cursor is None:
            return None
        cursor = _skip_trivia(source, cursor)
        if cursor < len(source) and source[cursor] == ")":
            return cursor + 1
        if cursor >= len(source) or source[cursor] != ",":
            return None
        cursor = _skip_trivia(source, cursor + 1)
    return None


def _skip_annotation_value(source: str, cursor: int) -> int | None:
    if cursor >= len(source):
        return None
    if source[cursor] == '"':
        end = source.find('"', cursor + 1)
        if end < 0 or "\n" in source[cursor + 1:end]:
            return None
        return end + 1
    if source[cursor].isdigit() and source[cursor].isascii():
        start = cursor
        while (
            cursor < len(source)
            and source[cursor].isdigit()
            and source[cursor].isascii()
        ):
            cursor += 1
        if int(source[start:cursor]) > 2**63 - 1:
            return None
        return cursor
    return _skip_symbol_path(source, cursor)


def _skip_symbol_path(source: str, cursor: int) -> int | None:
    end = _skip_ident(source, cursor)
    if end == cursor:
        return None
    cursor = end
    while True:
        after_segment = _skip_trivia(source, cursor)
        if after_segment >= len(source) or source[after_segment] != ".":
            return cursor
        next_start = _skip_trivia(source, after_segment + 1)
        next_end = _skip_ident(source, next_start)
        if next_end == next_start:
            return None
        cursor = next_end


def _skip_ident(source: str, cursor: int) -> int:
    if cursor >= len(source) or not (
        source[cursor].isascii()
        and (source[cursor].isalpha() or source[cursor] == "_")
    ):
        return cursor
    cursor += 1
    while cursor < len(source) and source[cursor].isascii() and (
        source[cursor].isalnum() or source[cursor] == "_"
    ):
        cursor += 1
    return cursor
