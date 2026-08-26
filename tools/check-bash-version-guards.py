#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Require an immediate Bash 4+ guard when repository scripts need it."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple


GUARD = re.compile(
    r'^\(\( BASH_VERSINFO\[0\] >= 4 \)\) \|\| \{ echo "[^"]+" >&2; exit 1; \}$'
)
ARRAY_EXPANSION = re.compile(r"\$\{[A-Za-z_][A-Za-z0-9_]*\[@\]\}")
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
COMMAND_BOUNDARIES = {"\n", ";", ";;", ";&", ";;&", "&", "&&", "|", "||", "(", ")", "{", "}"}
COMMAND_KEYWORDS = {
    "!",
    "case",
    "do",
    "done",
    "elif",
    "else",
    "esac",
    "fi",
    "for",
    "function",
    "if",
    "in",
    "select",
    "then",
    "time",
    "until",
    "while",
}


class Token(NamedTuple):
    kind: str
    text: str


class LexedShell:
    def __init__(self) -> None:
        self.tokens: list[Token] = []
        self.nested_commands: list[str] = []
        self.has_array_expansion = False


def consume_parenthesized(source: str, start: int) -> tuple[str, int]:
    """Return one parser-validated $(...) or $((...)) body and its end offset."""
    arithmetic = source.startswith("$((", start)
    first_parenthesis = start + 1
    depth = 0
    quote = ""
    escaped = False
    index = first_parenthesis
    while index < len(source):
        char = source[index]
        if escaped:
            escaped = False
        elif char == "\\" and quote != "'":
            escaped = True
        elif quote:
            if char == quote:
                quote = ""
        elif char in "'\"":
            quote = char
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                body_start = start + (3 if arithmetic else 2)
                body_end = index - (1 if arithmetic else 0)
                return source[body_start:body_end], index + 1
        index += 1
    return source[start:], len(source)


def consume_braced(source: str, start: int) -> tuple[str, int]:
    depth = 0
    index = start + 1
    while index < len(source):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1], index + 1
        index += 1
    return source[start:], len(source)


def consume_backticks(source: str, start: int) -> tuple[str, int]:
    index = start + 1
    escaped = False
    while index < len(source):
        char = source[index]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == "`":
            return source[start + 1 : index], index + 1
        index += 1
    return source[start + 1 :], len(source)


def lex_shell(source: str) -> LexedShell:
    """Tokenize enough parsed Bash structure to classify executed simple commands."""
    result = LexedShell()
    word: list[str] = []
    word_active = False
    index = 0

    def flush_word() -> None:
        nonlocal word_active
        if word_active:
            result.tokens.append(Token("word", "".join(word)))
            word.clear()
            word_active = False

    def append_parameter(expansion: str) -> None:
        nonlocal word_active
        word.append(expansion)
        word_active = True
        if ARRAY_EXPANSION.search(expansion):
            result.has_array_expansion = True

    while index < len(source):
        char = source[index]
        if char in " \t\r":
            flush_word()
            index += 1
            continue
        if char == "\n":
            flush_word()
            result.tokens.append(Token("operator", "\n"))
            index += 1
            continue
        if char == "#" and not word_active:
            newline = source.find("\n", index)
            index = len(source) if newline < 0 else newline
            continue
        if char == "\\":
            word_active = True
            if index + 1 < len(source):
                word.append(source[index + 1])
                index += 2
            else:
                index += 1
            continue
        if char == "'":
            word_active = True
            end = source.find("'", index + 1)
            if end < 0:
                word.append(source[index + 1 :])
                break
            word.append(source[index + 1 : end])
            index = end + 1
            continue
        if char == '"':
            word_active = True
            index += 1
            while index < len(source) and source[index] != '"':
                if source[index] == "\\" and index + 1 < len(source):
                    word.append(source[index + 1])
                    index += 2
                elif source.startswith("$((", index):
                    arithmetic, index = consume_parenthesized(source, index)
                    append_parameter(arithmetic)
                    word.append("$((...))")
                elif source.startswith("$(", index):
                    nested, index = consume_parenthesized(source, index)
                    result.nested_commands.append(nested)
                    word.append("$(...)")
                elif source.startswith("${", index):
                    expansion, index = consume_braced(source, index)
                    append_parameter(expansion)
                elif source[index] == "`":
                    nested, index = consume_backticks(source, index)
                    result.nested_commands.append(nested)
                    word.append("`...`")
                else:
                    word.append(source[index])
                    index += 1
            index += index < len(source)
            continue
        if source.startswith("$((", index):
            arithmetic, index = consume_parenthesized(source, index)
            append_parameter(arithmetic)
            word.append("$((...))")
            continue
        if source.startswith("$(", index):
            nested, index = consume_parenthesized(source, index)
            result.nested_commands.append(nested)
            word.append("$(...)")
            word_active = True
            continue
        if source.startswith("${", index):
            expansion, index = consume_braced(source, index)
            append_parameter(expansion)
            continue
        if char == "`":
            nested, index = consume_backticks(source, index)
            result.nested_commands.append(nested)
            word.append("`...`")
            word_active = True
            continue
        if char in ";&|(){}<>":
            flush_word()
            operator = char
            if index + 1 < len(source) and source[index : index + 2] in {
                "&&",
                "||",
                ";;",
                ";&",
                "|&",
                "<<",
                ">>",
                "<&",
                ">&",
            }:
                operator = source[index : index + 2]
                index += 2
                if operator == ";;" and index < len(source) and source[index] == "&":
                    operator = ";;&"
                    index += 1
            else:
                index += 1
            result.tokens.append(Token("operator", operator))
            continue
        word.append(char)
        word_active = True
        index += 1
    flush_word()
    return result


