#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check quoted DESIGN document section citations against ATX headings."""

from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


SCOPES = (".github", "tools", "rust", "docs", "skills")
EXCLUDED_PATHS = {Path("CHANGELOG.md")}
DESIGN_PATH = r"`?(docs/DESIGN-[A-Za-z0-9._-]+\.md)`?"
QUOTED_TITLE = r'["“]([^"”\n]+)["”]'
CITATION_PATTERNS = (
    re.compile(
        DESIGN_PATH
        + r"(?:'s|’s)?"
        + r"(?:\s*,\s*|\s*\(\s*|\s+)"
        + QUOTED_TITLE
    ),
    re.compile(QUOTED_TITLE + r"\s+(?:section\s+)?(?:in|of)\s+" + DESIGN_PATH),
)
FOLLOWING_TITLE = re.compile(
    r"\s*(?:(?:,\s*)?(?:and|or)\s+|,\s*)" + QUOTED_TITLE
)
COMMENT_PREFIX = re.compile(r"^\s*(?://!|///|//|#{1,2}|\*)\s?")
MARKDOWN_HEADING = re.compile(r"^#{2,6}\s+(.+?)\s*#*\s*$")


class AuditError(Exception):
    """An invocation or input error that makes the audit indeterminate."""


@dataclass(frozen=True, order=True)
class Citation:
    source: Path
    line: int
    design_path: str
    title: str


@dataclass(frozen=True, order=True)
class Finding:
    citation: Citation
    reason: str


def normalize_title(value: str) -> str:
    """Normalize presentation-only heading syntax symmetrically."""
    value = html.unescape(value)
    value = value.replace("`", "")
    value = " ".join(value.split())
    value = re.sub(r"^\d+(?:\.\d+)*\.?\s+", "", value)
    value = re.sub(r"\s+\(#[0-9]+(?:\s+[A-Z][0-9]+)?\)$", "", value)
    return value


def prose_groups(text: str) -> list[tuple[str, list[int]]]:
    """Join blank-line-delimited prose while retaining a character line map."""
    groups: list[tuple[str, list[int]]] = []
    chunks: list[str] = []
    line_map: list[int] = []

    def flush() -> None:
        if not chunks:
            return
        groups.append(("".join(chunks), list(line_map)))
        chunks.clear()
        line_map.clear()

    for line_number, raw in enumerate(text.splitlines(), 1):
        if not raw.strip():
            flush()
            continue
        content = COMMENT_PREFIX.sub("", raw).strip().replace(r'\"', '"')
        if chunks:
            chunks.append(" ")
            line_map.append(line_number)
        chunks.append(content)
        line_map.extend([line_number] * len(content))
    flush()
    return groups


def citations_in(source: Path, text: str) -> list[Citation]:
    citations: list[Citation] = []
    for joined, line_map in prose_groups(text):
        for pattern_number, pattern in enumerate(CITATION_PATTERNS):
            for match in pattern.finditer(joined):
                if pattern_number == 0:
                    design_path, title = match.group(1), match.group(2)
                else:
                    title, design_path = match.group(1), match.group(2)
                line = line_map[match.start()] if line_map else 1
                citations.append(
                    Citation(source, line, design_path, normalize_title(title))
                )
                if pattern_number != 0:
                    continue
                cursor = match.end()
                while following := FOLLOWING_TITLE.match(joined, cursor):
                    title_start = following.start(1)
                    citations.append(
                        Citation(
                            source,
                            line_map[title_start] if line_map else 1,
                            design_path,
                            normalize_title(following.group(1)),
                        )
                    )
                    cursor = following.end()
    return citations


def read_utf8(root: Path, relative: Path) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise AuditError(f"cannot read UTF-8 file {relative}: {error}") from error


