# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Deterministic structural contracts for the sitewide refresh integration."""
from __future__ import annotations

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
INTRO = ROOT / "docs" / "intro"
CSS = INTRO / "assets" / "site.css"
JS = INTRO / "assets" / "site.js"
HOOK = 'classList.add("site-refreshed")'
PLAYGROUND = (INTRO / "playground.en.html", INTRO / "playground.ja.html")
PLAYGROUND_IDS = ("fsl-source", "fsl-verify", "fsl-cancel", "fsl-output", "fsl-progress")
REPRESENTATIVES = {
    "home": INTRO / "index.en.html",
    "hub": INTRO / "get-started.en.html",
    "chapter": INTRO / "concept.en.html",
    "generated": INTRO / "language.en.html",
}
SHELL_MARKERS = (
    "body.site-refreshed",
    "body.site-refreshed.docs-page:not(.site-home)",
    ".docs-sidebar",
    ".chapter-card",
    ".disclosure-tree",
)
PG_CSS = "body.site-refreshed.playground-page"

BACKBONE_STAGE_IDS = ("business", "requirements", "design", "observe")
BACKBONE_JS_MARKERS = (
    "BACKBONE_STAGES",
    "PAGE_BACKBONE_FOCUS",
    "initBackbone();",
    'setAttribute("data-backbone"',
    "correctness-backbone",
    "implements.result",
)

HUB_STEMS = ("get-started", "guides", "reference", "examples-background")
HUB_JOURNEY_STEPS = ("audience", "contract", "evidence", "next")
REFERENCE_HUB_MARKERS = ("LANGUAGE.md", "cli-contract.json", "frozen-compat", "exit-interpret")
EXAMPLES_JOURNEY_MARKERS = (
    'data-journey="choose-goal"',
    'data-journey="inspect-evidence"',
    'id="correctness-chain"',
)
GUIDE_JOURNEY_MARKERS = (
    'data-journey="bounded-verify"',
    'data-journey="proof-induction"',
    'data-journey="refinement"',
    'data-journey="implementation-observe"',
)

SITE_STEMS = (
    "ai", "analysis", "business-layer", "cli", "concept", "db", "design-layer",
    "design-notes", "domain", "errors", "examples", "examples-background",
    "get-started", "glossary", "guide", "guides", "index", "language", "mechanism",
    "playground", "quickstart", "reference", "requirements-layer", "syntax",
    "when-to-use",
)

PY_CLI_AUTHORITY_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (re.compile(r"Source:\s*src/fslc/cli\.py"), "generator banner cites frozen Python as source"),
    (re.compile(r"Generated from cli\.py", re.I), "badge cites cli.py as authority"),
    (
        re.compile(r"exit_code\(\)\s*/\s*_envelope\(\)"),
        "cites frozen-Python envelope helpers as current authority",
    ),
    (
        re.compile(r"argparse.*src/fslc/cli\.py|src/fslc/cli\.py.*argparse", re.I),
        "presents Python argparse as the current CLI authority",
    ),
    (
        re.compile(r"generated from src/fslc/cli\.py", re.I),
        "meta description cites cli.py generation",
    ),
)

PALETTE_PROPS = (
    "--lp-bg", "--lp-surface", "--lp-ink", "--lp-muted", "--lp-line",
    "--lp-amber", "--lp-teal", "--lp-glow",
    "--bg", "--surface", "--ink", "--muted", "--line", "--brand", "--accent",
)


def _body_class(text: str) -> str:
    m = re.search(r'<body[^>]*class="([^"]*)"', text)
    return m.group(1) if m else ""


def _loads_shared_asset_links(text: str) -> bool:
    return 'href="assets/site.css"' in text and 'src="assets/site.js"' in text


def _loads_shared_assets(path: Path) -> bool:
    return _loads_shared_asset_links(path.read_text(encoding="utf-8"))


def _starts_regex_literal(src: str, i: int) -> bool:
    """Whether src[i] == "/" opens a regex literal rather than a comment or division.

    Needed because a regex may legally contain "//" or "/*", which the comment
    scanner would otherwise treat as the start of a comment and blank out real code.
    Decided from the previous significant character: after a value (identifier,
    literal, closing bracket) a "/" is division; after an operator, an opening
    bracket, or a keyword it opens a regex.
    """
    if i + 1 >= len(src) or src[i + 1] in "/*":
        return False
    j = i - 1
    while j >= 0 and src[j] in " \t":
        j -= 1
    if j < 0:
        return True
    prev = src[j]
    if prev in "=(,:[!&|?{};\n+-*%~^<>":
        return True
    for keyword in ("return", "typeof", "case", "in", "of", "new", "delete", "void"):
        if src[: j + 1].endswith(keyword) and (
            j + 1 - len(keyword) == 0 or not (src[j - len(keyword)].isalnum() or src[j - len(keyword)] == "_")
        ):
            return True
    return False