def simple_commands(tokens: list[Token]) -> list[list[str]]:
    commands: list[list[str]] = []
    current: list[str] = []
    expect_function_name = False
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.kind == "operator" and token.text in COMMAND_BOUNDARIES:
            if (
                token.text == "("
                and len(current) == 1
                and index + 1 < len(tokens)
                and tokens[index + 1] == Token("operator", ")")
            ):
                current = []
                index += 2
                continue
            if current:
                commands.append(current)
                current = []
            index += 1
            continue
        if token.kind != "word":
            index += 1
            continue
        if expect_function_name:
            expect_function_name = False
            index += 1
            continue
        if not current and token.text in COMMAND_KEYWORDS:
            expect_function_name = token.text == "function"
            index += 1
            continue
        current.append(token.text)
        index += 1
    if current:
        commands.append(current)
    return commands


def command_word(words: list[str]) -> tuple[str, list[str]]:
    index = 0
    while index < len(words) and ASSIGNMENT.match(words[index]):
        index += 1
    if index >= len(words):
        return "", []
    command = words[index]
    arguments = words[index + 1 :]
    if command == "command" and arguments:
        if any(argument in {"-v", "-V"} for argument in arguments):
            return "", []
        while arguments and arguments[0].startswith("-"):
            arguments = arguments[1:]
        if arguments:
            command = arguments[0]
            arguments = arguments[1:]
    elif command == "builtin" and arguments:
        command = arguments[0]
        arguments = arguments[1:]
    return command, arguments


def enables_nounset(arguments: list[str]) -> bool:
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument == "--":
            return False
        if argument == "-o":
            return index + 1 < len(arguments) and arguments[index + 1] == "nounset"
        if argument.startswith("-") and "u" in argument[1:]:
            return True
        index += 1
    return False


def scan_facts(source: str) -> tuple[set[str], bool, bool]:
    features: set[str] = set()
    has_set_u = False
    has_array_expansion = False
    pending = [source]
    while pending:
        lexed = lex_shell(pending.pop())
        pending.extend(lexed.nested_commands)
        has_array_expansion = has_array_expansion or lexed.has_array_expansion
        for words in simple_commands(lexed.tokens):
            command, arguments = command_word(words)
            if command in {"mapfile", "readarray"}:
                features.add("mapfile/readarray")
            if command in {"declare", "local", "typeset"}:
                for argument in arguments:
                    if argument == "--":
                        break
                    if argument.startswith("-") and "A" in argument[1:]:
                        features.add("associative array")
                        break
            if command == "set":
                has_set_u = has_set_u or enables_nounset(arguments)
    return features, has_set_u, has_array_expansion


def required_features(source: str) -> list[str]:
    found, has_set_u, has_array_expansion = scan_facts(source)
    if has_set_u and has_array_expansion:
        found.add("array expansion under set -u")
    return [
        feature
        for feature in (
            "mapfile/readarray",
            "associative array",
            "array expansion under set -u",
        )
        if feature in found
    ]


