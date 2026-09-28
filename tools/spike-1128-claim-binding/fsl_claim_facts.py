#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Spike projector `fsl-claim-facts@0` for issue #1128 (not wired to any gate).

A claim is a prose sentence in a hand-written document, bound to one named FSL
element. This projector answers one question only: since the commit recorded in
the claim, has either the bound FSL element or the claim's own text changed, so
that the recorded human verification no longer covers what is written now?

It does not decide whether the claim is true. `stale` means "the previous
verification is no longer enough", never "the document is wrong".

Two annotation forms are accepted, carrying identical fields:

  MyST directive form (renders badly on GitHub, see the spike record):

    :::{claim} Title
    :id: REQ-X-001
    :bind: fsl:specs/s.fsl#action:a
    :verified-at: <commit>
    :verified-with: fsl-claim-facts@0

    prose

    ```yaml facts
    requires: guard text
    ```
    :::

  HTML comment form (invisible on GitHub, the recommended one):

    <!-- claim
    id: REQ-X-001
    bind: fsl:specs/s.fsl#action:a
    verified-at: <commit>
    verified-with: fsl-claim-facts@0
    facts:
      requires: guard text
    -->
    prose
    <!-- /claim -->

Unlike `ts-claim-facts@0`, the facts block is a full-set assertion, not a
selector: every fact of a listed kind found in the bound element is compared,
so a guard *added* to the element is detected too.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

PROJECTOR_VERSION = "fsl-claim-facts@0"
FACT_KINDS = ("requires", "ensures", "writes")
OPTION_KEYS = ("id", "bind", "evidence", "verified-at", "verified-with")
STAMPED_KEYS = ("verified-at", "verified-with")

CLAIM_START = re.compile(r"^:::\{claim\}\s*(.*)$")
OPTION_LINE = re.compile(r"^:([a-z-]+):\s*(.*)$")
COMMENT_START = re.compile(r"^<!--\s*claim\s*$")
COMMENT_END = re.compile(r"^-->\s*$")
COMMENT_CLOSE = re.compile(r"^<!--\s*/claim\s*-->\s*$")
FIELD_LINE = re.compile(r"^\s*([a-z-]+):\s*(.*)$")
BIND_RE = re.compile(r"^([^:]+):([^#]+)#(.+)$")
SYMBOL_RE = re.compile(r"^(action|invariant|reachable)\s*:\s*([A-Za-z_][A-Za-z0-9_]*)$")
CLAUSE_RE = re.compile(r"^(requires|ensures)\b\s*(.*)$")
ASSIGN_RE = re.compile(r"^(?!let\b)([A-Za-z_][^=]*?)\s*=\s*(.+?)\s*$")


class ProjectorError(Exception):
    """An input error that makes the comparison indeterminate (exit 2)."""


@dataclass
class Claim:
    claim_id: str
    form: str
    options: dict[str, str]
    facts: dict[str, list[str]]
    prose: list[str]
    start_line: int
    stamp_line: int | None = None
    indent: str = ""

    @property
    def bind(self) -> str | None:
        return self.options.get("bind")

    @property
    def verified_at(self) -> str | None:
        return self.options.get("verified-at")

    @property
    def verified_with(self) -> str | None:
        return self.options.get("verified-with")

    def subject_hash(self) -> str:
        """Hash of everything the human attested to: the claim text and the
        asserted facts, but not the stamp itself."""
        parts: list[str] = []
        for key in OPTION_KEYS:
            if key in STAMPED_KEYS:
                continue
            if key in self.options:
                parts.append(f"{key}: {self.options[key]}")
        for kind in FACT_KINDS:
            for value in self.facts.get(kind, []):
                parts.append(f"{kind}: {value}")
        prose = "\n".join(line.strip() for line in self.prose).strip()
        parts.append(prose)
        return hashlib.sha256("\n".join(parts).encode("utf-8")).hexdigest()


def _norm(text: str) -> str:
    return " ".join(text.split())


def _add_fact(facts: dict[str, list[str]], key: str, value: str, line_no: int) -> None:
    if key not in FACT_KINDS:
        raise ProjectorError(f"line {line_no}: unknown fact kind: {key}")
    facts.setdefault(key, []).append(_norm(value))


def parse_claims(text: str) -> list[Claim]:
    lines = text.splitlines()
    claims: list[Claim] = []
    idx = 0
    while idx < len(lines):
        if CLAIM_START.match(lines[idx]):
            claim, idx = _parse_directive(lines, idx)
            claims.append(claim)
            continue
        if COMMENT_START.match(lines[idx]):
            claim, idx = _parse_comment(lines, idx)
            claims.append(claim)
            continue
        idx += 1
    return claims