def strip_js_comments(js_text: str) -> str:
    """Blank out // and /* */ comments in site.js, preserving offsets and strings.

    Every substring check in this module that reads site.js source was satisfied by
    a marker left behind in a comment while the code that used it was gone. (Checks
    that read page HTML instead are unaffected -- HTML has no // comments.)
    Measured: commenting out the breadcrumb aria-label assignment left
    audit_locale_nav_contract green, because both the assignment substring and the
    label strings survive inside `// crumb.setAttribute(...)`.

    String-aware on purpose -- site.js line 4 holds "http://www.w3.org/2000/svg",
    which a naive `//` strip would truncate. Comment bodies are replaced with
    spaces rather than removed so that reported offsets stay comparable.
    """
    out = list(js_text)
    i, n = 0, len(js_text)
    quote = None
    while i < n:
        c = js_text[i]
        if quote:
            if c == "\\":
                i += 2
                continue
            if c == quote:
                quote = None
            i += 1
            continue
        if c in "\"'`":
            quote = c
            i += 1
            continue
        if c == "/" and _starts_regex_literal(js_text, i):
            i += 1
            while i < n and js_text[i] != "/":
                if js_text[i] == "\\":
                    i += 2
                    continue
                if js_text[i] == "\n":
                    break
                i += 1
            i += 1
            continue
        if c == "/" and i + 1 < n and js_text[i + 1] == "/":
            while i < n and js_text[i] != "\n":
                out[i] = " "
                i += 1
            continue
        if c == "/" and i + 1 < n and js_text[i + 1] == "*":
            while i < n and not (js_text[i] == "*" and i + 1 < n and js_text[i + 1] == "/"):
                if js_text[i] != "\n":
                    out[i] = " "
                i += 1
            for j in range(i, min(i + 2, n)):
                out[j] = " "
            i += 2
            continue
        i += 1
    return "".join(out)


def audit_shell_wiring(
    css: str,
    js: str,
    home_text: str,
    representatives: dict[str, str],
) -> list[str]:
    """Structural contract for sitewide shell CSS/JS wiring."""
    missing: list[str] = []
    if "site-home" not in _body_class(home_text):
        missing.append("home-missing-site-home-class")
    if "body.site-home" not in css:
        missing.append("css-missing-body-site-home")
    hook_count = strip_js_comments(js).count(HOOK)
    if hook_count != 1:
        missing.append(f"hook-count-{hook_count}-expected-1")
    for marker in SHELL_MARKERS:
        if marker not in css:
            missing.append(f"css-missing-{marker}")
    for key in ("hub", "chapter", "generated"):
        if not _loads_shared_asset_links(representatives[key]):
            missing.append(f"{key}-missing-shared-assets")
    return missing


def _playground_ok(text: str) -> bool:
    classes = _body_class(text)
    return (
        'href="assets/site.css"' in text
        and 'src="assets/site.js"' not in text
        and "playground-page" in classes
        and "site-refreshed" in classes
        and all(f'id="{i}"' in text for i in PLAYGROUND_IDS)
    )


SKIP_LINK_EN = 'class="skip-link" href="#main">Skip to main content</a>'
SKIP_LINK_JA = 'class="skip-link" href="#main">メインコンテンツへスキップ</a>'
# Matched as a pattern, not an exact literal: the landmark carries tabindex="-1"
# so the skip link can actually move focus into it, and an exact-literal check
# would report the focusable landmark as a *missing* one.
MAIN_LANDMARK_RE = re.compile(r'<main\b[^>]*\bid="main"[^>]*>')
# A skip link that does not move focus is inert: the target must be programmatically
# focusable. Measured on 5273c1b0 -- both playground pages activated the link,
# set location.hash, and left document.activeElement on <body>, because
# <main id="main"> carried no tabindex. That record also listed a click path, which
# later re-measurement showed was a tool artifact, not evidence: the skip link is
# position:fixed at y=-48.7 while unfocused, so elementFromPoint returns <body> and a
# synthesized click never lands. Only the keyboard path established the defect.
MAIN_FOCUSABLE_RE = re.compile(
    r'<main\b(?=[^>]*\bid="main")(?=[^>]*\btabindex="-1")[^>]*>'
)


