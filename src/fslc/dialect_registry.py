# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Token-aware dialect selection for the retained Python compatibility surface."""

from dataclasses import dataclass
from typing import Optional


DIALECT_KEYWORDS = (
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

if len(DIALECT_KEYWORDS) != len(set(DIALECT_KEYWORDS)):
    raise RuntimeError("duplicate FSL dialect registry keyword")


@dataclass(frozen=True)
class DialectDispatch:
    keyword: Optional[str]
    source: str


def inspect_source(source: str) -> DialectDispatch:
    """Return the first declaration keyword and a position-preserving parser view.

    The native Rust lexer is authoritative. This retained adapter mirrors its
    BOM/comment/top-level-annotation significance rules so the frozen Python CLI
    picks the same frontend without copying raw ``lstrip().startswith`` branches.
    """

    chars = list(source)
    cursor = 0
    if chars[:1] == ["\ufeff"]:
        chars[0] = " "
        cursor = 1
    cursor = _skip_trivia(source, cursor)
    requirements: dict[str, Optional[str]] = {}
    while cursor < len(source) and source[cursor] == "@":
        scanned = _annotation(source, cursor)
        if scanned is None:
            return DialectDispatch(keyword=None, source="".join(chars))
        end, path, arguments = scanned
        if path == "requirement":
            requirement_id = arguments[0][1]
            text = arguments[1][1] if len(arguments) == 2 else None
            if requirement_id.lower() == "undecided" or (
                requirement_id in requirements and requirements[requirement_id] != text
            ):
                return DialectDispatch(keyword=None, source="".join(chars))
            requirements[requirement_id] = text
        for index in range(cursor, end):
            if chars[index] not in "\r\n":
                chars[index] = " "
        cursor = _skip_trivia(source, end)
    start = cursor
    while cursor < len(source) and (source[cursor].isalnum() or source[cursor] == "_"):
        cursor += 1
    keyword = source[start:cursor] or None
    if keyword not in DIALECT_KEYWORDS:
        keyword = None
    return DialectDispatch(keyword=keyword, source="".join(chars))


def dialect_keyword(source: str) -> Optional[str]:
    return inspect_source(source).keyword


def _skip_trivia(source: str, cursor: int) -> int:
    while True:
        while cursor < len(source) and source[cursor].isspace():
            cursor += 1
        if source.startswith("//", cursor):
            newline = source.find("\n", cursor + 2)
            cursor = len(source) if newline < 0 else newline + 1
            continue
        return cursor


def _ident_end(source: str, cursor: int) -> Optional[int]:
    if cursor >= len(source) or not (
        source[cursor].isascii()
        and (source[cursor].isalpha() or source[cursor] == "_")
    ):
        return None
    cursor += 1
    while cursor < len(source) and source[cursor].isascii() and (
        source[cursor].isalnum() or source[cursor] == "_"
    ):
        cursor += 1
    return cursor


def _symbol_path(source: str, cursor: int) -> Optional[tuple[int, str]]:
    end = _ident_end(source, cursor)
    if end is None:
        return None
    segments = [source[cursor:end]]
    while True:
        dot = _skip_trivia(source, end)
        if dot >= len(source) or source[dot] != ".":
            return end, ".".join(segments)
        cursor = _skip_trivia(source, dot + 1)
        end = _ident_end(source, cursor)
        if end is None:
            return None
        segments.append(source[cursor:end])


def _annotation_value(source: str, cursor: int) -> Optional[tuple[int, str, str]]:
    if cursor >= len(source):
        return None
    if source[cursor] == '"':
        end = source.find('"', cursor + 1)
        if end < 0 or "\n" in source[cursor + 1 : end]:
            return None
        return end + 1, "string", source[cursor + 1 : end]
    if source[cursor].isascii() and source[cursor].isdigit():
        end = cursor + 1
        while end < len(source) and source[end].isascii() and source[end].isdigit():
            end += 1
        text = source[cursor:end]
        if int(text) > 9_223_372_036_854_775_807:
            return None
        return end, "integer", text
    path = _symbol_path(source, cursor)
    if path is None:
        return None
    end, text = path
    if text.split(".", 1)[0] in ("true", "false"):
        if "." in text:
            return None
        return end, "boolean", text
    return end, "symbol", text


def _annotation(
    source: str, cursor: int
) -> Optional[tuple[int, str, list[tuple[str, str]]]]:
    cursor = _skip_trivia(source, cursor + 1)
    parsed_path = _symbol_path(source, cursor)
    if parsed_path is None:
        return None
    path_end, path = parsed_path
    cursor = _skip_trivia(source, path_end)
    if cursor >= len(source) or source[cursor] != "(":
        return None
    cursor = _skip_trivia(source, cursor + 1)
    arguments: list[tuple[str, str]] = []
    while cursor < len(source) and source[cursor] != ")":
        value = _annotation_value(source, cursor)
        if value is None:
            return None
        cursor, kind, text = value
        arguments.append((kind, text))
        cursor = _skip_trivia(source, cursor)
        if cursor < len(source) and source[cursor] == ")":
            break
        if cursor >= len(source) or source[cursor] != ",":
            return None
        cursor = _skip_trivia(source, cursor + 1)
        if cursor >= len(source) or source[cursor] == ")":
            return None
    if cursor >= len(source) or source[cursor] != ")":
        return None
    if path in ("requirement", "kind"):
        if len(arguments) not in (1, 2) or any(kind != "string" for kind, _ in arguments):
            return None
    elif path == "undecided":
        if len(arguments) != 1 or arguments[0][0] != "string":
            return None
    if path in ("requirement", "kind", "undecided") and not arguments[0][1].strip():
        return None
    return cursor + 1, path, arguments
