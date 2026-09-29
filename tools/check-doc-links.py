#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Check Markdown links to in-repository paths and heading anchors.

Closes rows 3 and 4 of the #1126 gate table: a plain ``[text](other.md)`` link
and a ``[text](other.md#anchor)`` reference are otherwise unchecked.

Scope, deliberately narrow:

* only link targets that name a path inside the repository are resolved;
  ``http(s):``, ``mailto:`` and other schemes are ignored (no network is used)
* a directory target resolves (``docs/README.md`` links ``../specs/``)
* ``#anchor`` is resolved against **GitHub's** heading-slug algorithm, because
  ``docs/`` is read on GitHub.  mystmd slugifies differently -- see
  ``docs/DESIGN-myst-spike.md``
* a ``.fsl`` target may carry a ``#<kind>:<name>`` fragment naming an element
  declared in that specification (``#action:add_to_cart``); it resolves against
  the declarations in the file.  GitHub renders such a link normally and
  ignores the fragment, so the same source is correct in both readers

Usage::

    python3 tools/check-doc-links.py check [PATH ...]   # default: docs/
    python3 tools/check-doc-links.py selftest
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SCOPES = ("docs",)

FENCE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")
CODE_SPAN = re.compile(r"`+[^`\n]*`+")
HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*#*\s*$")
INLINE_LINK = re.compile(r"!?\[(?:[^\]\\]|\\.)*\]\(\s*<?([^)\s<>]+)>?(?:\s+[\"'(][^)]*)?\s*\)")
REF_DEF = re.compile(r"^\s{0,3}\[(?:[^\]\\]|\\.)+\]:\s*<?([^\s<>]+)>?")
HTML_ANCHOR = re.compile(r"""<a\s+(?:[^>]*\s)?(?:id|name)\s*=\s*["']([^"']+)["']""", re.IGNORECASE)
IGNORED_SCHEME = re.compile(r"^(?:[a-zA-Z][a-zA-Z0-9+.-]*:|//)")
FSL_ELEMENT = re.compile(
    r"^\s*(spec|refinement|domain|dbsystem|action|invariant|trans|forbidden)"
    r"\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)


@dataclass(frozen=True, order=True)
class Finding:
    source: str
    line: int
    target: str
    reason: str

    def __str__(self) -> str:
        return f"{self.source}:{self.line}: {self.reason}: {self.target}"


def github_slug(heading: str) -> str:
    """GitHub's anchor slug for an ATX heading's rendered text.

    Lowercase, drop everything that is not a word character, a space or a
    hyphen, then turn spaces into hyphens.  Markdown emphasis, code spans and
    link syntax contribute their text only.
    """
    text = heading
    text = re.sub(r"`+([^`]*)`+", r"\1", text)
    text = re.sub(r"!?\[((?:[^\]\\]|\\.)*)\]\([^)]*\)", r"\1", text)
    # ``*`` and ``~`` fall out with the other punctuation below; ``_`` does not,
    # and a heading naming ``ledger::assurance_token`` needs it kept.
    text = text.strip().lower()
    text = re.sub(r"[^\w\s-]", "", text, flags=re.UNICODE)
    # GitHub replaces each whitespace character, so "a  b" slugs to "a--b".
    return re.sub(r"\s", "-", text)


def fsl_elements_of(path: Path) -> set[str]:
    """Every ``<kind>:<name>`` an FSL specification declares."""
    source = path.read_text(encoding="utf-8")
    return {f"{kind}:{name}" for kind, name in FSL_ELEMENT.findall(source)}


def anchors_of(path: Path) -> set[str]:
    """Every anchor a GitHub reader can target in ``path``."""
    anchors: set[str] = set()
    seen: dict[str, int] = {}
    fence: str | None = None
    for raw in path.read_text(encoding="utf-8").splitlines():
        opener = FENCE.match(raw)
        if fence is not None:
            if opener and raw.strip().startswith(fence):
                fence = None
            continue
        if opener:
            fence = opener.group(1)[0] * 3
            continue
        anchors.update(m.lower() for m in HTML_ANCHOR.findall(raw))
        heading = HEADING.match(raw)
        if not heading:
            continue
        base = github_slug(heading.group(2))
        if not base:
            continue
        count = seen.get(base, 0)
        seen[base] = count + 1
        anchors.add(base if count == 0 else f"{base}-{count}")
    return anchors


def links_of(path: Path) -> Iterator[tuple[int, str]]:
    """Every link target in ``path``, with its 1-based line number."""
    fence: str | None = None
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        opener = FENCE.match(raw)
        if fence is not None:
            if opener and raw.strip().startswith(fence):
                fence = None
            continue
        if opener:
            fence = opener.group(1)[0] * 3
            continue
        line = CODE_SPAN.sub(" ", raw)
        ref = REF_DEF.match(line)
        if ref:
            yield number, ref.group(1)
            continue
        for target in INLINE_LINK.findall(line):
            yield number, target


def check_target(source: Path, line: int, target: str, anchor_cache: dict[Path, set[str]]) -> Finding | None:
    raw = target.strip()
    if not raw or IGNORED_SCHEME.match(raw):
        return None
    path_part, _, anchor = raw.partition("#")
    path_part = path_part.split("?", 1)[0]
    # A bare ``#anchor`` targets the source document itself.
    resolved = source if not path_part else (source.parent / path_part)
    try:
        resolved = resolved.resolve()
    except OSError:
        return Finding(rel(source), line, raw, "unresolvable path")
    if not resolved.exists():
        return Finding(rel(source), line, raw, "no such path in the repository")
    if not anchor:
        return None
    if resolved.is_dir():
        return None
    if resolved.suffix.lower() == ".fsl":
        if resolved not in anchor_cache:
            anchor_cache[resolved] = fsl_elements_of(resolved)
        if unquote(anchor) not in anchor_cache[resolved]:
            return Finding(rel(source), line, raw, "the specification declares no such element")
        return None
    if resolved.suffix.lower() != ".md":
        return None
    if resolved not in anchor_cache:
        anchor_cache[resolved] = anchors_of(resolved)
    if unquote(anchor).lower() not in anchor_cache[resolved]:
        return Finding(rel(source), line, raw, "no heading in the target document makes this anchor")
    return None


def rel(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


def markdown_files(scopes: Iterable[str]) -> list[Path]:
    files: list[Path] = []
    for scope in scopes:
        base = (ROOT / scope).resolve()
        if base.is_file():
            files.append(base)
        else:
            files.extend(sorted(p for p in base.rglob("*.md") if "_build" not in p.parts))
    return files


def audit(scopes: Iterable[str]) -> tuple[list[Finding], int, int]:
    findings: list[Finding] = []
    anchor_cache: dict[Path, set[str]] = {}
    files = markdown_files(scopes)
    links = 0
    for source in files:
        for line, target in links_of(source):
            links += 1
            finding = check_target(source, line, target, anchor_cache)
            if finding is not None:
                findings.append(finding)
    return sorted(findings), len(files), links


SELFTEST_HEADINGS = {
    "5. Semantics": "5-semantics",
    "9. Migration policy": "9-migration-policy",
    "Frozen Python reference (`src/fslc/assurance.py`)": "frozen-python-reference-srcfslcassurancepy",
    "Envelope classifier — `ledger::assurance_token`": "envelope-classifier--ledgerassurance_token",
    "Transition and failure semantics": "transition-and-failure-semantics",
}


def selftest() -> int:
    failures = []
    for heading, expected in SELFTEST_HEADINGS.items():
        actual = github_slug(heading)
        if actual != expected:
            failures.append(f"github_slug({heading!r}) == {actual!r}, expected {expected!r}")
    for message in failures:
        print(message, file=sys.stderr)
    print(f"selftest: {len(SELFTEST_HEADINGS) - len(failures)}/{len(SELFTEST_HEADINGS)} slug cases pass")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check", help="audit Markdown link targets")
    check.add_argument("paths", nargs="*", default=list(DEFAULT_SCOPES))
    sub.add_parser("selftest", help="check the slug algorithm against known headings")
    args = parser.parse_args(argv)

    if args.command == "selftest":
        return selftest()

    findings, files, links = audit(args.paths or DEFAULT_SCOPES)
    for finding in findings:
        print(finding, file=sys.stderr)
    print(f"check-doc-links: {files} files, {links} in-repository link targets, {len(findings)} findings")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