FEATURE_CASES = (
    ("mapfile column zero", "mapfile -t values </dev/null\n", ["mapfile/readarray"]),
    ("mapfile indented", "f() {\n  mapfile -t values </dev/null\n}\n", ["mapfile/readarray"]),
    ("readarray command", "readarray -t values </dev/null\n", ["mapfile/readarray"]),
    ("quoted mapfile command", "\"mapfile\" -t values </dev/null\n", ["mapfile/readarray"]),
    ("builtin mapfile command", "builtin mapfile -t values </dev/null\n", ["mapfile/readarray"]),
    (
        "mapfile in command substitution",
        "printf '%s' \"$(mapfile -t values </dev/null)\"\n",
        ["mapfile/readarray"],
    ),
    (
        "mapfile in backticks",
        "printf '%s' \"`mapfile -t values </dev/null`\"\n",
        ["mapfile/readarray"],
    ),
    ("mapfile unquoted argument", "printf '%s' mapfile\n", []),
    ("mapfile single-quoted argument", "printf '%s' 'mapfile -t values'\n", []),
    ("mapfile double-quoted argument", "printf '%s' \"mapfile -t values\"\n", []),
    ("mapfile comment", "# mapfile -t values\nprintf ok\n", []),
    ("mapfile near miss", "mapfile_helper -t values\n", []),
    ("mapfile keyword argument", "printf '%s' if mapfile\n", []),
    ("mapfile function declaration", "mapfile() { printf ok; }\n", []),
    ("declare associative", "declare -A values=()\n", ["associative array"]),
    ("declare associative indented", "f() {\n  declare -A values=()\n}\n", ["associative array"]),
    ("local associative", "f() {\n  local -A values=()\n}\n", ["associative array"]),
    ("typeset associative", "f() {\n  typeset -A values=()\n}\n", ["associative array"]),
    ("combined associative flags", "declare -gA values=()\n", ["associative array"]),
    ("builtin associative declaration", "builtin declare -A values\n", ["associative array"]),
    ("declare single-quoted argument", "printf '%s' 'declare -A values=()'\n", []),
    ("local double-quoted argument", "printf '%s' \"local -A values=()\"\n", []),
    ("associative comment", "# local -A values=()\nprintf ok\n", []),
    ("indexed declaration", "declare -a values=()\n", []),
    ("associative function declaration", "declare() { printf ok; }\n", []),
    (
        "set short nounset",
        "set -u\nvalues=()\nprintf '%s' ${values[@]}\n",
        ["array expansion under set -u"],
    ),
    (
        "set combined nounset",
        "set -euo pipefail\nvalues=()\nprintf '%s' \"${values[@]}\"\n",
        ["array expansion under set -u"],
    ),
    (
        "set named nounset",
        "set -o nounset\nvalues=()\nprintf '%s' \"${values[@]}\"\n",
        ["array expansion under set -u"],
    ),
    ("array expansion without nounset", "values=()\nprintf '%s' \"${values[@]}\"\n", []),
    ("single-quoted array literal", "set -u\nprintf '%s' '${values[@]}'\n", []),
    ("array expansion comment", "set -u\n# printf '%s' \"${values[@]}\"\nprintf ok\n", []),
    (
        "nounset double-quoted argument",
        "printf '%s' \"set -u\"\nvalues=()\nprintf '%s' \"${values[@]}\"\n",
        [],
    ),
    ("nounset disabled", "set +u\nvalues=()\nprintf '%s' \"${values[@]}\"\n", []),
    ("nounset positional argument", "set -- -u\nvalues=()\nprintf '%s' \"${values[@]}\"\n", []),
)


def audit(root: Path, files: list[Path]) -> list[str]:
    findings: list[str] = []
    for relative in files:
        path = root / relative
        parsed = subprocess.run(
            ["bash", "-n", str(path)], capture_output=True, text=True, check=False
        )
        if parsed.returncode != 0:
            findings.append(
                f"{relative}: bash parser rejected file: {parsed.stderr.strip()}"
            )
            continue
        source = path.read_text(encoding="utf-8")
        features = required_features(source)
        if not features:
            continue
        lines = source.splitlines()
        if len(lines) < 2 or not GUARD.fullmatch(lines[1]):
            findings.append(
                f"{relative}: Bash 4+ feature(s) {', '.join(features)} require "
                "the fail-closed guard immediately after the shebang"
            )
    return findings


def repository_files(root: Path) -> list[Path]:
    return sorted(
        path.relative_to(root)
        for directory in (root / "tools", root / ".github/scripts")
        if directory.is_dir()
        for path in directory.glob("*.sh")
    )


def selftest(root: Path) -> int:
    fixtures = root / "tests/fixtures/bash-version-guards"
    cases = {
        "accepting": [],
        "missing-guard": ["require the fail-closed guard"],
        "late-guard": ["require the fail-closed guard"],
        "quoted-decoy": [],
        "double-quoted-decoy": [],
        "indented-declare": ["associative array"],
        "local-associative": ["associative array"],
    }
    ok = True
    for label, source, expected in FEATURE_CASES:
        parsed = subprocess.run(
            ["bash", "-n"], input=source, capture_output=True, text=True, check=False
        )
        if parsed.returncode != 0:
            print(
                f"selftest: FAIL: feature table {label}: bash parser rejected "
                f"source: {parsed.stderr.strip()}",
                file=sys.stderr,
            )
            ok = False
            continue
        produced = required_features(source)
        if produced != expected:
            print(
                f"selftest: FAIL: feature table {label}: produced={produced!r} "
                f"expected={expected!r}",
                file=sys.stderr,
            )
            ok = False
    for name, expected_fragments in cases.items():
        case_root = fixtures / name
        produced = audit(case_root, [Path("fixture.sh")])
        fragments_match = all(
            any(fragment in item for item in produced)
            for fragment in expected_fragments
        )
        if fragments_match and bool(produced) == bool(expected_fragments):
            continue
        print(
            f"selftest: FAIL: {name}: produced={produced!r} "
            f"expected_fragments={expected_fragments!r}",
            file=sys.stderr,
        )
        ok = False
    if ok:
        print(
            f"selftest: PASS: feature_cases={len(FEATURE_CASES)} "
            "guard_accepting=3 guard_rejecting=4"
        )
        return 0
    return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("check", "selftest"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.command == "selftest":
        return selftest(root)
    findings = audit(root, repository_files(root))
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("check-bash-version-guards: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
