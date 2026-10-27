# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Issue #1501 detectors and preservation controls for site ATX headings.

The detectors are expected to fail on base 6b8b73bc: Python-Markdown treats a
wrapped line starting with #1234 (or a real issue number) as an h1 and loses the
hash. The controls must pass on that base and after the renderer is fixed.
"""
from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
TOOL_PATH = REPO_ROOT / "tools" / "build_site_reference.py"

pytest.importorskip("markdown")


def _load_tool():
    spec = importlib.util.spec_from_file_location("build_site_reference", TOOL_PATH)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


def _rejoin_language_md(lead: str, sections: list[tuple[str, str]]) -> str:
    return lead + "".join(f"\n## {heading}\n{body}" for heading, body in sections) + "\n"


def _render_fixture(tmp_path, monkeypatch, lang: str, snippet: str) -> str:
    """Insert only body text, preserving the real manuals' section alignment."""
    mod = _load_tool()
    attr = "LANGUAGE_MD_JA" if lang == "ja" else "LANGUAGE_MD"
    source = getattr(mod, attr)
    original = source.read_text(encoding="utf-8")
    sections = mod.split_language_md(original)
    lead = original.split("\n## ", 1)[0]
    rebuilt = _rejoin_language_md(lead, sections)
    assert rebuilt == original, (
        f"Unmodified {source.name} must round-trip exactly; produced "
        f"{rebuilt!r}, expected {original!r}"
    )

    # Prepend to a section body with blank-line boundaries; no headings move and
    # the inserted snippet cannot accidentally fall inside an existing fence.
    modified = list(sections)
    heading, body = modified[0]
    modified[0] = (heading, f"\n\n{snippet}\n\n{body}")
    fixture_path = tmp_path / source.name
    fixture_path.write_text(_rejoin_language_md(lead, modified), encoding="utf-8")
    monkeypatch.setattr(mod, attr, fixture_path)
    return mod.render_language_tree(lang)


def _element_contents(tree: str, tag: str) -> list[str]:
    return re.findall(rf"<{tag}\b[^>]*>(.*?)</{tag}>", tree, flags=re.DOTALL)


@pytest.mark.parametrize(
    ("lang", "previous_line", "wrapped_line"),
    [
        ("ja", "前の行です。", "#1234 では、折り返された行です。"),
        ("en", "Previous line.", "#1234 starts this wrapped line."),
    ],
    ids=["ja", "en"],
)
def test_wrapped_issue_reference_is_paragraph_text(
    tmp_path, monkeypatch, lang, previous_line, wrapped_line
):
    """Detector: both locales fail on 6b8b73bc's h1 conversion and lost hash."""
    paragraph = f"{previous_line}\n{wrapped_line}"
    tree = _render_fixture(tmp_path, monkeypatch, lang, paragraph)
    issue_headings = [
        text for text in _element_contents(tree, "h1") if text.startswith("1234")
    ]
    assert not issue_headings, (
        f"{lang}: wrapped #1234 must not become an h1; produced {issue_headings!r}"
    )
    paragraphs = [
        text for text in _element_contents(tree, "p") if previous_line in text
    ]
    assert paragraph in paragraphs, (
        f"{lang}: expected one paragraph retaining {paragraph!r}; produced {paragraphs!r}"
    )


@pytest.mark.parametrize("issue", ["1448", "1200"])
def test_real_japanese_issue_reference_is_not_h1(issue):
    """Detector: 6b8b73bc turns each real wrapped issue reference into an h1."""
    tree = _load_tool().render_language_tree("ja")
    prefix = f'<h1 id="{issue}'
    headings = _element_contents(tree, "h1")
    assert prefix not in tree, (
        f"Real #{issue} must not produce {prefix!r}; produced h1 contents {headings!r}"
    )
    paragraphs = [
        text for text in _element_contents(tree, "p") if issue in text
    ]
    assert any(f"#{issue}" in text for text in paragraphs), (
        f"Real #{issue} must remain paragraph text with its hash; produced {paragraphs!r}"
    )


def test_space_separated_heading_is_preserved(tmp_path, monkeypatch):
    """Control: #### plus a space is an h4 before and after the fix (6b8b73bc)."""
    tree = _render_fixture(tmp_path, monkeypatch, "en", "#### Control heading")
    headings = _element_contents(tree, "h4")
    assert "Control heading" in headings, (
        f"Expected a real h4 with text 'Control heading'; produced {headings!r}"
    )


def test_fenced_issue_reference_is_preserved_verbatim(tmp_path, monkeypatch):
    """Control: fences protect #1234 on 6b8b73bc; fixes must not add a backslash."""
    line = "#1234 stays verbatim in code."
    tree = _render_fixture(tmp_path, monkeypatch, "en", f"```\n{line}\n```")
    code_blocks = re.findall(r"<pre><code>(.*?)</code></pre>", tree, flags=re.DOTALL)
    assert f"{line}\n" in code_blocks, (
        f"Expected {line!r} verbatim in pre/code with no added escape; "
        f"produced {code_blocks!r}"
    )


def test_already_escaped_issue_reference_remains_text(tmp_path, monkeypatch):
    """Control: 6b8b73bc consumes the existing escape; fixes must not double it."""
    tree = _render_fixture(
        tmp_path, monkeypatch, "en", r"\#5678 was already escaped."
    )
    paragraphs = [
        text for text in _element_contents(tree, "p") if "5678" in text
    ]
    assert "#5678 was already escaped." in paragraphs, (
        "Expected escaped #5678 as paragraph text without a backslash; "
        f"produced {paragraphs!r}"
    )
