# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Coupled-change metatests (issue #168).

CLAUDE.md/CONTRIBUTING.md state "a language feature moves grammar/model/bmc
(+runtime) + docs/LANGUAGE.md + skills/fsl/reference.md + a
docs/DESIGN-<feature>.md + tests together" -- until now that was a human
checklist. `d1770c4` showed the failure mode: `src/fslc/lsp/index.py` was
outside the list, so the `ai_component`/`dbsystem` dialects shipped with the
LSP entirely dark. Prototyping this metatest re-found the same class twice
more (fsl-domain, fsl-ai project files) -- fixed in the same PR that adds
this file (see docs/DESIGN-coupled-change-metatest.md).

Two independent checks, no Z3 dependency (lark + file scans only):
1. LSP index coverage -- every grammar production with a NAME/REQ_ID token
   either has a `_visit_<rule>` handler, or its tokens are proven to reach
   `symbols`/`references` through a parent handler, or it is an explicit,
   reviewed allowlist entry.
2. DESIGN-doc coverage -- docs/README.md's DESIGN-*.md map is bidirectional,
   every kernel dialect (`top_def` alternative) maps to a doc, and every CLI
   command maps to a doc (or an explicit waiver reason).
"""
from __future__ import annotations

import argparse
import re
from pathlib import Path

from lark import Token, Tree

from fslc.ai_parser import is_ai_agent_source, is_ai_source
from fslc.ai_project import is_ai_project_source
from fslc.cli import _build_arg_parser
from fslc.db_parser import is_dbsystem_source
from fslc.domain_parser import is_domain_source
from fslc.grammar import GRAMMAR
from fslc.lsp.index import DOMAIN_PARSER, _IndexBuilder, _parser_for_source, build_index

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"

# ---------------------------------------------------------------------------
# 1. LSP index coverage
# ---------------------------------------------------------------------------

# (parser_id, node.data) -> reason. A grammar production whose NAME/REQ_ID
# token is genuinely a free label / external artifact id / engine keyword,
# not an in-file symbol or reference -- reviewed design decisions, not a
# parking lot. Staleness is enforced by test_lsp_allowlist_not_stale below:
# every entry must still occur in the corpus for its parser.
INTENTIONALLY_UNINDEXED = {
    ("kernel", "control_severity"): "`severity high` -- free severity word, not an in-file symbol",
    ("db", "column_type"): "`column x: int` -- engine scalar-type words, dbsystem has no in-file type decls",
    ("db", "check_item"): "`rule <name>` names a built-in compatibility rule, not an in-file declaration",
    ("ai", "check_item"): "`check hard { rule ... }` names a built-in hard-check rule",
    ("ai", "tool_precondition"): "free-form semantic label (`precondition order_paid`)",
    ("ai", "tool_effect"): "free-form effect label",
    ("ai", "trust_def"): "`trust medium` -- free trust-level word",
    ("ai", "atom_name"): "`model gpt_5_5` / `prompt ..._v8` -- external artifact ids",
    ("ai", "failure_target"): "`-> HumanReviewPending` -- policy-outcome label, not an in-file symbol",
}

_CORPUS_ROOTS = ("specs", "examples")
_EXCLUDED_DIRS = (ROOT / "examples" / "gallery" / "errors",)


def _corpus_files() -> list[Path]:
    paths: set[Path] = set()
    for root in _CORPUS_ROOTS:
        paths.update((ROOT / root).rglob("*.fsl"))
    excluded = {p for p in paths if any(str(p).startswith(str(d)) for d in _EXCLUDED_DIRS)}
    return sorted(paths - excluded)


def _parser_id(src: str) -> "str | None":
    """None means the source is not Lark-parsed at all by the LSP (fsl-ai
    project files: indexed by ai_project._top_blocks scanning instead, see
    lsp/index.py's build_index)."""
    if is_ai_project_source(src):
        return None
    if is_dbsystem_source(src):
        return "db"
    if is_domain_source(src):
        return "domain"
    if is_ai_source(src):
        return "ai"
    return "kernel"


def _walk(node) -> "list":
    out = [node]
    if isinstance(node, Tree):
        for child in node.children:
            out.extend(_walk(child))
    return out


def _direct_name_tokens(node: Tree) -> list:
    return [c for c in node.children if isinstance(c, Token) and c.type in ("NAME", "REQ_ID")]


def test_lsp_index_covers_corpus_grammar():
    assert _EXCLUDED_DIRS[0].is_dir() and any(_EXCLUDED_DIRS[0].glob("*.fsl")), (
        "examples/gallery/errors must stay non-empty, or this exclusion is silently vacuous"
    )

    candidates: dict = {}  # (parser_id, node.data) -> list[(file, line)]
    for path in _corpus_files():
        src = path.read_text(encoding="utf-8")
        parser_id = _parser_id(src)
        if parser_id is None:
            continue
        try:
            tree = _parser_for_source(src).parse(src)
        except Exception as exc:  # noqa: BLE001 -- a corpus file the LSP cannot even parse is itself the finding
            raise AssertionError(f"{path}: LSP parser ({parser_id}) could not parse this corpus file: {exc}") from exc

        try:
            idx = build_index(src, str(path))
        except Exception as exc:  # noqa: BLE001
            raise AssertionError(f"{path}: build_index raised: {exc}") from exc
        indexed_positions = {s.selection_range.start_tuple for s in idx.symbols} | {
            r.range.start_tuple for r in idx.references
        }

        for node in _walk(tree):
            if not isinstance(node, Tree):
                continue
            names = _direct_name_tokens(node)
            if not names:
                continue
            key = (parser_id, node.data)
            handled = hasattr(_IndexBuilder, f"_visit_{node.data}")
            missing = [tok for tok in names if (tok.line - 1, tok.column - 1) not in indexed_positions]
            if missing:
                candidates.setdefault(key, []).append(
                    (path.relative_to(ROOT).as_posix(), missing[0].line, handled)
                )

    unallowed = {k: v for k, v in candidates.items() if k not in INTENTIONALLY_UNINDEXED}
    assert not unallowed, {
        f"{parser_id}:{node_data}": f"{len(hits)} miss(es), e.g. {hits[0][0]}:{hits[0][1]} (handler exists: {hits[0][2]})"
        for (parser_id, node_data), hits in unallowed.items()
    }


def test_lsp_allowlist_not_stale():
    seen: set = set()
    for path in _corpus_files():
        src = path.read_text(encoding="utf-8")
        parser_id = _parser_id(src)
        if parser_id is None:
            continue
        tree = _parser_for_source(src).parse(src)
        for node in _walk(tree):
            if isinstance(node, Tree) and _direct_name_tokens(node):
                seen.add((parser_id, node.data))
    stale = set(INTENTIONALLY_UNINDEXED) - seen
    assert not stale, f"allowlist entries no longer occur in the corpus, remove them: {stale}"


def test_domain_parser_exported_for_lsp_dispatch():
    # Guards the specific bug this issue closed: a dedicated Lark instance
    # for fsl-domain must exist and be reachable from lsp/index.py, or every
    # examples/domain/*.fsl silently fails to parse there again.
    assert DOMAIN_PARSER is not None


def test_ai_agent_dialect_still_dispatches_through_ai_parser():
    # A distinct guard from the metatest above: `agent` recursive-composition
    # files (unlike `ai_component` hard-contract files) must keep resolving
    # through the same AI_PARSER path, not regress into the ai-project branch.
    sample = next(
        p for p in _corpus_files()
        if is_ai_source(p.read_text(encoding="utf-8")) and is_ai_agent_source(p.read_text(encoding="utf-8"))
    )
    src = sample.read_text(encoding="utf-8")
    assert not is_ai_project_source(src)
    idx = build_index(src, str(sample))
    assert idx.symbols, f"{sample}: expected at least one indexed symbol"


# ---------------------------------------------------------------------------
# 2. DESIGN-doc coverage
# ---------------------------------------------------------------------------

TOP_DEF_DESIGN_DOCS = {
    "spec_def": ("DESIGN-v1.md",),
    "refinement_def": ("DESIGN-refinement.md",),
    "compose_def": ("DESIGN-compose.md",),
    "requirements_def": ("DESIGN-dialects.md",),
    "business_def": ("DESIGN-dialects.md",),
    "governance_def": ("DESIGN-dialects.md",),
}

# command -> tuple of required docs, or a str waiver reason.
COMMAND_DESIGN_DOCS = {
    "version": "CLI plumbing, no design doc of its own",
    "check": ("DESIGN-v1.md",),
    "verify": ("DESIGN-v1.md", "DESIGN-induction.md", "DESIGN-from-state.md"),
    "sweep": "scope-grid driver over verify; no standalone semantics (documented in LANGUAGE.md)",
    "scenarios": ("DESIGN-scenarios.md",),
    "replay": ("DESIGN-bridge.md", "DESIGN-log-replay.md"),
    "testgen": ("DESIGN-bridge.md",),
    "mutate": ("DESIGN-mutate.md",),
    "explain": ("DESIGN-explain.md",),
    "html": ("DESIGN-html-report.md",),
    "ledger": ("DESIGN-ledger.md",),
    "refine": ("DESIGN-refinement.md",),
    "diff": ("DESIGN-semantic-diff.md",),
    "chain": ("DESIGN-layers.md",),
    "typestate": ("DESIGN-typestate.md",),
    "analyze": ("DESIGN-analysis.md",),
    "db": ("DESIGN-db.md",),
    "compat": ("DESIGN-db.md",),
    "ai": ("DESIGN-ai-hard.md", "DESIGN-stochastic.md"),
    "domain": ("DESIGN-domain.md", "DESIGN-effect.md"),
}


def test_design_docs_readme_map_bidirectional():
    readme = (DOCS / "README.md").read_text(encoding="utf-8")
    linked = set(re.findall(r"DESIGN-[A-Za-z0-9-]+\.md", readme))
    on_disk = {p.name for p in DOCS.glob("DESIGN-*.md")}
    missing_on_disk = linked - on_disk
    unlinked = on_disk - linked
    assert not missing_on_disk, f"docs/README.md links a DESIGN doc that doesn't exist: {missing_on_disk}"
    assert not unlinked, f"DESIGN doc(s) on disk but not linked from docs/README.md: {unlinked}"


def test_top_level_dialects_map_to_design_docs():
    match = re.search(r"^top_def:\s*(.+)$", GRAMMAR, re.MULTILINE)
    assert match, "could not find the top_def rule in fslc.grammar.GRAMMAR"
    alternatives = [alt.strip() for alt in match.group(1).split("|")]
    assert set(alternatives) == set(TOP_DEF_DESIGN_DOCS), (
        f"top_def alternatives {sorted(alternatives)} != TOP_DEF_DESIGN_DOCS keys "
        f"{sorted(TOP_DEF_DESIGN_DOCS)} -- map the new/removed alternative"
    )
    for alt, docs in TOP_DEF_DESIGN_DOCS.items():
        for doc in docs:
            assert (DOCS / doc).exists(), f"{alt} -> {doc}, but {doc} does not exist"


def test_cli_commands_map_to_design_docs():
    parser = _build_arg_parser()
    sub_action = next(a for a in parser._actions if isinstance(a, argparse._SubParsersAction))
    commands = set(sub_action.choices)
    assert commands == set(COMMAND_DESIGN_DOCS), (
        f"CLI commands {sorted(commands)} != COMMAND_DESIGN_DOCS keys {sorted(COMMAND_DESIGN_DOCS)} "
        "-- map the new/removed command (a tuple of required docs, or a str waiver reason)"
    )
    for command, docs in COMMAND_DESIGN_DOCS.items():
        if isinstance(docs, str):
            continue
        for doc in docs:
            assert (DOCS / doc).exists(), f"{command} -> {doc}, but {doc} does not exist"