def audit_playground_skip_contract(texts: dict[str, str]) -> list[str]:
    """Playground static skip link + main landmark; localized per locale."""
    missing: list[str] = []
    en = texts.get("playground.en.html") or texts.get("playground.en", "")
    ja = texts.get("playground.ja.html") or texts.get("playground.ja", "")
    if SKIP_LINK_EN not in en:
        missing.append("playground.en-missing-english-skip-link")
    if SKIP_LINK_JA not in ja:
        missing.append("playground.ja-missing-japanese-skip-link")
    if SKIP_LINK_JA in en:
        missing.append("playground.en-has-japanese-skip-link")
    for lang, text in (("en", en), ("ja", ja)):
        if not MAIN_LANDMARK_RE.search(text):
            missing.append(f"playground.{lang}-missing-main-id-main")
        elif not MAIN_FOCUSABLE_RE.search(text):
            missing.append(f"playground.{lang}-main-not-focusable-tabindex")
        if text.count('class="skip-link"') != 1:
            missing.append(f"playground.{lang}-skip-link-count-not-1")
        body_open = text.find("<body")
        skip_at = text.find('class="skip-link"')
        if body_open >= 0 and skip_at >= 0 and skip_at > text.find(">", body_open) + 200:
            missing.append(f"playground.{lang}-skip-link-not-near-body-start")
    return missing


# The two playground pages carry a *static* skip link, but site.js injects one into
# every other page that has a <main> -- 48 of them on dd00fed5. The focusable-landmark
# requirement above therefore has to hold on the injection path too, and MAIN_FOCUSABLE_RE
# cannot see it: the attribute is set at runtime, so no static page text contains it.
#
# Measured on dd00fed5 with the injection path NOT setting it (headless Chrome, viewport
# 360x800, keyboard Tab then Enter, 2 runs per page): index.{en,ja}.html, cli.en.html,
# guide.ja.html and examples.en.html all set location.hash to "#main" and left
# document.activeElement on <body>. With it set, document.activeElement === <main> on all
# five, 2 runs each. The URL moved either way -- only focus distinguishes the two states.
#
# Matched inside initSkipLink's own body rather than anywhere in the file, so the shortest
# way to satisfy this is to actually set the attribute on the injection path.
# Receiver-qualified on purpose. Bare 'setAttribute("tabindex", "-1")' is satisfied by
# link.setAttribute(...) too, which would leave the landmark unfocusable while the check
# stayed green. The id assignment is here for the same reason the tabindex one is: on the
# 48 injected pages there is no static id="main" (measured: index.en.html and cli.en.html
# both contain zero <main ... id="main">), so the injected href="#main" has no target at
# all without it.
SKIP_JS_FOCUSABLE_MARKERS = (
    'main.hasAttribute("tabindex")',
    'main.setAttribute("tabindex", "-1")',
    'main.id = "main"',
)
INIT_SKIP_LINK_RE = re.compile(
    r"function initSkipLink\(\)\s*\{(.*?)\n  \}", re.S
)


def audit_skip_injection_sets_tabindex(js_text: str) -> list[str]:
    """site.js's skip-link injection path must assign tabindex to the landmark.

    Source-structure check on site.js, not a focus-behavior check: nothing here
    renders a page. That the assignment actually moves focus was measured in a
    browser (see the commit that introduced it), and no CI lane observes it.
    """
    missing: list[str] = []
    js_text = strip_js_comments(js_text)
    match = INIT_SKIP_LINK_RE.search(js_text)
    if not match:
        return ["skip-js-missing-initSkipLink-body"]
    body = match.group(1)
    for marker in SKIP_JS_FOCUSABLE_MARKERS:
        if marker not in body:
            missing.append(f"skip-js-missing-{marker}")
    # The attribute has to be set before the early return that fires when a static skip
    # link already exists, or a page shipping its own link keeps an inert landmark.
    early_return = body.find('if ($(".skip-link")) return;')
    tabindex_at = body.find('main.setAttribute("tabindex", "-1")')
    if early_return >= 0 and tabindex_at >= 0 and tabindex_at > early_return:
        missing.append("skip-js-tabindex-set-after-early-return")
    return missing


# The whole assignment expression, not the label strings on their own: "Breadcrumb"
# also appears in the comment above the assignment in site.js, so a bare label search
# stayed green with the en label deleted. Comment shadowing is handled for every
# js-source check by strip_js_comments() above -- matching the whole expression is the
# second, independent guard.
BREADCRUMB_ASSIGNMENT = (
    'crumb.setAttribute("aria-label", lang === "ja" ? "パンくずリスト" : "Breadcrumb")'
)


