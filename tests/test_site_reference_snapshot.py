# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Freshness check for the generated site reference pages.

``docs/intro/language.{ja,en}.html`` and ``docs/intro/cli.{ja,en}.html`` are committed
generated output (tools/build_site_reference.py), not hand-authored — the same
"commit the generated artifact, diff it in review" discipline as
test_corpus_snapshot.py. This test regenerates into memory and compares against the
committed files so a change to docs/manual/LANGUAGE.md or the native CLI contract that forgot to
regenerate the site fails loudly instead of silently shipping a stale reference page.

Regenerate after an intended change to LANGUAGE.md or the native CLI contract with::

    python tools/build_site_reference.py
"""
from __future__ import annotations

import html
import importlib.util
import re
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


def _rejoin_language_md(lead: str, sections: list[tuple[str, str]]) -> str:
    """Rebuild a LANGUAGE.md-shaped text from split_language_md() output.

    This is the exact inverse of split_language_md(): each (heading, body) pair
    round-trips as "\\n## {heading}\\n{body}", and a final trailing newline is
    appended once. A caller that builds a reordered/mutated fixture from this must
    first verify the *unmutated* round-trip is byte-identical to the real source
    file — see the assertions in the tests below — before trusting a mutated
    variant built the same way.
    """
    return lead + "".join(f"\n## {heading}\n{body}" for heading, body in sections) + "\n"


def test_language_reference_accepts_aligned_real_files():
    """Accepting control: the untouched, correctly section-aligned real files must
    still generate successfully through the heading-correspondence check added for
    issue #741 — a rejecting test alone would not show the normal path still works.
    """
    mod = _load_tool()
    assert mod.render_language_tree("ja")
    assert mod.render_language_tree("en")


def test_language_reference_rejects_reordered_ja_sections(tmp_path, monkeypatch):
    """Rejecting control for issue #741: swapping two '## ' sections in
    docs/manual/LANGUAGE.ja.md leaves the section *count* unchanged but breaks positional
    heading correspondence with docs/manual/LANGUAGE.md. The generator must reject this
    instead of silently pairing each Japanese section body with the wrong English
    anchor/blurb. The swapped variant is a tmp_path fixture built from the real
    LANGUAGE.ja.md at test time — the committed docs/manual/LANGUAGE.ja.md is never touched.
    """
    mod = _load_tool()
    ja_text = (REPO_ROOT / "docs" / "manual" / "LANGUAGE.ja.md").read_text(encoding="utf-8")
    sections = mod.split_language_md(ja_text)
    lead = ja_text.split("\n## ", 1)[0]

    # The reconstruction must round-trip byte-identically on the *unswapped*
    # sections before a swapped variant built the same way can be trusted to
    # test what this test claims it tests.
    assert _rejoin_language_md(lead, sections) == ja_text

    idx2 = next(i for i, (h, _) in enumerate(sections) if h.startswith("2."))
    idx3 = next(i for i, (h, _) in enumerate(sections) if h.startswith("3."))
    swapped = list(sections)
    swapped[idx2], swapped[idx3] = swapped[idx3], swapped[idx2]
    assert len(swapped) == len(sections)  # the count check alone would not catch this

    swapped_path = tmp_path / "LANGUAGE.ja.swapped.md"
    swapped_path.write_text(_rejoin_language_md(lead, swapped), encoding="utf-8")
    monkeypatch.setattr(mod, "LANGUAGE_MD_JA", swapped_path)

    with pytest.raises(SystemExit, match=r"section #3.*does not correspond"):
        mod.render_language_tree("ja")


def test_language_reference_rejects_duplicate_en_section_numbers(tmp_path, monkeypatch):
    """Rejecting control for the section-number-uniqueness precondition added
    alongside issue #741's heading-correspondence check: that check only detects a
    docs/manual/LANGUAGE.ja.md reorder if docs/manual/LANGUAGE.md's numeric section prefixes are
    themselves unique. If two English sections shared a number, a matching swap on
    the ja side would be prefix-equal at every position and slip through
    undetected — so uniqueness must be an enforced precondition, not an unstated
    assumption. The duplicate-numbered variant is a tmp_path fixture built from the
    real LANGUAGE.md at test time — the committed docs/manual/LANGUAGE.md is never
    touched. SECTION_BLURBS is patched only to add an entry for the one renamed
    heading, so the unrelated "unknown SECTION_BLURBS entry" check does not
    preempt the check under test.
    """
    mod = _load_tool()
    en_text = (REPO_ROOT / "docs" / "manual" / "LANGUAGE.md").read_text(encoding="utf-8")
    sections = mod.split_language_md(en_text)
    lead = en_text.split("\n## ", 1)[0]

    # Same fidelity guarantee as the reorder control above.
    assert _rejoin_language_md(lead, sections) == en_text

    idx3 = next(i for i, (h, _) in enumerate(sections) if h.startswith("3."))
    heading3, body3 = sections[idx3]
    duplicated_heading = heading3.replace("3.", "2.", 1)
    monkeypatch.setitem(mod.SECTION_BLURBS, duplicated_heading, mod.SECTION_BLURBS[heading3])

    renumbered = list(sections)
    renumbered[idx3] = (duplicated_heading, body3)
    # Now two sections (the original "2." section and this renamed one) share
    # numeric prefix "2" — a duplicate, not a reorder: count is unchanged and no
    # section moved position.
    assert len(renumbered) == len(sections)

    dup_path = tmp_path / "LANGUAGE.duplicate.md"
    dup_path.write_text(_rejoin_language_md(lead, renumbered), encoding="utf-8")
    monkeypatch.setattr(mod, "LANGUAGE_MD", dup_path)

    with pytest.raises(SystemExit, match=r"more than one '## ' section numbered"):
        mod.render_language_tree("ja")


@pytest.mark.parametrize("page_id", ["language", "cli"])
def test_generated_reference_pages_are_fresh(page_id):
    mod = _load_tool()
    for lang in ("ja", "en"):
        committed = (OUT_DIR / f"{page_id}.{lang}.html").read_text(encoding="utf-8")
        if page_id == "language":
            tree = mod.render_language_tree(lang)
            source_note = "docs/manual/LANGUAGE.ja.md" if lang == "ja" else "docs/manual/LANGUAGE.md"
        else:
            tree = mod.render_cli_tree()
            source_note = "rust/fslc/cli-contract.json"
        fresh = mod.page_shell(page_id, lang, tree, source_note)
        assert fresh == committed, (
            f"docs/intro/{page_id}.{lang}.html is stale — run "
            "`python tools/build_site_reference.py` and commit the result."
        )


@pytest.mark.parametrize("lang", ["en", "ja"])
def test_checked_manual_example_keeps_following_prose_and_table_outside_code(lang):
    """Detect the real checked fence becoming prose and swallowing the rules table."""
    mod = _load_tool()
    source = (mod.LANGUAGE_MD if lang == "en" else mod.LANGUAGE_MD_JA).read_text(
        encoding="utf-8"
    )
    examples = re.findall(r"^```fsl check\n(.*?)^```$", source, flags=re.M | re.S)
    assert len(examples) == 1
    tree = mod.render_language_tree(lang)
    escaped = html.escape(examples[0], quote=False).replace('"', "&quot;")
    assert f'<pre><code class="language-fsl">{escaped}</code></pre>' in tree
    outside_code = re.sub(r"<pre\b[^>]*>.*?</pre>", "", tree, flags=re.S)
    label = "Entities and concepts." if lang == "en" else "エンティティと区分。"
    assert f"<p><strong>{label}</strong>" in outside_code
    assert "<td><code>allow Actor verb Entity only when condition</code></td>" in outside_code
    for anchor in ("fsl-editions", "fsl5-requirements", "fsl4-to-fsl5-migration"):
        assert outside_code.count(f'id="{anchor}"') == 1
        assert f'href="#{anchor}"' in outside_code or anchor == "fsl-editions"


def _render_fence_snippet(tmp_path, monkeypatch, snippet):
    mod = _load_tool()
    original = mod.LANGUAGE_MD.read_text(encoding="utf-8")
    sections = mod.split_language_md(original)
    lead = original.split("\n## ", 1)[0]
    assert _rejoin_language_md(lead, sections) == original
    heading, body = sections[0]
    sections[0] = (heading, f"\n\n{snippet}\n\n{body}")
    path = tmp_path / "LANGUAGE.fence.md"
    path.write_text(_rejoin_language_md(lead, sections), encoding="utf-8")
    monkeypatch.setattr(mod, "LANGUAGE_MD", path)
    return mod.render_language_tree("en")


@pytest.mark.parametrize("fence", ["```", "~~~"])
def test_checked_fence_preserves_escaping_and_following_prose(tmp_path, monkeypatch, fence):
    code = '#1234 <tag> & "quoted"\n'
    tree = _render_fence_snippet(
        tmp_path, monkeypatch, f"{fence}fsl check\n{code}{fence}\n\nAfter checked code."
    )
    assert (
        '<pre><code class="language-fsl">#1234 &lt;tag&gt; &amp; &quot;quoted&quot;\n</code></pre>'
        in tree
    )
    assert "<p>After checked code.</p>" in tree


def test_fence_normalization_preserves_fragments_other_info_and_nested_markers(
    tmp_path, monkeypatch
):
    """Preservation control: these render the same before the checked-fence fix."""
    snippet = '''```fsl-fragment
fragment <value>
```

```fsl
ordinary <value>
```

```python
print("<value>")
```

````text
```fsl check
literal <value>
```
````

After ordinary fences.

```python check
Other info remains unsupported.
```
'''
    tree = _render_fence_snippet(tmp_path, monkeypatch, snippet)
    assert '<pre><code class="language-fsl-fragment">fragment &lt;value&gt;\n</code></pre>' in tree
    assert '<pre><code class="language-fsl">ordinary &lt;value&gt;\n</code></pre>' in tree
    assert '<pre><code class="language-python">print(&quot;&lt;value&gt;&quot;)\n</code></pre>' in tree
    assert (
        '<pre><code class="language-text">```fsl check\nliteral &lt;value&gt;\n```\n</code></pre>'
        in tree
    )
    assert "<p>After ordinary fences.</p>" in tree
    assert "<p><code>python check\nOther info remains unsupported.</code></p>" in tree


def test_checked_fence_normalization_respects_literate_closing_delimiters():
    # A shorter same-character run and a different delimiter do not end the
    # outer block. A longer run does, so the following real marker is normalized.
    literal = "````text\n```fsl check\n```\n~~~\n```fsl check\n`````\n"
    checked = "~~~ fsl check\n<tag>\n~~~\n"
    assert _load_tool()._renderable_fsl_fences(literal + checked) == (
        literal + "~~~fsl\n<tag>\n~~~\n"
    )
