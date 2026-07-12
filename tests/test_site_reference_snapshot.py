# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Freshness check for the generated site reference pages.

``docs/intro/language.{ja,en}.html`` and ``docs/intro/cli.{ja,en}.html`` are committed
generated output (tools/build_site_reference.py), not hand-authored — the same
"commit the generated artifact, diff it in review" discipline as
test_corpus_snapshot.py. This test regenerates into memory and compares against the
committed files so a change to docs/LANGUAGE.md or the fslc CLI surface that forgot to
regenerate the site fails loudly instead of silently shipping a stale reference page.

Regenerate after an intended change to LANGUAGE.md or the CLI surface with::

    python tools/build_site_reference.py
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
TOOL_PATH = REPO_ROOT / "tools" / "build_site_reference.py"
OUT_DIR = REPO_ROOT / "docs" / "intro"

pytest.importorskip("markdown")


def _load_tool():
    spec = importlib.util.spec_from_file_location("build_site_reference", TOOL_PATH)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


def test_argparse_help_normalization_is_python_version_independent():
    mod = _load_tool()
    older = """usage: tool [-h] [--profile PROFILE]
            -o OUTPUT file {check,verify}

optional arguments:
  -o OUTPUT, --output OUTPUT  destination
"""
    newer = """usage: tool [-h] [--profile PROFILE] -o OUTPUT file {check,verify} ...

options:
  -o, --output OUTPUT  destination
"""

    assert mod._normalize_argparse_help(older) == mod._normalize_argparse_help(newer)


@pytest.mark.parametrize("page_id", ["language", "cli"])
def test_generated_reference_pages_are_fresh(page_id):
    mod = _load_tool()
    for lang in ("ja", "en"):
        committed = (OUT_DIR / f"{page_id}.{lang}.html").read_text(encoding="utf-8")
        if page_id == "language":
            tree = mod.render_language_tree(lang)
            source_note = "docs/LANGUAGE.md"
        else:
            tree = mod.render_cli_tree()
            source_note = "src/fslc/cli.py"
        fresh = mod.page_shell(page_id, lang, tree, source_note)
        assert fresh == committed, (
            f"docs/intro/{page_id}.{lang}.html is stale — run "
            "`python tools/build_site_reference.py` and commit the result."
        )
