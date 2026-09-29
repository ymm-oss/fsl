#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Check the FSL in Markdown fences that are marked as complete specifications.

Most ``fsl`` fences under ``docs/`` are fragments (a few lines of an action, a
snippet of a state block), so ``fslc check`` on the whole document exits 2.
Inferring "complete" from "checks" is rejected (issue #1137): it cannot tell an
intended fragment from a complete specification that is now broken.  Instead an
author marks a complete specification in the fence's info string::

    ```fsl check
    spec Example { ... }
    ```

GitHub renders this as an ordinary ``fsl`` code block (the extra word is kept
only as a ``data-meta`` attribute, never displayed), and ``fslc``'s literate
extractor keys on the *first* info-string token being ``fsl``, so the marked
fence is still extracted by ``fslc check file.md``.

Rules the gate enforces and reports:

* every marked fence must check; all fences of one document form one
  compilation unit, exactly as ``fslc`` treats a literate ``.md``
* every ``fsl`` fence that is not marked is skipped and **counted**, so the
  unchecked surface is printed rather than assumed to be small
* failures are reported at the line in the Markdown file.  ``fslc`` is handed
  the ``.md`` itself; when a document also holds unmarked fragments, a copy is
  handed instead whose unmarked opening fences are renamed on their own line, so
  the line count never changes (the extracted-temp-file approach measured a
  149-line drift)

Usage::

    python3 tools/check-doc-fences.py check [PATH ...]   # default: docs/
    python3 tools/check-doc-fences.py selftest

``fslc`` is taken from ``--fslc``, then ``$FSLC``, then ``PATH``.  A missing
binary is an error, never a silent pass.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SCOPES = ("docs",)
MARKER = "check"
FRAGMENT_LANG = "fsl-fragment"


@dataclass(frozen=True)
class Fence:
    line: int  # 1-based line of the opening fence
    marked: bool


def _opening(trimmed: str) -> tuple[str, int, str] | None:
    if not trimmed or trimmed[0] not in "`~":
        return None
    ch = trimmed[0]
    run = len(trimmed) - len(trimmed.lstrip(ch))
    if run < 3:
        return None
    return ch, run, trimmed[run:]


def fsl_fences(text: str) -> list[Fence]:
    """``fsl`` fences, recognised exactly as ``extract_literate_fsl`` does."""
    fences: list[Fence] = []
    open_fence: tuple[str, int] | None = None
    for number, line in enumerate(text.split("\n"), start=1):
        trimmed = line.strip()
        if open_fence is not None:
            ch, run = open_fence
            if trimmed and set(trimmed) == {ch} and len(trimmed) >= run:
                open_fence = None
            continue
        found = _opening(trimmed)
        if found is None:
            continue
        ch, run, info = found
        open_fence = (ch, run)
        tokens = info.split()
        if tokens and tokens[0] == "fsl":
            fences.append(Fence(number, MARKER in tokens[1:]))
    return fences


def rename_unmarked(text: str, fences: list[Fence]) -> str:
    """Turn unmarked ``fsl`` fences into non-``fsl`` ones without moving a line."""
    unmarked = {f.line for f in fences if not f.marked}
    lines = text.split("\n")
    for number in unmarked:
        lines[number - 1] = re.sub(
            r"^(\s*[`~]{3,}\s*)fsl\b", rf"\g<1>{FRAGMENT_LANG}", lines[number - 1], count=1
        )
    return "\n".join(lines)


@dataclass(frozen=True)
class Failure:
    path: str
    line: int | None
    message: str

    def render(self) -> str:
        where = f"{self.path}:{self.line}" if self.line else self.path
        return f"{where}: {self.message}"


def run_fslc(fslc: str, target: Path) -> tuple[int, dict | None, str]:
    proc = subprocess.run(
        [fslc, "check", str(target)], capture_output=True, text=True, check=False
    )
    try:
        payload = json.loads(proc.stdout)
    except json.JSONDecodeError:
        payload = None
    return proc.returncode, payload, proc.stderr.strip()


def check_document(fslc: str, path: Path, display: str) -> tuple[int, int, list[Failure]]:
    """Return (marked, unmarked, failures) for one document."""
    text = path.read_text(encoding="utf-8")
    fences = fsl_fences(text)
    marked = sum(1 for f in fences if f.marked)
    unmarked = len(fences) - marked
    if marked == 0:
        return 0, unmarked, []
    target, temp = path, None
    if unmarked:
        temp = path.with_name(f".{path.stem}.fence-gate-{os.getpid()}.md")
        temp.write_text(rename_unmarked(text, fences), encoding="utf-8")
        target = temp
    try:
        code, payload, stderr = run_fslc(fslc, target)
    finally:
        if temp is not None:
            temp.unlink(missing_ok=True)
    if code == 0 and payload is not None and payload.get("result") == "ok":
        return marked, unmarked, []
    if payload is None:
        return marked, unmarked, [Failure(display, None, f"fslc exit {code}, no JSON: {stderr[:200]}")]
    line = (payload.get("loc") or {}).get("line")
    message = str(payload.get("message", payload.get("result")))
    if temp is not None:
        message = message.replace(temp.name, path.name)
    return marked, unmarked, [Failure(display, line, f"{payload.get('kind', 'error')}: {message}")]


def documents(scopes: Iterable[str]) -> list[Path]:
    found: list[Path] = []
    for scope in scopes:
        p = Path(scope)
        p = p if p.is_absolute() else ROOT / p
        found.extend(sorted(p.rglob("*.md")) if p.is_dir() else [p])
    return found


def display_name(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def check(fslc: str, scopes: Iterable[str], out=sys.stdout) -> int:
    docs = documents(scopes)
    checked = skipped = with_marked = with_fsl = 0
    failures: list[Failure] = []
    for path in docs:
        marked, unmarked, bad = check_document(fslc, path, display_name(path))
        checked += marked
        skipped += unmarked
        with_marked += bool(marked)
        with_fsl += bool(marked or unmarked)
        failures.extend(bad)
    for failure in failures:
        print(f"check-doc-fences: FAIL {failure.render()}", file=out)
    print(
        f"check-doc-fences: {len(docs)} documents, {with_fsl} with fsl fences; "
        f"{checked} fences checked in {with_marked} documents, "
        f"{skipped} fsl fences skipped (unmarked, NOT verified); "
        f"{len(failures)} failures",
        file=out,
    )
    return 1 if failures else 0


GOOD = "spec Good {\n  state {\n    n: Int,\n  }\n  init { n = 0 }\n}\n"


def selftest(fslc: str) -> int:
    problems: list[str] = []

    def expect(label: str, ok: bool) -> None:
        if not ok:
            problems.append(label)

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)

        def doc(name: str, body: str) -> Path:
            path = root / name
            path.write_text(body, encoding="utf-8")
            return path

        # Parser controls, no fslc needed.
        marks = [
            f.marked
            for f in fsl_fences(
                "```fsl check\n```\n```fsl\n```\n```fsl title=x check\n```\n"
                "```text\n```\n````text\n```fsl check\n```\n````\n~~~fsl check\n~~~\n"
                "```fsl-req check\n```\n```fsl checked\n```\n"
            )
        ]
        expect("fence recognition", marks == [True, False, True, True, False])

        renamed = rename_unmarked("a\n```fsl\nx\n```\n```fsl check\ny\n```\n", fsl_fences("a\n```fsl\nx\n```\n```fsl check\ny\n```\n"))
        expect("rename keeps lines and marked fences",
               renamed == "a\n```fsl-fragment\nx\n```\n```fsl check\ny\n```\n")

        # Accepting: a marked complete spec.
        _, _, bad = check_document(fslc, doc("ok.md", f"# t\n\n```fsl check\n{GOOD}```\n"), "ok.md")
        expect("marked good spec passes", not bad)

        # Rejecting, and the reported line is the Markdown line (calibration).
        broken = GOOD.replace("spec Good", "spce Good")
        text = "# t\n\nprose\n\n```fsl check\n" + broken + "```\n"
        want = 1 + text.split("\n").index("spce Good {") 
        _, _, bad = check_document(fslc, doc("bad.md", text), "bad.md")
        expect("marked broken spec fails", len(bad) == 1)
        expect(f"reported line is the .md line ({want})", bool(bad) and bad[0].line == want)

        # Over-detection control: an unmarked fragment is skipped and counted,
        # and a marked good spec beside it still passes.
        frag = "\n\nSee:\n\n```fsl\naction broken_fragment {\n```\n"
        _, skipped, bad = check_document(fslc, doc("frag.md", f"```fsl check\n{GOOD}```{frag}"), "frag.md")
        expect("fragment beside a marked spec is skipped, not failed", not bad and skipped == 1)

        # A broken marked fence after a fragment keeps its Markdown line.
        text = "```fsl\nfragment\n```\n\n```fsl check\n" + broken + "```\n"
        want = 1 + text.split("\n").index("spce Good {")
        _, skipped, bad = check_document(fslc, doc("mixed.md", text), "mixed.md")
        expect("mixed document reports the .md line",
               skipped == 1 and len(bad) == 1 and bad[0].line == want)

        # An unmarked document is never run, whatever it contains.
        marked, skipped, bad = check_document(fslc, doc("plain.md", "```fsl\nnot a spec\n```\n"), "plain.md")
        expect("unmarked-only document is skipped and counted", (marked, skipped, bad) == (0, 1, []))

        # No temp file left behind.
        expect("no temp file left", not [p for p in root.iterdir() if p.name.startswith(".")])

    for problem in problems:
        print(f"check-doc-fences selftest: FAIL {problem}", file=sys.stderr)
    if problems:
        return 1
    print("check-doc-fences selftest: ok")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("command", choices=("check", "selftest"))
    parser.add_argument("paths", nargs="*", default=list(DEFAULT_SCOPES))
    parser.add_argument("--fslc", default=os.environ.get("FSLC") or "fslc")
    args = parser.parse_args(argv)
    fslc = shutil.which(args.fslc) or (args.fslc if Path(args.fslc).is_file() else None)
    if fslc is None:
        print(f"check-doc-fences: fslc not found ({args.fslc}); refusing to pass", file=sys.stderr)
        return 2
    if args.command == "selftest":
        return selftest(fslc)
    return check(fslc, args.paths)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
