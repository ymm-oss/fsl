# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Python compatibility and DESIGN-doc coupled-change metatests (issue #168).

The authoritative LSP corpus/index gate moved to `rust/fsl-lsp/tests/corpus.rs`.
This module retains only the frozen Python compatibility checks and the
DESIGN-doc map checks that still inspect Python-owned surfaces.
"""
from __future__ import annotations

import argparse
import re
from pathlib import Path

from fslc.ai_project import PROJECT_BLOCKS
from fslc.cli import _build_arg_parser
from fslc.dialect_registry import DIALECT_KEYWORDS
from fslc.grammar import GRAMMAR

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"


def test_retained_python_dialect_registry_matches_native_authority():
    source = (ROOT / "rust" / "fsl-syntax" / "src" / "dispatch.rs").read_text()
    block = re.search(r"frontends!\s*\{(?P<body>.*?)\n\}", source, re.DOTALL)
    assert block is not None
    native = tuple(re.findall(r'"([a-z_]+)"\s*=>', block.group("body")))
    assert native == DIALECT_KEYWORDS


def test_native_ai_project_block_gate_matches_retained_parser():
    source = (ROOT / "rust" / "fslc" / "src" / "frontend_output.rs").read_text()
    block = re.search(
        r"const PROJECT_BLOCKS: &\[&str\] = &\[(?P<body>.*?)\n\s*\];",
        source,
        re.DOTALL,
    )
    assert block is not None
    native = set(re.findall(r'"([a-z_]+)"', block.group("body")))
    assert native == PROJECT_BLOCKS

# DESIGN-doc coverage

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
    "verify": (
        "DESIGN-v1.md",
        "DESIGN-induction.md",
        "DESIGN-induction-lemmas.md",
        "DESIGN-from-state.md",
    ),
    "sweep": "scope-grid driver over verify; no standalone semantics (documented in LANGUAGE.md)",
    "scenarios": ("DESIGN-scenarios.md",),
    "replay": ("DESIGN-bridge.md", "DESIGN-log-replay.md"),
    "testgen": ("DESIGN-bridge.md",),
    "mutate": ("DESIGN-mutate.md",),
    "explain": ("DESIGN-explain.md",),
    "html": ("DESIGN-html-report.md",),
    "ledger": ("DESIGN-ledger.md",),
    "refine": ("DESIGN-refinement.md",),
    "diff": ("DESIGN-semantic-diff.md", "DESIGN-diff-git.md"),
    "chain": ("DESIGN-layers.md",),
    "typestate": ("DESIGN-typestate.md",),
    "analyze": (
        "DESIGN-analysis.md",
        "DESIGN-tag-drift.md",
        "DESIGN-underspecification.md",
    ),
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