def _parse_directive(lines: list[str], start: int) -> tuple[Claim, int]:
    idx = start + 1
    options: dict[str, str] = {}
    facts: dict[str, list[str]] = {}
    prose: list[str] = []
    stamp_line: int | None = None
    in_options = True
    in_facts = False
    while idx < len(lines) and lines[idx].strip() != ":::":
        line = lines[idx]
        opt = OPTION_LINE.match(line)
        if in_options and opt:
            if opt.group(1) not in OPTION_KEYS:
                raise ProjectorError(f"line {idx + 1}: unknown claim option: {opt.group(1)}")
            options[opt.group(1)] = opt.group(2).strip()
            if opt.group(1) == "verified-at" and stamp_line is None:
                stamp_line = idx
            idx += 1
            continue
        if line.strip():
            in_options = False
        stripped = line.strip()
        if stripped.startswith("```"):
            if stripped == "```yaml facts":
                in_facts = True
            elif in_facts:
                in_facts = False
            idx += 1
            continue
        if in_facts:
            if stripped:
                field_m = FIELD_LINE.match(stripped)
                if not field_m:
                    raise ProjectorError(f"line {idx + 1}: invalid facts line")
                _add_fact(facts, field_m.group(1), field_m.group(2), idx + 1)
        elif not in_options:
            prose.append(line)
        idx += 1
    if idx >= len(lines):
        raise ProjectorError(f"line {start + 1}: unclosed claim directive")
    if "id" not in options:
        raise ProjectorError(f"line {start + 1}: claim missing id")
    return (
        Claim(options["id"], "directive", options, facts, prose, start + 1, stamp_line),
        idx + 1,
    )


def _parse_comment(lines: list[str], start: int) -> tuple[Claim, int]:
    idx = start + 1
    options: dict[str, str] = {}
    facts: dict[str, list[str]] = {}
    prose: list[str] = []
    stamp_line: int | None = None
    in_facts = False
    while idx < len(lines) and not COMMENT_END.match(lines[idx]):
        line = lines[idx]
        if not line.strip():
            idx += 1
            continue
        field_m = FIELD_LINE.match(line)
        if not field_m:
            raise ProjectorError(f"line {idx + 1}: invalid claim field")
        key, value = field_m.group(1), field_m.group(2).strip()
        if key == "facts" and not value:
            in_facts = True
        elif in_facts and line.startswith((" ", "\t")):
            _add_fact(facts, key, value, idx + 1)
        else:
            if key not in OPTION_KEYS:
                raise ProjectorError(f"line {idx + 1}: unknown claim field: {key}")
            in_facts = False
            options[key] = value
            if key == "verified-at" and stamp_line is None:
                stamp_line = idx
        idx += 1
    if idx >= len(lines):
        raise ProjectorError(f"line {start + 1}: unterminated claim header comment")
    idx += 1
    while idx < len(lines) and not COMMENT_CLOSE.match(lines[idx]):
        prose.append(lines[idx])
        idx += 1
    if idx >= len(lines):
        raise ProjectorError(f"line {start + 1}: missing <!-- /claim --> terminator")
    if "id" not in options:
        raise ProjectorError(f"line {start + 1}: claim missing id")
    return (
        Claim(options["id"], "comment", options, facts, prose, start + 1, stamp_line),
        idx + 1,
    )


def parse_bind(bind: str) -> tuple[str, str, str, str]:
    m = BIND_RE.match(bind)
    if not m:
        raise ProjectorError(f"invalid bind: {bind}")
    lang, path, symbol = m.group(1), m.group(2), m.group(3)
    if lang != "fsl":
        raise ProjectorError(f"unsupported bind language: {lang}")
    sym = SYMBOL_RE.match(symbol)
    if not sym:
        raise ProjectorError(f"invalid FSL symbol: {symbol} (want action:NAME)")
    return lang, path, sym.group(1), sym.group(2)


def extract_element_body(source: str, kind: str, name: str) -> str:
    """Return the brace-delimited body of one named FSL element."""
    pattern = re.compile(
        rf"^\s*{kind}\s+{re.escape(name)}\s*(?:\([^)]*\))?\s*\{{",
        re.MULTILINE,
    )
    m = pattern.search(source)
    if not m:
        raise ProjectorError(f"FSL element not found: {kind} {name}")
    start = source.index("{", m.start())
    depth = 0
    for i in range(start, len(source)):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return source[start + 1 : i]
    raise ProjectorError(f"unbalanced braces in {kind} {name}")


def project_facts(body: str, kinds: list[str]) -> dict[str, list[str]]:
    requires: list[str] = []
    ensures: list[str] = []
    writes: list[str] = []
    for raw in body.splitlines():
        line = raw.split("//", 1)[0].strip()
        if not line:
            continue
        clause = CLAUSE_RE.match(line)
        if clause:
            (requires if clause.group(1) == "requires" else ensures).append(
                _norm(clause.group(2))
            )
            continue
        if "==" in line or line.endswith("{") or line.startswith("}"):
            continue
        assign = ASSIGN_RE.match(line)
        if assign:
            writes.append(f"{_norm(assign.group(1))} = {_norm(assign.group(2))}")
    found = {"requires": requires, "ensures": ensures, "writes": writes}
    return {kind: sorted(found[kind]) for kind in kinds}