def audit_locale_nav_contract(js_text: str) -> list[str]:
    """Shared nav chrome that initNav() renders: the locale toggle's aria-current,
    and the breadcrumb's accessible name.

    Both are runtime-only. The static hosts ship
    `<nav class="breadcrumb" data-nav>` with no aria-label, so if initNav() stops
    setting one, nothing in the static pages reveals it.
    docs/DESIGN-docs-site.md requires `<nav aria-label="Breadcrumb">`.
    """
    missing: list[str] = []
    js_text = strip_js_comments(js_text)
    if 'aria-current="true"' not in js_text:
        missing.append("locale-nav-missing-aria-current-true")
    if js_text.count('aria-current="true"') < 2:
        missing.append("locale-nav-missing-bilingual-aria-current-patterns")
    for marker in (
        'class="active" aria-current="true">日本語</a>',
        'class="active" aria-current="true">English</a>',
    ):
        if marker not in js_text:
            missing.append(f"locale-nav-missing-{marker[:24]}")
    # Receiver-qualified: a bare aria-label search matches six other assignments in
    # site.js, so it would stay green with the breadcrumb's own label deleted.
    if 'crumb.setAttribute("aria-label"' not in js_text:
        missing.append("breadcrumb-missing-aria-label-assignment")
    if BREADCRUMB_ASSIGNMENT not in js_text:
        missing.append("breadcrumb-missing-localized-labels")
    return missing


def _page_paths() -> dict[str, dict[str, Path]]:
    return {
        stem: {
            lang: INTRO / f"{stem}.{lang}.html"
            for lang in ("en", "ja")
        }
        for stem in SITE_STEMS
    }


def python_cli_authority_offenders(text: str) -> list[str]:
    return [label for pattern, label in PY_CLI_AUTHORITY_PATTERNS if pattern.search(text)]


def audit_backbone_contract(js_text: str) -> list[str]:
    """Structural contract for the shared correctness backbone in site.js."""
    missing: list[str] = []
    js_text = strip_js_comments(js_text)
    for marker in BACKBONE_JS_MARKERS:
        if marker not in js_text:
            missing.append(f"backbone-missing-{marker}")
    for stage in BACKBONE_STAGE_IDS:
        if f'id: "{stage}"' not in js_text:
            missing.append(f"backbone-missing-stage-{stage}")
    for field in ("intent", "contract", "evidence", "limitation", "next"):
        if f"{field}:" not in js_text:
            missing.append(f"backbone-missing-field-{field}")
    return missing


HOME_BACKBONE_MARKERS = (
    'id="home-backbone"',
    "initHomeBackbone();",
    "data-home-repair",
    "data-home-outcome",
)
HOME_BACKBONE_FORBIDDEN = (
    "confidence-meter",
    "meter-fill",
    "chain-ascent",
    "chain-peak",
    "Higher confidence",
)
HOME_STAGE_LABELS = ("Business", "Requirements", "Design", "Implementation")


def audit_home_backbone_contract(home_text: str, js_text: str) -> list[str]:
    """Homepage must expose the four-stage backbone without a fake confidence meter."""
    missing: list[str] = []
    js_text = strip_js_comments(js_text)
    for marker in HOME_BACKBONE_MARKERS:
        if marker not in home_text and marker not in js_text:
            missing.append(f"home-backbone-missing-{marker}")
    for forbidden in HOME_BACKBONE_FORBIDDEN:
        if forbidden in home_text:
            missing.append(f"home-backbone-forbidden-{forbidden}")
    if "78%" in home_text:
        missing.append("home-backbone-forbidden-78-percent")
    for label in HOME_STAGE_LABELS:
        if label not in home_text and label not in js_text:
            missing.append(f"home-backbone-missing-label-{label}")
    if 'role="listitem"' in home_text and "chain-node" in home_text:
        missing.append("home-backbone-legacy-five-node-chain")
    return missing


def _hub_journey_keys(js_text: str) -> set[str] | None:
    """Top-level keys of site.js's HUB_JOURNEYS object, or None if it is absent."""
    match = re.search(r"const HUB_JOURNEYS = \{(.*?)\n  \};", js_text, re.S)
    if not match:
        return None
    return set(re.findall(r'^\s{4}"?([a-z][a-z0-9-]*)"?:\s*\{', match.group(1), re.M))