def tracked_files(root: Path) -> list[Path]:
    command = ["git", "ls-files", "-z", "--", *SCOPES]
    try:
        result = subprocess.run(
            command, cwd=root, check=False, capture_output=True, timeout=30
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AuditError(f"cannot enumerate tracked files: {error}") from error
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise AuditError(
            f"git ls-files failed with exit {result.returncode}: {detail or '(no stderr)'}"
        )
    try:
        names = result.stdout.decode("utf-8").split("\0")
    except UnicodeDecodeError as error:
        raise AuditError(f"git returned a non-UTF-8 path: {error}") from error
    return sorted(
        Path(name)
        for name in names
        if name and Path(name) not in EXCLUDED_PATHS
    )


def fixture_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for scope in SCOPES:
        directory = root / scope
        if directory.is_dir():
            files.extend(
                path.relative_to(root)
                for path in directory.rglob("*")
                if path.is_file()
            )
    return sorted(path for path in files if path not in EXCLUDED_PATHS)


def headings_in(root: Path, relative: Path) -> set[str]:
    headings: set[str] = set()
    for line in read_utf8(root, relative).splitlines():
        if match := MARKDOWN_HEADING.match(line):
            headings.add(normalize_title(match.group(1)))
    return headings


def audit(root: Path, files: Iterable[Path]) -> tuple[list[Citation], list[Finding]]:
    citations: list[Citation] = []
    for relative in files:
        citations.extend(citations_in(relative, read_utf8(root, relative)))
    citations.sort()

    cache: dict[str, set[str]] = {}
    findings: list[Finding] = []
    for citation in citations:
        design_relative = Path(citation.design_path)
        if not (root / design_relative).is_file():
            findings.append(Finding(citation, "design document does not exist"))
            continue
        if citation.design_path not in cache:
            cache[citation.design_path] = headings_in(root, design_relative)
        if citation.title not in cache[citation.design_path]:
            findings.append(Finding(citation, "quoted heading does not exist"))
    return citations, sorted(findings)


def fixture_audit(root: Path) -> tuple[list[Citation], list[Finding]]:
    return audit(root, fixture_files(root))


def assert_selftest(
    label: str,
    actual: object,
    expected: object,
) -> bool:
    if actual == expected:
        return True
    print(
        f"selftest: FAIL: {label}: produced={actual!r} expected={expected!r}",
        file=sys.stderr,
    )
    return False


def selftest(repository_root: Path) -> int:
    fixtures = repository_root / "tests/fixtures/design-citation-headings"
    accepting_citations, accepting_findings = fixture_audit(fixtures / "accepting")
    stale_citations, stale_findings = fixture_audit(fixtures / "stale-435b0f6")

    expected_accepting = sorted(
        [
            (
                "Product gate contract",
                ".github/workflows/site-reference-freshness.yml",
            ),
            ("Ruleset drift audit", ".github/workflows/ruleset-drift-audit.yml"),
            (
                "Ruleset drift audit",
                ".github/workflows/site-reference-freshness.yml",
            ),
            ("Sharded pre-merge Linux evidence", ".github/workflows/ci.yml"),
            ("Ruleset drift audit", ".github/workflows/ci.yml"),
        ]
    )
    produced_accepting = sorted(
        (citation.title, citation.source.as_posix()) for citation in accepting_citations
    )
    expected_stale = [
        (".github/workflows/ci.yml", 2, "Merge queue (planned, not yet enabled)"),
        (".github/workflows/ci.yml", 5, "Merge queue (planned, not yet enabled)"),
        (
            "tools/check-product-gate-scope.sh",
            2,
            "Merge queue (planned, not yet enabled)",
        ),
    ]
    produced_stale = [
        (
            finding.citation.source.as_posix(),
            finding.citation.line,
            finding.citation.title,
        )
        for finding in stale_findings
    ]
    checks = (
        assert_selftest("accepting citations", produced_accepting, expected_accepting),
        assert_selftest("accepting findings", accepting_findings, []),
        assert_selftest("historical CHANGELOG exclusion", len(accepting_citations), 5),
        assert_selftest("435b0f6 citations", len(stale_citations), 4),
        assert_selftest("435b0f6 exact findings", produced_stale, expected_stale),
        assert_selftest(
            "435b0f6 reasons",
            {finding.reason for finding in stale_findings},
            {"quoted heading does not exist"},
        ),
    )
    if not all(checks):
        return 1
    print(
        "selftest: PASS: produced=5 accepting/0 findings and "
        "3 issue citations/3 exact stale findings; expected=same"
    )
    return 0


def check(root: Path, list_all: bool) -> int:
    citations, findings = audit(root, tracked_files(root))
    if list_all:
        for citation in citations:
            print(
                f"CITATION {citation.source}:{citation.line}: "
                f"{citation.design_path} -> {citation.title!r}"
            )
    for finding in findings:
        citation = finding.citation
        print(
            f"{citation.source}:{citation.line}: {finding.reason}: "
            f"{citation.design_path} -> {citation.title!r}",
            file=sys.stderr,
        )
    print(f"summary: citations={len(citations)} findings={len(findings)}")
    return 1 if findings else 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("check", "selftest"))
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    parser.add_argument(
        "--list", action="store_true", help="list every recognized citation"
    )
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        if args.command == "selftest":
            return selftest(root)
        return check(root, args.list)
    except AuditError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
