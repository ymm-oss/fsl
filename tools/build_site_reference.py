# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Generate the two site "generated reference" pages from their canonical sources.

    docs/intro/language.{ja,en}.html  <-  docs/LANGUAGE.md
    docs/intro/cli.{ja,en}.html       <-  src/fslc/cli.py (argparse introspection)

Design contract (see docs/DESIGN-docs-site.md D3/D4/D5 and the fable decision on the
"canonical language problem"): the English body of LANGUAGE.md is reused verbatim for
both the ja and en pages — no translation, no second source to keep in sync. The ja page
only adds a Japanese lead paragraph and a Japanese one-line description per top-level
section, sourced from SECTION_BLURBS below. If LANGUAGE.md grows a top-level ("## ")
section that isn't in SECTION_BLURBS, this script fails loudly instead of silently
shipping an incomplete reference — the dictionary is where the "language feature moves
all of its files together" rule (CLAUDE.md) reaches this site.

Output is deterministic (no timestamps/commit hashes) so regeneration produces a clean,
reviewable diff only when the sources actually changed. Run after any change to
LANGUAGE.md or the fslc CLI surface:

    python tools/build_site_reference.py
"""

from __future__ import annotations

import argparse
import html
import inspect
import re
import sys
from pathlib import Path

import markdown

REPO_ROOT = Path(__file__).resolve().parent.parent
LANGUAGE_MD = REPO_ROOT / "docs" / "LANGUAGE.md"
OUT_DIR = REPO_ROOT / "docs" / "intro"

sys.path.insert(0, str(REPO_ROOT / "src"))

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
    text = LANGUAGE_MD.read_text(encoding="utf-8")
    sections = split_language_md(text)
    unknown = [h for h, _ in sections if h not in SECTION_BLURBS]
    if unknown:
        raise SystemExit(
            "build_site_reference: docs/LANGUAGE.md has section(s) with no entry in "
            f"SECTION_BLURBS: {unknown!r}. Add a ja/en one-line description to "
            "tools/build_site_reference.py:SECTION_BLURBS before regenerating "
            "(this is the connective-tissue check for the 'a language feature moves "
            "all of its files together' rule reaching the site)."
        )
    md = markdown.Markdown(extensions=["tables", "fenced_code", "toc"])
    nodes = []
    for heading, body in sections:
        md.reset()
        body_html = _rewrite_relative_md_links(md.convert(body))
        slug = slugify(heading)
        blurb = SECTION_BLURBS[heading][lang]
        nodes.append(
            f'<details id="{slug}"><summary>{html.escape(heading)}'
            f'<span class="tree-blurb"> — {html.escape(blurb)}</span></summary>'
            f'<div class="tree-body">{body_html}</div></details>'
        )
    return '<div class="disclosure-tree">' + "\n".join(nodes) + "</div>"


def get_subparsers_action(parser: argparse.ArgumentParser):
    for action in parser._actions:  # noqa: SLF001 - intentional argparse introspection
        if isinstance(action, argparse._SubParsersAction):  # noqa: SLF001
            return action
    return None


# argparse's own group heading changed from "optional arguments:" to
# "options:" in Python 3.10 (bpo-9694) — purely cosmetic, but it makes
# format_help() output depend on which Python generated this page. CI runs
# the full suite (and this generator) on multiple Python versions
# (pyproject: requires-python >=3.9), so normalize to the modern spelling
# rather than letting the committed page's exact bytes depend on which
# interpreter happened to regenerate it last.
def _normalize_argparse_help(text: str) -> str:
    return text.replace("optional arguments:", "options:")


def render_cli_tree() -> str:
    from fslc.cli import _build_arg_parser, exit_code, _envelope, _error_envelope  # noqa: PLC0415

    top = _build_arg_parser()
    top_sub = get_subparsers_action(top)
    nodes = []
    for name, sub in top_sub.choices.items():
        if name == "version":
            continue
        nested = get_subparsers_action(sub)
        if nested:
            children = []
            for name2, sub2 in nested.choices.items():
                help_text = html.escape(_normalize_argparse_help(sub2.format_help()))
                children.append(
                    f'<details><summary>fslc {html.escape(name)} {html.escape(name2)}</summary>'
                    f'<div class="tree-body"><pre>{help_text}</pre></div></details>'
                )
            body = '<div class="disclosure-tree">' + "\n".join(children) + "</div>"
            nodes.append(
                f'<details><summary>fslc {html.escape(name)} <span class="tree-blurb">'
                f"— {len(nested.choices)} subcommands</span></summary>"
                f'<div class="tree-body">{body}</div></details>'
            )
        else:
            help_text = html.escape(_normalize_argparse_help(sub.format_help()))
            nodes.append(
                f'<details><summary>fslc {html.escape(name)}</summary>'
                f'<div class="tree-body"><pre>{help_text}</pre></div></details>'
            )
    tree = '<div class="disclosure-tree">' + "\n".join(nodes) + "</div>"

    exit_code_src = html.escape(inspect.getsource(exit_code))
    envelope_src = html.escape(inspect.getsource(_envelope) + "\n\n" + inspect.getsource(_error_envelope))
    contract = (
        '<details open><summary>Exit codes &amp; JSON envelope <span class="tree-blurb">'
        "— exit_code() / _envelope() / _error_envelope(), from src/fslc/cli.py</span></summary>"
        '<div class="tree-body">'
        "<p>Every command prints one JSON object to stdout and maps its <code>result</code> "
        "field to a process exit code through this function — verbatim from the source, "
        "so this table cannot drift from the actual contract:</p>"
        f"<pre>{exit_code_src}</pre>"
        "<p>Every result is wrapped by <code>_envelope()</code> (adds <code>{&quot;fsl&quot;: "
        '&quot;1.0&quot;, ...}</code> + faithfulness metadata); parse/name/type/semantics/io '
        "errors additionally go through <code>_error_envelope()</code>:</p>"
        f"<pre>{envelope_src}</pre>"
        "</div></details>"
    )
    return contract + tree


PAGE_STRINGS = {
    "language": {
        "ja": {
            "title": "FSL 言語リファレンス — LANGUAGE.md から生成",
            "description": "docs/LANGUAGE.md から生成される、FSLの網羅的な言語リファレンス。",
            "kicker": "Generated Reference",
            "h1": "言語リファレンス",
            "lead": (
                "これは <code>docs/LANGUAGE.md</code> からの生成物です。正典は英語の本文で、"
                "翻訳は行いません — FSLの予約語・エラーメッセージ・JSON出力はすべて英語であり、"
                "訳を作ると <code>LANGUAGE.md</code> / このページ / <code>skills/fsl/reference.md</code> "
                "の三重管理に戻ってしまうためです。各節の日本語1行説明は目次として付けています。"
            ),
            "badge": "Generated from LANGUAGE.md",
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
            "title": "FSL CLI リファレンス — fslc の全コマンド",
            "description": "src/fslc/cli.py から生成される、fslcの全サブコマンド・終了コード・JSON契約のリファレンス。",
            "kicker": "Generated Reference",
            "h1": "CLI リファレンス",
            "lead": (
                "これは <code>src/fslc/cli.py</code> の argparse 定義から生成されています。"
                "各コマンドの使い方はPython自身の <code>--help</code> 出力そのものなので、"
                "手書きの一覧が実装から取り残されることはありません。"
            ),
            "badge": "Generated from cli.py",
            "expand": "すべて展開",
            "collapse": "すべて折りたたむ",
            "top": "↑ 先頭へ",
        },
        "en": {
            "title": "FSL CLI Reference — every fslc subcommand",
            "description": "The full fslc CLI surface, exit codes, and JSON contract, generated from src/fslc/cli.py.",
            "kicker": "Generated Reference",
            "h1": "CLI Reference",
            "lead": (
                "Generated directly from the argparse definitions in <code>src/fslc/cli.py</code> — "
                "each command's usage is Python's own <code>--help</code> output, so this page "
                "cannot lag the implementation."
            ),
            "badge": "Generated from cli.py",
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
        (OUT_DIR / f"language.{lang}.html").write_text(
            page_shell("language", lang, tree, "docs/LANGUAGE.md"), encoding="utf-8"
        )
    cli_tree = render_cli_tree()
    for lang in ("ja", "en"):
        (OUT_DIR / f"cli.{lang}.html").write_text(
            page_shell("cli", lang, cli_tree, "src/fslc/cli.py"), encoding="utf-8"
        )
    print("Generated docs/intro/{language,cli}.{ja,en}.html")


if __name__ == "__main__":
    main()