def audit_journey_contract(js_text: str, texts: dict[str, dict[str, str]]) -> list[str]:
    """Bounded structural contract for primary journeys (not prose quality)."""
    missing: list[str] = []
    js_text = strip_js_comments(js_text)
    for step in HUB_JOURNEY_STEPS:
        if f'data-journey="{step}"' not in js_text:
            missing.append(f"hub-template-missing-{step}")
    # Keys of the HUB_JOURNEYS object, not merely the stem appearing somewhere in
    # site.js: every hub stem is also a CATEGORIES id, so a substring search stays
    # green when the journey entry itself is deleted. initHub() early-returns
    # without a journey, which renders that hub blank -- measured on 73744f72 by
    # deleting HUB_JOURNEYS.guides (27 lines): the whole freshness lane still
    # returned exit 0 / 32 passed before this was keyed off the object.
    journey_keys = _hub_journey_keys(js_text)
    if journey_keys is None:
        missing.append("hub-journeys-object-not-found")
    else:
        for hub in HUB_STEMS:
            if hub not in journey_keys:
                missing.append(f"hub-journey-missing-{hub}")
    for marker in REFERENCE_HUB_MARKERS:
        if marker not in js_text:
            missing.append(f"reference-hub-missing-{marker}")
    for lang in ("en", "ja"):
        for marker in EXAMPLES_JOURNEY_MARKERS:
            if marker not in texts["examples"][lang]:
                missing.append(f"examples.{lang}-missing-{marker}")
        for marker in GUIDE_JOURNEY_MARKERS:
            if marker not in texts["guide"][lang]:
                missing.append(f"guide.{lang}-missing-{marker}")
    return missing


def audit_site_content(pages: dict[str, dict[str, Path]] | None = None) -> dict[str, object]:
    pages = pages or _page_paths()
    assert len(pages) == 25
    texts = {
        stem: {lang: path.read_text(encoding="utf-8") for lang, path in langs.items()}
        for stem, langs in pages.items()
    }
    offenders: dict[str, list[str]] = {}
    for stem, by_lang in texts.items():
        for lang, text in by_lang.items():
            hits = python_cli_authority_offenders(text)
            if hits:
                offenders[f"{stem}.{lang}"] = hits
    missing_generated_cli = [
        lang
        for lang in ("en", "ja")
        if "cli-contract.json" not in texts["cli"][lang]
    ]
    bilingual_ok = all(
        (INTRO / f"{stem}.en.html").is_file() and (INTRO / f"{stem}.ja.html").is_file()
        for stem in SITE_STEMS
    )
    lang_attr_ok = all(
        f'<html lang="{lang}">' in texts[stem][lang]
        for stem in SITE_STEMS
        for lang in ("en", "ja")
    )
    shared_nav_ok = all(
        'data-nav' in texts[stem][lang] and 'href="assets/site.css"' in texts[stem][lang]
        for stem in SITE_STEMS
        if stem != "playground"
    )
    js_text = JS.read_text(encoding="utf-8")
    return {
        "page_count": len(texts) * 2,
        "python_authority_offenders": offenders,
        "missing_generated_cli_markers": missing_generated_cli,
        "journey_contract_missing": audit_journey_contract(js_text, texts),
        "backbone_contract_missing": audit_backbone_contract(js_text),
        "bilingual_ok": bilingual_ok,
        "lang_attr_ok": lang_attr_ok,
        "shared_nav_ok": shared_nav_ok,
        "texts": texts,
    }


def _scoped_declarations(css: str) -> list[tuple[str, str]]:
    css = re.sub(r"/\*.*?\*/", " ", css, flags=re.S)
    out: list[tuple[str, str]] = []
    stack: list[str] = []
    buf = ""
    for ch in css:
        if ch == "{":
            stack.append(" ".join(buf.split()))
            buf = ""
        elif ch == "}":
            for decl in buf.split(";"):
                if decl.strip() and stack:
                    out.append((stack[-1], decl.strip()))
            buf = ""
            if stack:
                stack.pop()
        elif ch == ";":
            if stack:
                out.append((stack[-1], buf.strip()))
            buf = ""
        else:
            buf += ch
    return out


def palette_scopes(css: str) -> dict[str, set[str]]:
    found: dict[str, set[str]] = {}
    for selector, decl in _scoped_declarations(css):
        name = decl.split(":", 1)[0].strip()
        if name in PALETTE_PROPS:
            found.setdefault(name, set()).add(selector)
    return found


def _specificity(selector: str) -> tuple[int, int, int]:
    ids = len(re.findall(r"#[\w-]+", selector))
    classes = len(re.findall(r"\.[\w-]+", selector)) + len(re.findall(r"\[[^\]]+\]", selector))
    classes += len(re.findall(r"::?(?!not\b)[a-z-]+", selector))
    types = len(re.findall(r"(?:^|[\s>+~])([a-z][\w-]*)", selector))
    return (ids, classes, types)


def gutter_clobbers(css: str) -> list[str]:
    decls = _scoped_declarations(css)
    gutter_at = None
    for i, (selector, decl) in enumerate(decls):
        if selector == "body.docs-page section" and decl.startswith("padding-left"):
            gutter_at = i
            break
    if gutter_at is None:
        return ["body.docs-page section no longer reserves the sidebar gutter"]
    reserved = _specificity("body.docs-page section")
    bad = []
    for selector, decl in decls:
        positive = re.sub(r":not\([^)]*\)", "", selector)
        if "site-home" in positive:
            continue
        targets_section = selector.endswith("section") or ".hero" in selector
        if not targets_section or decl.split(":", 1)[0].strip() != "padding":
            continue
        if _specificity(selector) >= reserved:
            bad.append(f"{selector} {{ {decl} }}")
    return bad


