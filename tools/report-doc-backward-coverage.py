#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Measure the *backward* documentation direction: which FSL elements no
hand-written document discusses.

The forward direction -- every FSL element a document names exists -- is
``tools/check-doc-links.py``.  This is the other one, and issue #1138 is a
spike to *measure* it, not a gate.  Nothing in this repository claims the
backward direction, and running this report does not start claiming it.

What "discussed" means here, from weakest to strongest:

``file``
    some hand-written Markdown names the declaring ``.fsl`` file.  A path with
    a repository prefix (``examples/self/fslc_fold.fsl``, optionally
    ``../``-relative) counts anywhere in the document, in prose and inside
    fenced blocks alike -- that is the convention ``docs/README.md`` states.
    A bare ``fslc_fold.fsl`` also counts, but only in a document whose own
    directory holds that file, which is how ``examples/*/README.md`` names its
    specifications.
``file-nonindex``
    the same, from a document that is not an index: not ``docs/README.md``, and
    not the ``README.md`` sitting in the element's own directory.  An index row
    is cheap to write, so this separates a catalogue entry from a discussion.
``element``
    a hand-written Markdown links the element itself, ``<path>.fsl#kind:name``.
    This is the only level that is about the element rather than about the file
    that happens to contain it, and it is exactly the reference form
    ``check-doc-links.py`` already resolves in the forward direction.

**What is still not claimed.**  A reference is a reference.  That a document
names an element says nothing about whether the surrounding prose is correct,
complete, or current -- that property is issue #1128's subject, not this one.

Usage::

    python3 tools/report-doc-backward-coverage.py report [--json]
    python3 tools/report-doc-backward-coverage.py selftest
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# The same declaration set ``tools/check-doc-links.py`` resolves ``#kind:name``
# against, so the two directions share one notion of "FSL element".
FSL_ELEMENT = re.compile(
    r"^\s*(spec|refinement|domain|dbsystem|action|invariant|trans|forbidden)"
    r"\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)
PREFIXED_PATH = re.compile(
    r"(?:\.\./)*((?:specs|examples|tests|rust|docs|skills)/[A-Za-z0-9_./-]+\.fsl)"
)
ELEMENT_LINK = re.compile(
    r"(?:\.\./)*((?:specs|examples|tests|rust|docs|skills)/[A-Za-z0-9_./-]+\.fsl)"
    r"#([a-z]+:[A-Za-z_][A-Za-z0-9_]*)"
)
BARE_NAME = re.compile(r"(?<![A-Za-z0-9_./-])([A-Za-z0-9_-]+\.fsl)")

#: Layers of the denominator.  Order matters: the first matching rule wins, so
#: every ``.fsl`` in the repository lands in exactly one layer.
LAYERS: list[tuple[str, bool, str]] = [
    (
        "fixtures",
        False,
        "regression fixtures under tests/: owned by the test that reads them, "
        "and that test is itself checked",
    ),
    (
        "specs/",
        False,
        "conformance corpus (docs/DESIGN-conformance-harness.md): written to be "
        "verified, not to be documented -- stated in #1138 itself",
    ),
    (
        "examples/gallery/",
        False,
        "deliberately invalid, adversarial and injected inputs: their meaning is "
        "'must be rejected', so a per-element rationale is an anti-goal",
    ),
    (
        "docs/intro/specs/",
        False,
        "payloads embedded in the generated manual site: content of a generated "
        "page, not a separately documentable element",
    ),
    (
        "examples/self/",
        True,
        "self-specifications of fslc's own shipped behaviour: exactly the case "
        "#1138 describes, where a behaviour lands and the rationale is never written",
    ),
    (
        "examples/",
        True,
        "the repository's hand-maintained showcase: an example nobody discusses "
        "is an example that teaches nobody",
    ),
]


def layer_of(relative: Path) -> tuple[str, bool, str]:
    text = relative.as_posix()
    if "fixtures" in relative.parts:
        return LAYERS[0]
    for layer in LAYERS[1:]:
        if text.startswith(layer[0]):
            return layer
    return ("other", False, "not part of any authored FSL layer")


@dataclass(frozen=True, order=True)
class Element:
    path: str
    element: str


def fsl_files() -> list[Path]:
    return sorted(p for p in ROOT.rglob("*.fsl") if ".git" not in p.parts)


def elements_of(path: Path) -> list[str]:
    source = path.read_text(encoding="utf-8")
    seen: list[str] = []
    for kind, name in FSL_ELEMENT.findall(source):
        token = f"{kind}:{name}"
        if token not in seen:
            seen.append(token)
    return seen


def hand_written_markdown() -> list[str]:
    """Every tracked Markdown file a human wrote.

    Excluded, because nobody writes them: ``CHANGELOG.md`` and
    ``changelog.d/`` (assembled from fragments), and anything under a
    ``fixtures``/``snapshots`` directory (inputs to a test).
    """
    listed = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "*.md"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    kept = []
    for name in listed:
        parts = Path(name).parts
        if name == "CHANGELOG.md" or parts[0] == "changelog.d":
            continue
        if "fixtures" in parts or "snapshots" in parts:
            continue
        kept.append(name)
    return sorted(kept)


def references(documents: list[str]) -> tuple[dict[str, set[str]], dict[Element, set[str]]]:
    """Which documents name each ``.fsl`` file, and each ``kind:name`` element."""
    by_file: dict[str, set[str]] = {}
    by_element: dict[Element, set[str]] = {}
    for name in documents:
        source = (ROOT / name).read_text(encoding="utf-8")
        for target in PREFIXED_PATH.findall(source):
            by_file.setdefault(target, set()).add(name)
        for target, element in ELEMENT_LINK.findall(source):
            by_element.setdefault(Element(target, element), set()).add(name)
        directory = Path(name).parent
        for bare in BARE_NAME.findall(source):
            candidate = directory / bare
            if (ROOT / candidate).is_file():
                by_file.setdefault(candidate.as_posix(), set()).add(name)
    return by_file, by_element


def is_index(document: str, element_path: str) -> bool:
    """An index is a catalogue, not a discussion."""
    if document == "docs/README.md":
        return True
    return document == (Path(element_path).parent / "README.md").as_posix()


def measure() -> dict:
    documents = hand_written_markdown()
    by_file, by_element = references(documents)
    layers: dict[str, dict] = {}
    for path in fsl_files():
        relative = path.relative_to(ROOT)
        name, in_scope, reason = layer_of(relative)
        bucket = layers.setdefault(
            name,
            {
                "in_scope": in_scope,
                "reason": reason,
                "files": 0,
                "elements": 0,
                "file": 0,
                "file_nonindex": 0,
                "element": 0,
                "undiscussed": [],
                "unnamed_files": [],
                "index_only_files": [],
            },
        )
        bucket["files"] += 1
        key = relative.as_posix()
        citing = by_file.get(key, set())
        non_index = {d for d in citing if not is_index(d, key)}
        if not citing:
            bucket["unnamed_files"].append(key)
        elif not non_index:
            bucket["index_only_files"].append(key)
        for element in elements_of(path):
            bucket["elements"] += 1
            if citing:
                bucket["file"] += 1
            if non_index:
                bucket["file_nonindex"] += 1
            if Element(key, element) in by_element:
                bucket["element"] += 1
            else:
                bucket["undiscussed"].append(f"{key}#{element}")
    return {"documents": len(documents), "layers": layers}


def report(show_undiscussed: int) -> int:
    result = measure()
    layers = result["layers"]
    print(f"hand-written Markdown documents read: {result['documents']}")
    print()
    header = f"{'layer':22s} {'scope':>6s} {'files':>6s} {'elems':>6s} {'file':>6s} {'!index':>7s} {'elem':>6s}"
    print(header)
    print("-" * len(header))
    for name, _, _ in LAYERS:
        bucket = layers.get(name)
        if bucket is None:
            continue
        print(
            f"{name:22s} {('IN' if bucket['in_scope'] else 'out'):>6s} "
            f"{bucket['files']:6d} {bucket['elements']:6d} "
            f"{bucket['file']:6d} {bucket['file_nonindex']:7d} {bucket['element']:6d}"
        )
    print()
    for name, _, reason in LAYERS:
        bucket = layers.get(name)
        if bucket is None or bucket["in_scope"]:
            continue
        print(f"excluded  {name:22s} {reason}")
    print()
    scoped = [b for b in layers.values() if b["in_scope"]]
    total = sum(b["elements"] for b in scoped)
    discussed = sum(b["element"] for b in scoped)
    by_file_level = sum(b["file"] for b in scoped)
    non_index = sum(b["file_nonindex"] for b in scoped)
    print(f"denominator (in scope): {total} elements in {sum(b['files'] for b in scoped)} files")
    print(f"  discussed at element level : {discussed:5d}   undiscussed {total - discussed}")
    print(f"  discussed at file level    : {by_file_level:5d}   undiscussed {total - by_file_level}")
    print(f"  ... from a non-index doc   : {non_index:5d}   undiscussed {total - non_index}")
    unnamed = sorted(f for b in scoped for f in b["unnamed_files"])
    index_only = sorted(f for b in scoped for f in b["index_only_files"])
    print(f"  files no hand-written document names : {len(unnamed)}")
    for name in unnamed:
        print(f"    unnamed {name}")
    print(f"  files only an index names            : {len(index_only)}")
    for name in index_only:
        print(f"    index-only {name}")
    if show_undiscussed:
        print()
        rows = sorted(r for b in scoped for r in b["undiscussed"])
        for row in rows[:show_undiscussed]:
            print(f"  undiscussed(element) {row}")
        if len(rows) > show_undiscussed:
            print(f"  ... and {len(rows) - show_undiscussed} more")
    return 0


def emit_json() -> int:
    result = measure()
    for bucket in result["layers"].values():
        for key in ("undiscussed", "unnamed_files", "index_only_files"):
            bucket[key] = sorted(bucket[key])
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


SELFTEST_LAYERS = {
    "specs/cart_v1.fsl": "specs/",
    "examples/self/fslc_fold.fsl": "examples/self/",
    "examples/gallery/valid/x.fsl": "examples/gallery/",
    "examples/nfr/sla.fsl": "examples/",
    "docs/intro/specs/x.fsl": "docs/intro/specs/",
    "rust/fslc/tests/fixtures/x.fsl": "fixtures",
    "tests/fixtures/chain/x.fsl": "fixtures",
}


def selftest() -> int:
    failures = []
    for path, expected in SELFTEST_LAYERS.items():
        actual = layer_of(Path(path))[0]
        if actual != expected:
            failures.append(f"layer_of({path!r}) == {actual!r}, expected {expected!r}")
    # A bare name must resolve against the citing document's own directory.
    by_file, by_element = references(["examples/self/README.md"])
    if "examples/self/fslc_fold.fsl" not in by_file:
        failures.append("examples/self/README.md should name fslc_fold.fsl by its bare name")
    if by_element:
        failures.append(f"examples/self/README.md should carry no element link, got {by_element}")
    for message in failures:
        print(message, file=sys.stderr)
    checks = len(SELFTEST_LAYERS) + 2
    print(f"selftest: {checks - len(failures)}/{checks} cases pass")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("report", help="measure the backward direction")
    run.add_argument("--json", action="store_true", help="emit the full measurement as JSON")
    run.add_argument(
        "--list", type=int, default=0, metavar="N", help="also list N undiscussed elements"
    )
    sub.add_parser("selftest", help="check the layer rules and the bare-name rule")
    args = parser.parse_args(argv)
    if args.command == "selftest":
        return selftest()
    if args.json:
        return emit_json()
    return report(args.list)


if __name__ == "__main__":
    raise SystemExit(main())
