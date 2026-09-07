# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Generate the two site "generated reference" pages from their canonical sources.

    docs/intro/language.{ja,en}.html  <-  docs/LANGUAGE.md
    docs/intro/cli.{ja,en}.html       <-  rust/fslc/cli-contract.json (native CLI)

Design contract (see docs/DESIGN-docs-site.md D3/D4/D5/D7): the ja page body is rendered
from docs/LANGUAGE.ja.md, a second canonical source kept section-aligned 1:1 with
docs/LANGUAGE.md (same count and order of "## " sections; see D7 for why this replaced
the earlier "no translation" stance). Section anchors (`id=`) and blurb lookups always key
off the English heading text so cross-page links and SECTION_BLURBS stay stable regardless
of language. The ja page also adds a Japanese lead paragraph and a Japanese one-line
description per top-level section, sourced from SECTION_BLURBS below. If LANGUAGE.md grows
a top-level ("## ") section that isn't in SECTION_BLURBS, or LANGUAGE.md/LANGUAGE.ja.md fall
out of section-count sync, or a same-count LANGUAGE.ja.md reorders sections relative to
LANGUAGE.md (checked by comparing each positional pair's leading numeric section prefix; see
issue #741), this script fails loudly instead of silently shipping an incomplete or
misaligned reference — this is where the "language feature moves all of its files together"
rule (CLAUDE.md) reaches this site.

Output is deterministic (no timestamps/commit hashes) so regeneration produces a clean,
reviewable diff only when the sources actually changed. Run after any change to
LANGUAGE.md or the fslc CLI surface:

    python tools/build_site_reference.py
"""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

import markdown

REPO_ROOT = Path(__file__).resolve().parent.parent
LANGUAGE_MD = REPO_ROOT / "docs" / "LANGUAGE.md"
LANGUAGE_MD_JA = REPO_ROOT / "docs" / "LANGUAGE.ja.md"
CLI_CONTRACT_JSON = REPO_ROOT / "rust" / "fslc" / "cli-contract.json"
OUT_DIR = REPO_ROOT / "docs" / "intro"

sys.path.insert(0, str(REPO_ROOT / "src"))

from fslc.cli_help import normalize_argparse_help as _normalize_argparse_help  # noqa: E402

GENERATED_BANNER = (
    "<!-- GENERATED — do not edit by hand. Regenerate with:\n"
    "     python tools/build_site_reference.py\n"
    "     Source: {source} -->"
)

# One entry per top-level ("## ") heading in LANGUAGE.md, keyed by the exact heading
# text. Both langs are one-line descriptions, not translations of the section body.
SECTION_BLURBS = {
    "Design principles": {
        "ja": "設計原則 G1〜G5 と、型システムの立場。",
        "en": "Design principles G1-G5 and the type-system stance.",
    },
    "1. Structure of a specification": {
        "ja": "spec 宣言・状態・初期状態・アクションの基本構造。",
        "en": "The declaration / state / init / action shape of a spec.",
    },
    "2. Types": {
        "ja": "基本型、列挙、構造体、Seq、Option などの型システム。",
        "en": "Primitive types, enums, structs, Seq, Option.",
    },
    "3. Expressions": {
        "ja": "式の文法 — 演算子、量化子、関数呼び出しなど。",
        "en": "Expression grammar — operators, quantifiers, calls.",
    },
    "4. Statements (init / action bodies)": {
        "ja": "init と action の本体で使える文。",
        "en": "Statements usable inside init/action bodies.",
    },
    "5. Semantics": {
        "ja": "状態遷移の意味論 — 有効化・実行・stutter。",
        "en": "Transition semantics — enabling, firing, stutter.",
    },
    "6. Automatic checks (things checked even without being written)": {
        "ja": "書かなくても検証器が自動で行う検査。",
        "en": "Checks the verifier runs even if you never wrote them.",
    },
    "7. The verifier `fslc`": {
        "ja": "fslc コマンド一覧、結果種別、終了コード、カバレッジ診断。",
        "en": "The fslc CLI surface, result kinds, exit codes, coverage diagnosis.",
    },
    "8. Recommended workflow: make proved the standard": {
        "ja": "BMC から帰納法へ、proved を標準にする推奨ワークフロー。",
        "en": "The recommended BMC-to-induction workflow toward proved.",
    },
    "9. Idiom collection": {
        "ja": "よく使うイディオム集 — Option、Seq集約、ghost変数など。",
        "en": "Common idioms — Option, Seq aggregation, ghost variables.",
    },
    "10. Refinement (fidelity of a detailed spec)": {
        "ja": "詳細化 — 実装が抽象仕様に忠実かどうかの検査。",
        "en": "Refinement checking between a detailed and an abstract spec.",
    },
    "11. Composition (compose)": {
        "ja": "名前空間と同期アクションによるスペックの合成。",
        "en": "Composing specs via namespaces and synchronized actions.",
    },
    "12. The bridge to implementation": {
        "ja": "実装への接続 — replay、testgen、具体インタプリタ。",
        "en": "The bridge to real implementations — replay, testgen, the concrete interpreter.",
    },
    "13. The three-layer dialects (consulting / requirements / design) and traceability": {
        "ja": "業務・要件・設計の3層ダイアレクトとトレーサビリティ。",
        "en": "The business/requirements/design dialects and cross-layer traceability.",
    },
    "14. Library API": {
        "ja": "Python ライブラリとして呼び出す API。",
        "en": "The Python library API.",
    },
    "15. Validation suite (the spec ≠ intent gap)": {
        "ja": "仕様が意図から外れていないかを検査する一式 — mutate/explain/analyze。",
        "en": "The suite that checks the spec matches intent — mutate, explain, analyze.",
    },
    "16. Promotion judgment to ghost types (typestate)": {
        "ja": "状態機械を ghost 型 / typestate へ昇格させる判断。",
        "en": "Deciding when a state machine promotes to a ghost type / typestate.",
    },
    "17. The causal profile (review-only causal hypothesis graphs)": {
        "ja": "レビュー専用の因果仮説グラフ — proved/verified は決して付かない。",
        "en": "Review-only causal hypothesis graphs — never proved/verified.",
    },
}


def slugify(text: str) -> str:
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = text.lower()
    text = re.sub(r"[^a-z0-9぀-ヿ一-鿿]+", "-", text)
    return text.strip("-")


def split_language_md(text: str):
    """Split LANGUAGE.md into (heading_text, body_markdown) for each '## ' section."""
    lines = text.splitlines()
    sections = []
    current_heading = None
    current_body: list[str] = []
    for line in lines:
        m = re.match(r"^## (.+)$", line)
        if m:
            if current_heading is not None:
                sections.append((current_heading, "\n".join(current_body)))
            current_heading = m.group(1).strip()
            current_body = []
        elif current_heading is not None:
            current_body.append(line)
    if current_heading is not None:
        sections.append((current_heading, "\n".join(current_body)))
    return sections


_SECTION_NUMBER = re.compile(r"^(\d+)\.")


def _section_number(heading: str) -> str | None:
    """Return a '## ' heading's leading numeric prefix (e.g. "2" from "2. Types"),
    or None for an unnumbered heading (the lead "Design principles" / "設計原則" section).
    """
    m = _SECTION_NUMBER.match(heading)
    return m.group(1) if m else None


GITHUB_BLOB = "https://github.com/ymm-oss/fsl/blob/main/docs/"

# LANGUAGE.md links to sibling docs/ files with bare relative hrefs
# ("DESIGN-forbidden.md"), which is correct from docs/LANGUAGE.md itself but
# wrong once embedded in docs/intro/language.*.html — and .md files render as
# plain text on GitHub Pages anyway (docs/.nojekyll). Rewrite them to GitHub
# blob URLs so they work from wherever this generated page is read.
_RELATIVE_MD_LINK = re.compile(r'href="([A-Za-z0-9_.-]+\.md)"')


def _rewrite_relative_md_links(html_text: str) -> str:
    return _RELATIVE_MD_LINK.sub(lambda m: f'href="{GITHUB_BLOB}{m.group(1)}"', html_text)


def render_language_tree(lang: str) -> str:
    en_text = LANGUAGE_MD.read_text(encoding="utf-8")
    en_sections = split_language_md(en_text)
    unknown = [h for h, _ in en_sections if h not in SECTION_BLURBS]
    if unknown:
        raise SystemExit(
            "build_site_reference: docs/LANGUAGE.md has section(s) with no entry in "
            f"SECTION_BLURBS: {unknown!r}. Add a ja/en one-line description to "
            "tools/build_site_reference.py:SECTION_BLURBS before regenerating "
            "(this is the connective-tissue check for the 'a language feature moves "
            "all of its files together' rule reaching the site)."
        )

    if lang == "ja":
        ja_text = LANGUAGE_MD_JA.read_text(encoding="utf-8")
        render_sections = split_language_md(ja_text)
        if len(render_sections) != len(en_sections):
            raise SystemExit(
                "build_site_reference: docs/LANGUAGE.ja.md has "
                f"{len(render_sections)} '## ' section(s) but docs/LANGUAGE.md has "
                f"{len(en_sections)}. The two files must stay section-aligned 1:1 "
                "(same count, same order) — reconcile docs/LANGUAGE.ja.md with the "
                "current docs/LANGUAGE.md before regenerating (see docs/DESIGN-docs-site.md D7)."
            )

        # The per-position correspondence check below only detects a reorder if
        # docs/LANGUAGE.md's numeric section prefixes are unique — two English
        # sections sharing a number would make a matching ja-side swap
        # prefix-equal at every position and slip through undetected. Assert the
        # precondition instead of silently relying on it (docs/DESIGN-docs-site.md
        # D7's "unique section numbers" addendum).
        numbers_to_headings: dict[str, list[str]] = {}
        for en_heading, _ in en_sections:
            number = _section_number(en_heading)
            if number is not None:
                numbers_to_headings.setdefault(number, []).append(en_heading)
        duplicates = {n: hs for n, hs in numbers_to_headings.items() if len(hs) > 1}
        if duplicates:
            number, headings = sorted(duplicates.items())[0]
            raise SystemExit(
                "build_site_reference: docs/LANGUAGE.md has more than one '## ' "
                f"section numbered {number!r}: {headings!r}. The positional "
                "heading-correspondence check assumes docs/LANGUAGE.md's section "
                "numbers are unique in order to detect a docs/LANGUAGE.ja.md reorder "
                "— renumber the duplicate section(s) in docs/LANGUAGE.md before "
                "regenerating (see docs/DESIGN-docs-site.md D7)."
            )

        for position, ((en_heading, _), (ja_heading, _)) in enumerate(
            zip(en_sections, render_sections), start=1
        ):
            en_number = _section_number(en_heading)
            ja_number = _section_number(ja_heading)
            if en_number != ja_number:
                raise SystemExit(
                    "build_site_reference: docs/LANGUAGE.ja.md section "
                    f"#{position} ({ja_heading!r}) does not correspond to "
                    f"docs/LANGUAGE.md section #{position} ({en_heading!r}). The two files "
                    "must stay section-aligned 1:1 (same count, same order) — reconcile "
                    "docs/LANGUAGE.ja.md with the current docs/LANGUAGE.md before "
                    "regenerating (see docs/DESIGN-docs-site.md D7)."
                )
    else:
        render_sections = en_sections

    md = markdown.Markdown(extensions=["tables", "fenced_code", "toc"])
    nodes = []
    for (en_heading, _), (heading, body) in zip(en_sections, render_sections):
        md.reset()
        body_html = _rewrite_relative_md_links(md.convert(body))
        # Anchors and blurb lookups always key off the English heading so cross-page
        # links and SECTION_BLURBS stay stable regardless of which language renders.
        slug = slugify(en_heading)
        blurb = SECTION_BLURBS[en_heading][lang]
        nodes.append(
            f'<details id="{slug}"><summary>{html.escape(heading)}'
            f'<span class="tree-blurb"> — {html.escape(blurb)}</span></summary>'
            f'<div class="tree-body">{body_html}</div></details>'
        )
    return '<div class="disclosure-tree">' + "\n".join(nodes) + "</div>"


def _load_cli_contract() -> dict:
    import json

    return json.loads(CLI_CONTRACT_JSON.read_text(encoding="utf-8"))


def _extract_exit_codes_paragraph() -> str:
    text = LANGUAGE_MD.read_text(encoding="utf-8")
    match = re.search(
        r"(Exit codes:.*?)(?=\n\n`approval_check`|\n\n### )",
        text,
        flags=re.S,
    )
    if not match:
        raise SystemExit(
            "build_site_reference: could not locate the Exit codes paragraph in "
            "docs/LANGUAGE.md — reconcile LANGUAGE.md before regenerating."
        )
    return match.group(1).strip()


def _render_cli_command_nodes(node: dict, *, skip_top_level: tuple[str, ...] = ("version",)) -> str:
    nodes = []
    for command in node.get("commands", []):
        path = command.get("path", [])
        if not path:
            continue
        name = path[-1]
        if len(path) == 1 and name in skip_top_level:
            continue
        label = "fslc " + " ".join(html.escape(part) for part in path)
        children = command.get("commands", [])
        if children:
            child_body = _render_cli_command_nodes(command, skip_top_level=())
            # The parent carries its own help in the contract. Rendering only the child
            # list would drop it, and the page's lead promises that each command's usage
            # is the native --help output.
            own_help = command.get("help", "")
            own = f"<pre>{html.escape(own_help)}</pre>" if own_help else ""
            body = own + '<div class="disclosure-tree">' + child_body + "</div>"
            nodes.append(
                f'<details><summary>{label} <span class="tree-blurb">'
                f"— {len(children)} subcommands</span></summary>"
                f'<div class="tree-body">{body}</div></details>'
            )
        else:
            help_text = html.escape(command.get("help", ""))
            nodes.append(
                f"<details><summary>{label}</summary>"
                f'<div class="tree-body"><pre>{help_text}</pre></div></details>'
            )
    return "\n".join(nodes)


def render_cli_tree() -> str:
    contract = _load_cli_contract()
    root = contract["root"]
    exit_codes = html.escape(_extract_exit_codes_paragraph())
    contract_block = (
        '<details open><summary>Exit codes &amp; JSON envelope <span class="tree-blurb">'
        "— docs/LANGUAGE.md + rust/fslc/src/outcome.rs::exit_status()</span></summary>"
        '<div class="tree-body">'
        "<p>The authoritative native CLI maps every JSON <code>result</code> to a process "
        "exit code through <code>rust/fslc/src/outcome.rs</code> "
        "<code>exit_status()</code>, which implements the table in "
        "<code>docs/LANGUAGE.md</code>:</p>"
        f"<pre>{exit_codes}</pre>"
        "<p>Verdict-bearing commands print one JSON object to stdout with "
        '<code>{"fsl":"1.0", ...}</code>, and the exit code is derived from its '
        "<code>result</code>. Commands that emit a generated artifact write that "
        "artifact to stdout <em>under some flag combinations</em> instead — "
        "<code>fslc fmt PATH</code> prints formatted source, while "
        "<code>fslc fmt --check</code> returns a <code>format_check</code> envelope, and "
        "<code>document</code>, <code>db</code>, and <code>domain</code> subcommands print "
        "generated content or return an envelope depending on whether an output path is "
        "given. The rule is per command and per flag, so read a command's own "
        "<code>--help</code> before parsing its stdout as JSON. Native CLI and browser Worker "
        "<code>check</code>/<code>verify</code> envelopes also include "
        "<code>versions.verifier</code>, <code>versions.core</code>, and "
        "<code>versions.solver</code> (see <code>docs/LANGUAGE.md</code> §14). "
        "The machine-readable schema is "
        "<code>schemas/fslc/envelope.v1.schema.json</code>. "
        "The frozen Python compatibility reference under <code>src/fslc/</code> "
        "mirrors a subset for parity tests only — it is not the distribution surface.</p>"
        "</div></details>"
    )
    tree = (
        '<div class="disclosure-tree">'
        + _render_cli_command_nodes(root)
        + "</div>"
    )
    return contract_block + tree


PAGE_STRINGS = {
    "language": {
        "ja": {
            "title": "FSL 言語リファレンス — LANGUAGE.ja.md から生成",
            "description": "docs/LANGUAGE.ja.md から生成される、FSLの網羅的な言語リファレンス(日本語版)。",
            "kicker": "Generated Reference",
            "h1": "言語リファレンス",
            "lead": (
                "これは <code>docs/LANGUAGE.ja.md</code> からの生成物です。"
                "<code>docs/LANGUAGE.md</code>(英語)と節単位で1対1対応する第二の正典として"
                "保守されています。FSLの予約語・コマンド名・診断コード・JSON出力などは"
                "そのまま英語で表記しています。"
            ),
            "badge": "Generated from LANGUAGE.ja.md",
            "expand": "すべて展開",
            "collapse": "すべて折りたたむ",
            "top": "↑ 先頭へ",
        },
        "en": {
            "title": "FSL Language Reference — generated from LANGUAGE.md",
            "description": "The exhaustive FSL language reference, generated from docs/LANGUAGE.md.",
            "kicker": "Generated Reference",
            "h1": "Language Reference",
            "lead": (
                "Generated from <code>docs/LANGUAGE.md</code>, reproduced verbatim — this is the "
                "canonical source, not a copy that can drift from it."
            ),
            "badge": "Generated from LANGUAGE.md",
            "expand": "Expand all",
            "collapse": "Collapse all",
            "top": "↑ Top",
        },
    },
    "cli": {
        "ja": {
            "title": "FSL CLI リファレンス — ネイティブ fslc のコマンド一覧",
            "description": "rust/fslc/cli-contract.json から生成される、ネイティブ fslc のコマンド・終了コード・JSON契約のリファレンス（version を除く）。",
            "kicker": "Generated Reference",
            "h1": "CLI リファレンス",
            "lead": (
                "これは権威あるネイティブ CLI 契約 <code>rust/fslc/cli-contract.json</code> "
                "から生成されています。各コマンドの使い方はネイティブ <code>fslc</code> の "
                "<code>--help</code> 出力そのものです。"
                "<code>src/fslc/</code> の凍結 Python 互換参照は配布面ではなく、"
                "明示的な parity テスト用のみに言及します。"
            ),
            "badge": "Generated from cli-contract.json",
            "expand": "すべて展開",
            "collapse": "すべて折りたたむ",
            "top": "↑ 先頭へ",
        },
        "en": {
            "title": "FSL CLI Reference — the native fslc command surface",
            "description": "The native fslc CLI surface except version, with exit codes and the JSON contract, generated from rust/fslc/cli-contract.json.",
            "kicker": "Generated Reference",
            "h1": "CLI Reference",
            "lead": (
                "Generated from the authoritative native CLI contract "
                "<code>rust/fslc/cli-contract.json</code> — each command's usage is the native "
                "<code>fslc --help</code> output. The frozen Python compatibility reference "
                "under <code>src/fslc/</code> is mentioned only where parity testing requires it; "
                "it is not the distribution surface."
            ),
            "badge": "Generated from cli-contract.json",
            "expand": "Expand all",
            "collapse": "Collapse all",
            "top": "↑ Top",
        },
    },
}

NAV_LABELS = {"ja": {"brand": "Manual"}, "en": {"brand": "Manual"}}


def page_shell(page_id: str, lang: str, tree_html: str, source_note: str) -> str:
    s = PAGE_STRINGS[page_id][lang]
    other = "en" if lang == "ja" else "ja"
    return f"""<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>{html.escape(s['title'])}</title>
<meta name="description" content="{html.escape(s['description'])}" />
<link rel="stylesheet" href="assets/site.css" />
</head>
<body class="docs-page" data-page="{page_id}">
{GENERATED_BANNER.format(source=source_note)}
<div class="progress"></div>

<header class="topbar" data-nav></header>

<aside class="docs-sidebar" data-nav></aside>

<main>

<section id="{page_id}-top">
  <div class="wrap">
    <nav class="breadcrumb" data-nav></nav>
    <p class="kicker mono reveal">{html.escape(s['kicker'])}</p>
    <span class="badge ref reveal">{html.escape(s['badge'])}</span>
    <h1 class="reveal" style="margin-top:14px">{html.escape(s['h1'])}</h1>
    <p class="lead narrow reveal">{s['lead']}</p>
    <div class="tree-controls reveal">
      <button class="btn op-expand" type="button">{html.escape(s['expand'])}</button>
      <button class="btn op-collapse" type="button">{html.escape(s['collapse'])}</button>
    </div>
    {tree_html}
  </div>
</section>

</main>

<a class="back-to-top" href="#{page_id}-top">{html.escape(s['top'])}</a>

<footer data-nav></footer>

<script src="assets/site.js"></script>
</body>
</html>
"""


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for lang in ("ja", "en"):
        tree = render_language_tree(lang)
        source_note = "docs/LANGUAGE.ja.md" if lang == "ja" else "docs/LANGUAGE.md"
        (OUT_DIR / f"language.{lang}.html").write_text(
            page_shell("language", lang, tree, source_note), encoding="utf-8"
        )
    cli_tree = render_cli_tree()
    for lang in ("ja", "en"):
        (OUT_DIR / f"cli.{lang}.html").write_text(
            page_shell("cli", lang, cli_tree, "rust/fslc/cli-contract.json"),
            encoding="utf-8",
        )
    print("Generated docs/intro/{language,cli}.{ja,en}.html")


if __name__ == "__main__":
    main()