def _shell_wiring_inputs() -> tuple[str, str, str, dict[str, str]]:
    css = CSS.read_text(encoding="utf-8")
    js = JS.read_text(encoding="utf-8")
    home_text = REPRESENTATIVES["home"].read_text(encoding="utf-8")
    representatives = {
        key: REPRESENTATIVES[key].read_text(encoding="utf-8")
        for key in ("hub", "chapter", "generated")
    }
    return css, js, home_text, representatives


def test_site_refresh_shell_wiring():
    css, js, home_text, representatives = _shell_wiring_inputs()
    assert audit_shell_wiring(css, js, home_text, representatives) == []


def test_site_refresh_playground_isolation():
    css = CSS.read_text(encoding="utf-8")
    pages = [p.read_text(encoding="utf-8") for p in PLAYGROUND]
    assert PG_CSS in css
    assert all(_playground_ok(text) for text in pages)
    assert not any('src="assets/site.js"' in text for text in pages)


def test_site_refresh_playground_skip_link_contract():
    texts = {
        p.name: p.read_text(encoding="utf-8")
        for p in PLAYGROUND
    }
    assert audit_playground_skip_contract(texts) == []


def test_site_js_skip_injection_sets_tabindex_on_the_landmark():
    assert audit_skip_injection_sets_tabindex(JS.read_text(encoding="utf-8")) == []


def test_site_js_skip_injection_rejects_the_missing_tabindex_mutant():
    js = JS.read_text(encoding="utf-8")
    mutant = js.replace(
        '    if (!main.hasAttribute("tabindex")) main.setAttribute("tabindex", "-1");\n', "", 1
    )
    assert mutant != js, "anchor for the focusability assignment not found in site.js"
    offenders = audit_skip_injection_sets_tabindex(mutant)
    assert 'skip-js-missing-main.hasAttribute("tabindex")' in offenders
    assert 'skip-js-missing-main.setAttribute("tabindex", "-1")' in offenders
    assert audit_skip_injection_sets_tabindex(js) == []


def test_site_js_skip_injection_rejects_the_missing_landmark_id_mutant():
    """Without main.id the injected href="#main" has no target on the 48 injected pages."""
    js = JS.read_text(encoding="utf-8")
    mutant = js.replace('    if (!main.id) main.id = "main";\n', "", 1)
    assert mutant != js, "anchor for the landmark id assignment not found in site.js"
    assert 'skip-js-missing-main.id = "main"' in audit_skip_injection_sets_tabindex(mutant)
    assert audit_skip_injection_sets_tabindex(js) == []


def test_site_js_skip_injection_rejects_the_late_tabindex_mutant():
    """Setting the attribute after the early return leaves static-link pages inert."""
    js = JS.read_text(encoding="utf-8")
    assignment = '    if (!main.hasAttribute("tabindex")) main.setAttribute("tabindex", "-1");\n'
    early = '    if ($(".skip-link")) return;\n'
    mutant = js.replace(assignment, "", 1).replace(early, early + assignment, 1)
    assert mutant != js, "anchors for the reordering mutant not found in site.js"
    assert "skip-js-tabindex-set-after-early-return" in audit_skip_injection_sets_tabindex(mutant)
    assert audit_skip_injection_sets_tabindex(js) == []


def test_site_refresh_playground_skip_rejects_english_label_mutant():
    texts = {
        p.name: p.read_text(encoding="utf-8")
        for p in PLAYGROUND
    }
    mutant = dict(texts)
    mutant["playground.en.html"] = texts["playground.en.html"].replace(SKIP_LINK_EN, SKIP_LINK_JA, 1)
    assert "playground.en-has-japanese-skip-link" in audit_playground_skip_contract(mutant)
    assert audit_playground_skip_contract(texts) == []


def test_site_refresh_playground_skip_rejects_unfocusable_main_mutant():
    texts = {p.name: p.read_text(encoding="utf-8") for p in PLAYGROUND}
    mutant = dict(texts)
    mutant["playground.en.html"] = re.sub(
        r'(<main\b[^>]*\bid="main"[^>]*?)\s+tabindex="-1"',
        r"\1",
        texts["playground.en.html"],
        count=1,
    )
    assert mutant["playground.en.html"] != texts["playground.en.html"]
    assert "playground.en-main-not-focusable-tabindex" in audit_playground_skip_contract(mutant)
    assert "playground.en-missing-main-id-main" not in audit_playground_skip_contract(mutant)
    assert audit_playground_skip_contract(texts) == []


