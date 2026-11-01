#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check quoted DESIGN document section citations against ATX headings."""

from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


SCOPES = (".github", "tools", "rust", "docs", "skills")
EXCLUDED_PATHS = {Path("CHANGELOG.md")}
DESIGN_PATH = r"`?(docs/design/DESIGN-[A-Za-z0-9._-]+\.md)`?"
QUOTED_TITLE = r'["“]([^"”\n]+)["”]'
CITATION_PATTERNS = (
    re.compile(
        DESIGN_PATH
        + r"(?:'s|’s)?"
        + r"(?:\s*,\s*|\s*:\s*|\s*\(\s*|\s+section\s+|\s+)"
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
    files = sorted(
        Path(name)
        for name in names
        if name and Path(name) not in EXCLUDED_PATHS
    )
    if not files:
        return files
    # Git's binary macro unsets text. Only an explicit -text declaration
    # excludes input: text=auto/unspecified must still decode strictly as UTF-8.
    try:
        attributes = subprocess.run(
            ["git", "check-attr", "-z", "--stdin", "text"],
            input=b"".join(path.as_posix().encode("utf-8") + b"\0" for path in files),
            cwd=root, check=False, capture_output=True, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AuditError(f"cannot query Git text attributes: {error}") from error
    if attributes.returncode != 0:
        detail = attributes.stderr.decode("utf-8", errors="replace").strip()
        raise AuditError(
            f"git check-attr failed with exit {attributes.returncode}: "
            f"{detail or '(no stderr)'}"
        )
    try:
        records = attributes.stdout.decode("utf-8").split("\0")
    except UnicodeDecodeError as error:
        raise AuditError(f"git returned non-UTF-8 attributes: {error}") from error
    if len(records) != 3 * len(files) + 1 or records[-1] != "":
        raise AuditError("git check-attr returned malformed records")
    text_files: list[Path] = []
    for index, path in enumerate(files):
        name, attribute, value = records[3 * index:3 * index + 3]
        if name != path.as_posix() or attribute != "text":
            raise AuditError("git check-attr returned unexpected path or attribute")
        if value != "unset":
            text_files.append(path)
    return text_files


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
    audited_files = sorted(set(files))
    audited_file_set = set(audited_files)
    citations: list[Citation] = []
    for relative in audited_files:
        citations.extend(citations_in(relative, read_utf8(root, relative)))
    citations.sort()

    cache: dict[str, set[str]] = {}
    findings: list[Finding] = []
    for citation in citations:
        design_relative = Path(citation.design_path)
        if design_relative not in audited_file_set:
            findings.append(Finding(citation, "design document is not tracked"))
            continue
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


def binary_selftest(repository_root: Path) -> list[bool]:
    """Exercise the real tracked-file/attribute/CLI path in an isolated index."""
    scratch = repository_root / ".codex/tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    checks: list[bool] = []
    with tempfile.TemporaryDirectory(prefix="design-citation-", dir=scratch) as temp:
        root = Path(temp)
        for directory in ("tools", "docs/design"):
            (root / directory).mkdir(parents=True)
        (root / ".gitattributes").write_text(
            '* text=auto\n"tools/binary data\\nfixture" binary\n'
            'tools/nontext -text\ntools/explicit-text text\n',
            encoding="utf-8",
        )
        (root / "tools/binary data\nfixture").write_bytes(b"\xff\x00")
        (root / "tools/nontext").write_bytes(b"\xff\x00")
        design = "docs/design/DESIGN-selftest.md"
        (root / design).write_text("## Existing heading\n", encoding="utf-8")
        source = root / "tools/reference.zlib"
        source.write_text(f'{design}: "Existing heading"\n', encoding="utf-8")
        for name in ("unmarked", "explicit-text"):
            (root / "tools" / name).write_text("plain text\n", encoding="utf-8")
        for command in (["git", "init", "--quiet"], ["git", "add", "--", "."]):
            result = subprocess.run(
                command, cwd=root, check=False, capture_output=True, timeout=30
            )
            if result.returncode != 0:
                raise AuditError(f"selftest Git setup failed: {result.stderr!r}")

        def expect(label: str, code: int, detail: str) -> None:
            result = subprocess.run(
                [sys.executable, str(Path(__file__).resolve()), "check", str(root)],
                check=False, capture_output=True, timeout=30,
            )
            output = (result.stdout + result.stderr).decode("utf-8")
            checks.append(assert_selftest(label + " exit", result.returncode, code))
            checks.append(assert_selftest(label + " diagnostic", detail in output, True))

        expect("declared binary without UTF-8 decode", 0, "citations=1 findings=0")
        for name in ("unmarked", "explicit-text"):
            malformed = root / "tools" / name
            malformed.write_bytes(b"\xff")
            expect(name + " malformed UTF-8", 2, f"cannot read UTF-8 file tools/{name}")
            malformed.write_text("plain text\n", encoding="utf-8")
        # The same extension as the real binary fixtures is still citation prose
        # unless Git explicitly declares it nontext.
        source.write_text(f'{design}: "Missing heading"\n', encoding="utf-8")
        expect("bad heading beside binary", 1, "quoted heading does not exist")
        untracked_design = "docs/design/DESIGN-untracked.md"
        source.write_text(f'{untracked_design}: "Ambient heading"\n', encoding="utf-8")
        (root / untracked_design).write_text(
            "## Ambient heading\n", encoding="utf-8"
        )
        expect("real untracked target beside binary", 1, "design document is not tracked")
        source.unlink()
        expect(
            "missing tracked text beside binary", 2,
            "cannot read UTF-8 file tools/reference.zlib",
        )
    return checks


def selftest(repository_root: Path) -> int:
    fixtures = repository_root / "tests/fixtures/design-citation-headings"
    accepting_citations, accepting_findings = fixture_audit(fixtures / "accepting")
    stale_citations, stale_findings = fixture_audit(fixtures / "stale-435b0f6")
    separator_citations, separator_findings = fixture_audit(
        fixtures / "stale-new-separators"
    )
    untracked_root = fixtures / "untracked-design"
    untracked_source = Path("tools/untracked-target-reference.txt")
    untracked_citations, untracked_findings = audit(
        untracked_root, [untracked_source]
    )

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
            ("Merge readiness contract", "tools/colon-reference.txt"),
            ("Merge readiness contract", "tools/section-word-reference.txt"),
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
    expected_separator_findings = [
        (
            "tools/broken-separators.txt",
            1,
            "Colon heading removed",
            "quoted heading does not exist",
        ),
        (
            "tools/broken-separators.txt",
            2,
            "Section-word heading removed",
            "quoted heading does not exist",
        ),
    ]
    produced_separator_findings = [
        (
            finding.citation.source.as_posix(),
            finding.citation.line,
            finding.citation.title,
            finding.reason,
        )
        for finding in separator_findings
    ]
    expected_untracked_findings = [
        (
            "tools/untracked-target-reference.txt",
            1,
            "Ambient-only heading",
            "design document is not tracked",
        )
    ]
    produced_untracked_findings = [
        (
            finding.citation.source.as_posix(),
            finding.citation.line,
            finding.citation.title,
            finding.reason,
        )
        for finding in untracked_findings
    ]
    checks = (
        assert_selftest("accepting citations", produced_accepting, expected_accepting),
        assert_selftest("accepting findings", accepting_findings, []),
        assert_selftest("historical CHANGELOG exclusion", len(accepting_citations), 7),
        assert_selftest("435b0f6 citations", len(stale_citations), 4),
        assert_selftest("435b0f6 exact findings", produced_stale, expected_stale),
        assert_selftest(
            "435b0f6 reasons",
            {finding.reason for finding in stale_findings},
            {"quoted heading does not exist"},
        ),
        assert_selftest("new separator citations", len(separator_citations), 2),
        assert_selftest(
            "new separator exact findings",
            produced_separator_findings,
            expected_separator_findings,
        ),
        assert_selftest("untracked target citations", len(untracked_citations), 1),
        assert_selftest(
            "untracked target exact findings",
            produced_untracked_findings,
            expected_untracked_findings,
        ),
        *binary_selftest(repository_root),
    )
    if not all(checks):
        return 1
    print(
        "selftest: PASS: produced=7 accepting/0 findings, "
        "3 issue citations/3 exact stale findings, 2 new-separator findings, "
        "and 1 untracked-target finding; expected=same; "
        "6 Git binary/text/heading/target/I/O controls matched expected exits 0/2/2/1/1/2"
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
