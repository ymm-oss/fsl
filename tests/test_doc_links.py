# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Accepting/rejecting controls for the link-target gate (issue #1127).

`tools/check-doc-links.py` is a required pre-merge step. These controls
reproduce, in CI, the measured table from issue #1127: each break is written
into a throwaway document inside `docs/` (so the relative targets in the table
resolve exactly as a real document's would), the real command-line entry point
runs over that one file, and the exit code is asserted.

Scope note, mirrored in `docs/design/DESIGN-ci.md` ("Link-target resolution"): the gate
checks that a **link target** resolves. A bare path written in prose is not a
link and is not checked, and the backward direction -- every FSL element being
discussed by some hand-written document -- is not claimed here (issue #1138).
"""
from __future__ import annotations

import subprocess
import sys
import uuid
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
DOCS = REPO_ROOT / "docs"
CHECKER = REPO_ROOT / "tools" / "check-doc-links.py"

# Issue #1127's measured table. Each row is (link, expected exit code).
TABLE = (
    ("[broken](DOES-NOT-EXIST.md)", 1),
    ("[broken](../specs/DOES-NOT-EXIST.fsl)", 1),
    ("[x](DESIGN-kernel-contract.md#no-such-heading-anchor)", 1),
    ("[x](#no-such-anchor-in-this-file)", 1),
    # The anchor exists, but in a different document: `## Decision` is a
    # heading of `DESIGN-ci.md`, not of `DESIGN-bridge.md`.
    ("[x](DESIGN-bridge.md#decision)", 1),
    ("[x](../specs/cart_v1.fsl#action:add_to_cart)", 0),
    ("[d](../specs/)", 0),
)


def run_checker(*paths: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CHECKER), "check", *paths],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.fixture()
def fixture_document():
    """A throwaway Markdown file inside `docs/`, removed again afterwards."""
    written: list[Path] = []

    def write(body: str) -> str:
        path = DOCS / f"_doc-links-control-{uuid.uuid4().hex}.md"
        path.write_text(f"# Control fixture\n\n{body}\n", encoding="utf-8")
        written.append(path)
        return str(path.relative_to(REPO_ROOT))

    yield write
    for path in written:
        path.unlink(missing_ok=True)


def test_selftest_passes():
    result = subprocess.run(
        [sys.executable, str(CHECKER), "selftest"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


def test_clean_tree_control_repository_wide():
    """Row 1: the repository as committed reports no finding (over-detection)."""
    result = run_checker(".")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "0 findings" in result.stdout


@pytest.mark.parametrize(("link", "expected"), TABLE, ids=[row[0] for row in TABLE])
def test_measured_table_row(fixture_document, link, expected):
    scope = fixture_document(link)
    result = run_checker(scope)
    assert result.returncode == expected, (
        f"{link}: expected exit {expected}, got {result.returncode}\n"
        f"{result.stdout}{result.stderr}"
    )


def test_renaming_a_linked_document_fails_the_gate(fixture_document, tmp_path):
    """Negative control: a linked document that moves away breaks the gate."""
    target = DOCS / f"_doc-links-target-{uuid.uuid4().hex}.md"
    target.write_text("# Target\n", encoding="utf-8")
    scope = fixture_document(f"[link]({target.name})")
    try:
        assert run_checker(scope).returncode == 0
        renamed = target.with_name(f"renamed-{target.name}")
        target.rename(renamed)
        try:
            assert run_checker(scope).returncode == 1
        finally:
            renamed.unlink(missing_ok=True)
    finally:
        target.unlink(missing_ok=True)


def test_bare_prose_path_is_out_of_scope(fixture_document):
    """The gate checks link targets, not paths mentioned in prose (#1124)."""
    scope = fixture_document("See docs/DOES-NOT-EXIST.md for details.")
    assert run_checker(scope).returncode == 0