def test_strip_js_comments_preserves_code_it_must_not_touch():
    """The comment scanner is hand-written and six checks depend on it.

    Cited mutation for the regex arm: drop _starts_regex_literal's regex handling and
    the first case below mangles real code into `const re = /\\/\\`.

    site.js has no regex containing // or /* today, so this guards a latent defect:
    mangled code would make a check report a marker missing that is actually present.
    """
    keep_cases = (
        (r'const re = /\/\//g; const keep = 1;', "const keep = 1;"),
        (r'const re = /\/\*/g; const keep = 2;', "const keep = 2;"),
        ("const x = a / b; // gone", "const x = a / b;"),
        ("const p = h.scrollTop / (h.scrollHeight - 1); // gone", "h.scrollHeight - 1)"),
        ('const u = "http://x.y/z"; // gone', '"http://x.y/z"'),
        ("const t = `a//b ${x} c`; // gone", "`a//b ${x} c`"),
        ('const s = "*/"; const keep = 3;', "const keep = 3;"),
    )
    for src, must_survive in keep_cases:
        out = strip_js_comments(src)
        assert must_survive in out, f"stripper mangled code: {src!r} -> {out!r}"
        assert "gone" not in out, f"stripper left a comment body: {src!r} -> {out!r}"

    js = JS.read_text(encoding="utf-8")
    stripped = strip_js_comments(js)
    assert len(stripped) == len(js), "stripper must preserve offsets"
    assert 'http://www.w3.org/2000/svg' in stripped, "string content must survive"
    for marker in ('main.setAttribute("tabindex", "-1")', "initBackbone();", HOOK):
        assert marker in stripped, f"stripper removed real code: {marker}"


def test_js_source_checks_reject_commented_out_code():
    """Comment shadowing, as a class.

    Population: every audit in this module whose verdict depends on site.js source
    alone -- audit_shell_wiring (hook count), audit_skip_injection_sets_tabindex,
    audit_locale_nav_contract, audit_backbone_contract, audit_journey_contract. All
    five are covered below. audit_home_backbone_contract is deliberately excluded:
    it accepts each marker from home_text *or* js_text, so commenting out the js
    copy alone cannot change its verdict.

    Cited mutation: comment out the implementation line rather than deleting it, so
    every marker substring survives. Before strip_js_comments() this left
    audit_locale_nav_contract green with the breadcrumb assignment commented out.
    """
    js = JS.read_text(encoding="utf-8")

    def comment_out(needle):
        lines = js.splitlines(True)
        for i, line in enumerate(lines):
            if needle in line and not line.strip().startswith("//"):
                indent = line[: len(line) - len(line.lstrip())]
                lines[i] = f"{indent}// {line.strip()}\n"
                return "".join(lines)
        raise AssertionError(f"anchor not found in site.js: {needle}")

    css = CSS.read_text(encoding="utf-8")
    home = REPRESENTATIVES["home"].read_text(encoding="utf-8")
    reps = {k: v.read_text(encoding="utf-8") for k, v in REPRESENTATIVES.items()}
    texts = {
        stem: {lang: (INTRO / f"{stem}.{lang}.html").read_text(encoding="utf-8")
               for lang in ("en", "ja")}
        for stem in ("examples", "guide")
    }
    for needle, audit, prefix in (
        (HOOK, lambda j: audit_shell_wiring(css, j, home, reps), "hook-count-"),
        ('main.setAttribute("tabindex", "-1")', audit_skip_injection_sets_tabindex, "skip-js-"),
        ('crumb.setAttribute("aria-label"', audit_locale_nav_contract, "breadcrumb-"),
        ("initBackbone();", audit_backbone_contract, "backbone-missing-"),
        ('data-journey="audience"', lambda j: audit_journey_contract(j, texts), "hub-template-"),
    ):
        mutant = comment_out(needle)
        assert mutant != js, needle
        offenders = [o for o in audit(mutant) if o.startswith(prefix)]
        assert offenders, f"commented-out {needle} not detected; produced {audit(mutant)}"
        assert not [o for o in audit(js) if o.startswith(prefix)], f"baseline dirty for {needle}"


def test_site_refresh_locale_nav_aria_current_contract():
    js = JS.read_text(encoding="utf-8")
    assert audit_locale_nav_contract(js) == []


def test_site_refresh_locale_nav_rejects_missing_aria_current_mutant():
    js = JS.read_text(encoding="utf-8")
    mutant = js.replace('class="active" aria-current="true"', 'class="active"', 2)
    assert audit_locale_nav_contract(mutant) != []
    assert audit_locale_nav_contract(js) == []