def _git(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)


def git_show(root: Path, commit: str, rel_path: str) -> str:
    proc = _git(root, "show", f"{commit}:{rel_path}")
    if proc.returncode != 0:
        raise ProjectorError(proc.stderr.strip() or f"git show failed: {commit}:{rel_path}")
    return proc.stdout


def evaluate(claim: Claim, root: Path, md_rel: str) -> dict[str, object]:
    result: dict[str, object] = {"id": claim.claim_id, "form": claim.form}
    if not claim.bind:
        raise ProjectorError(f"claim {claim.claim_id} missing bind")
    if not claim.verified_at:
        result["status"] = "unverified"
        return result
    if claim.verified_with != PROJECTOR_VERSION:
        result["status"] = "reindex"
        return result
    if _git(root, "cat-file", "-e", f"{claim.verified_at}^{{commit}}").returncode != 0:
        raise ProjectorError(f"invalid verified-at: {claim.verified_at}")

    _lang, rel_path, kind, name = parse_bind(claim.bind)
    kinds = [k for k in FACT_KINDS if k in claim.facts]
    if not kinds:
        raise ProjectorError(f"claim {claim.claim_id} asserts no facts")

    head_facts = project_facts(
        extract_element_body((root / rel_path).read_text(encoding="utf-8"), kind, name), kinds
    )
    was_facts = project_facts(
        extract_element_body(git_show(root, claim.verified_at, rel_path), kind, name), kinds
    )
    asserted = {k: sorted(claim.facts.get(k, [])) for k in kinds}

    reasons: list[str] = []
    diff: dict[str, object] = {}
    if head_facts != was_facts:
        reasons.append("facts_changed")
        for k in kinds:
            if head_facts[k] != was_facts[k]:
                diff[k] = {"head": head_facts[k], "verified_at": was_facts[k]}
    if head_facts != asserted:
        reasons.append("document_facts_disagree_with_head")
        for k in kinds:
            if head_facts[k] != asserted[k] and k not in diff:
                diff[k] = {"head": head_facts[k], "document": asserted[k]}

    was_md = git_show(root, claim.verified_at, md_rel)
    was_claims = [c for c in parse_claims(was_md) if c.claim_id == claim.claim_id]
    if not was_claims:
        reasons.append("claim_absent_at_verified_at")
    elif was_claims[0].subject_hash() != claim.subject_hash():
        reasons.append("prose_changed")

    if reasons:
        result["status"] = "stale"
        result["reasons"] = reasons
        if diff:
            result["diff"] = diff
        return result
    result["status"] = "verified"
    return result


def cmd_check(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    md_path = Path(args.md).resolve()
    md_rel = str(md_path.relative_to(root))
    claims = parse_claims(md_path.read_text(encoding="utf-8"))
    results = [evaluate(c, root, md_rel) for c in claims]
    print(json.dumps({"projector": PROJECTOR_VERSION, "claims": results}, indent=2))
    return 3 if any(r["status"] == "stale" for r in results) else 0


def cmd_stamp(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    md_path = Path(args.md).resolve()
    head = _git(root, "rev-parse", "HEAD").stdout.strip()
    lines = md_path.read_text(encoding="utf-8").splitlines()
    claims = [c for c in parse_claims("\n".join(lines)) if c.claim_id == args.claim]
    if not claims:
        raise ProjectorError(f"claim not found: {args.claim}")
    claim = claims[0]
    if claim.form == "comment":
        new = [f"verified-at: {head}", f"verified-with: {PROJECTOR_VERSION}"]
        anchor = "verified-at:"
    else:
        new = [f":verified-at: {head}", f":verified-with: {PROJECTOR_VERSION}"]
        anchor = ":verified-at:"
    kept = [
        line
        for i, line in enumerate(lines)
        if not (claim.start_line <= i + 1 <= claim.start_line + 40 and line.strip().startswith((anchor, anchor.replace("-at:", "-with:"))))
    ]
    insert_at = None
    for i, line in enumerate(kept):
        if i + 1 < claim.start_line:
            continue
        if line.strip().startswith(("bind:", ":bind:", "evidence:", ":evidence:")):
            insert_at = i + 1
    if insert_at is None:
        raise ProjectorError(f"claim {args.claim}: no bind/evidence line to stamp after")
    kept[insert_at:insert_at] = new
    md_path.write_text("\n".join(kept) + "\n", encoding="utf-8")
    print(f"{args.claim}: verified-at {head}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="fsl_claim_facts")
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check", help="Compare claim facts against the bound FSL element")
    check.add_argument("md")
    check.add_argument("--root", required=True)
    check.set_defaults(func=cmd_check)
    stamp = sub.add_parser("stamp", help="Record verified-at/verified-with on a claim")
    stamp.add_argument("md")
    stamp.add_argument("--claim", required=True)
    stamp.add_argument("--root", required=True)
    stamp.set_defaults(func=cmd_stamp)
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except ProjectorError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