def test_site_refresh_home_backbone_contract():
    home = REPRESENTATIVES["home"].read_text(encoding="utf-8")
    js = JS.read_text(encoding="utf-8")
    assert audit_home_backbone_contract(home, js) == []


def test_site_refresh_home_backbone_rejects_legacy_meter_mutant():
    home = REPRESENTATIVES["home"].read_text(encoding="utf-8")
    js = JS.read_text(encoding="utf-8")
    mutant = home.replace(
        '<div id="home-backbone" class="home-backbone reveal"></div>',
        '<div class="confidence-meter"><span class="meter-fill"></span></div>',
        1,
    )
    assert audit_home_backbone_contract(mutant, js) == [
        'home-backbone-missing-id="home-backbone"',
        "home-backbone-forbidden-confidence-meter",
        "home-backbone-forbidden-meter-fill",
    ]
    assert audit_home_backbone_contract(home, js) == []

    report = audit_site_content()
    assert report["page_count"] == 50
    assert report["bilingual_ok"] is True
    assert report["lang_attr_ok"] is True
    assert report["shared_nav_ok"] is True
    assert report["python_authority_offenders"] == {}
    assert report["missing_generated_cli_markers"] == []
    assert report["journey_contract_missing"] == []
    assert report["backbone_contract_missing"] == []


def test_site_refresh_contract_calibrates_incomplete_hook():
    css, js, home_text, representatives = _shell_wiring_inputs()
    mutant_js = js.replace(HOOK, "")
    assert audit_shell_wiring(css, mutant_js, home_text, representatives) == [
        "hook-count-0-expected-1"
    ]
    assert audit_shell_wiring(css, js, home_text, representatives) == []


def test_site_refresh_contract_rejects_playground_site_js_mutant():
    pages = [p.read_text(encoding="utf-8") for p in PLAYGROUND]
    pages[0] = pages[0].replace(
        "</body>",
        '<script src="assets/site.js"></script></body>',
        1,
    )
    assert any('src="assets/site.js"' in text for text in pages)
    assert not all(_playground_ok(text) for text in pages)


def test_site_refresh_contract_rejects_missing_backbone_mutant():
    js = JS.read_text(encoding="utf-8").replace("initBackbone();", "")
    assert audit_backbone_contract(js) == ["backbone-missing-initBackbone();"]
    assert audit_backbone_contract(JS.read_text(encoding="utf-8")) == []


def test_site_refresh_contract_rejects_missing_journey_mutant():
    text = (INTRO / "examples.en.html").read_text(encoding="utf-8")
    mutant = text.replace('data-journey="choose-goal"', 'data-journey="choose-goal-x"', 1)
    texts = audit_site_content()["texts"]
    texts_mut = {stem: dict(lang_text) for stem, lang_text in texts.items()}
    texts_mut["examples"]["en"] = mutant
    missing = audit_journey_contract(JS.read_text(encoding="utf-8"), texts_mut)
    assert missing == ['examples.en-missing-data-journey="choose-goal"']
    assert audit_journey_contract(JS.read_text(encoding="utf-8"), texts) == []


def test_site_refresh_contract_rejects_legacy_python_cli_authority_mutant():
    text = (INTRO / "cli.en.html").read_text(encoding="utf-8")
    mutant = text.replace(
        "Source: rust/fslc/cli-contract.json",
        "Source: src/fslc/cli.py",
        1,
    )
    assert python_cli_authority_offenders(mutant) == [
        "generator banner cites frozen Python as source"
    ]
    assert python_cli_authority_offenders(text) == []


def test_palette_is_defined_once_at_the_root():
    scopes = palette_scopes(CSS.read_text(encoding="utf-8"))
    assert scopes, "no palette tokens found"
    offenders = {n: sorted(s) for n, s in scopes.items() if s - {":root"}}
    assert not offenders, f"palette tokens scoped to a page: {offenders}"


def test_palette_single_source_rejects_page_scoped_mutant():
    mutant = CSS.read_text(encoding="utf-8") + "\nbody.site-home { --lp-amber: #e8a54b; }\n"
    scopes = palette_scopes(mutant)
    assert scopes["--lp-amber"] == {":root", "body.site-home"}
    assert any(s - {":root"} for s in scopes.values())


def test_docs_sidebar_gutter_survives_the_refresh_layer():
    assert gutter_clobbers(CSS.read_text(encoding="utf-8")) == []


def test_sidebar_gutter_detector_rejects_the_shorthand_mutant():
    mutant = CSS.read_text(encoding="utf-8") + (
        "\nbody.site-refreshed.docs-page:not(.site-home) section { padding: 72px 20px; }\n"
    )
    assert gutter_clobbers(mutant) == [
        "body.site-refreshed.docs-page:not(.site-home) section { padding: 72px 20px }"
    ]
