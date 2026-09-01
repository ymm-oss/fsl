# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Static integrity checks for the bounded bilingual correctness-chain route.

These hand-authored manual pages are intentionally separate from the generated
reference snapshot test. The check parses committed bytes only and does not
fetch remote URLs or claim browser or assistive-technology behavior.
"""
from __future__ import annotations

from collections import Counter
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "docs" / "intro"
PINNED_PREFIX = "https://github.com/ymm-oss/fsl/blob/f565aeb9c46daa28a927ecad79bfdb72e44b6bb7/"
BLOB_PATHS = (
    "examples/e2e/1_business.fsl", "examples/e2e/2_requirements.fsl",
    "examples/e2e/3_design.fsl", "examples/e2e/3_refines_2.fsl",
    "examples/e2e/impl/expense.py", "examples/e2e/impl/test_conformance.py",
)
HOME_TARGET = {lang: f"examples.{lang}.html#correctness-chain" for lang in ("en", "ja")}
EXPECTED_CHAIN_HREFS = {
    lang: [f"business-layer.{lang}.html", f"requirements-layer.{lang}.html", f"design-layer.{lang}.html",
           *(PINNED_PREFIX + path for path in BLOB_PATHS),
           f"language.{lang}.html#12-the-bridge-to-implementation",
           f"language.{lang}.html#12-the-bridge-to-implementation"]
    for lang in ("en", "ja")
}


class ManualPageParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.all_hrefs, self.ids, self.section_stack = [], [], []
        self.chain_section_count, self.chain_attrs = 0, []
        self.chain_heading_ids, self.chain_hrefs = [], []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "href" in attrs:
            self.all_hrefs.append(attrs["href"])
        if "id" in attrs:
            self.ids.append(attrs["id"])
        if tag == "section":
            in_chain = any(self.section_stack)
            if attrs.get("id") == "correctness-chain":
                self.chain_section_count += 1
                self.chain_attrs.append(attrs)
                in_chain = True
            self.section_stack.append(in_chain)
        if self.section_stack and self.section_stack[-1]:
            if tag == "a" and "href" in attrs:
                self.chain_hrefs.append(attrs["href"])
            if tag == "h2" and "id" in attrs:
                self.chain_heading_ids.append(attrs["id"])

    def handle_endtag(self, tag):
        if tag == "section":
            assert self.section_stack, "unexpected closing section"
            self.section_stack.pop()


def _parse(path):
    parser = ManualPageParser()
    parser.feed(path.read_text(encoding="utf-8"))
    assert not parser.section_stack, f"unclosed section in {path.name}"
    return parser


def _resolve_local(page, href):
    file_name, marker, fragment = href.partition("#")
    assert "://" not in href and not href.startswith("/"), f"nonlocal href: {href}"
    target = page.parent / file_name
    assert target.is_file(), f"missing local target: {href}"
    if marker:
        assert fragment in _parse(target).ids, f"missing local fragment: {href}"


def test_manual_correctness_chain_integrity():
    for lang in ("en", "ja"):
        home_path, gallery_path = ROOT / f"index.{lang}.html", ROOT / f"examples.{lang}.html"
        home, gallery = _parse(home_path), _parse(gallery_path)
        assert home.all_hrefs.count(HOME_TARGET[lang]) == 1, f"missing {lang} home href"
        _resolve_local(home_path, HOME_TARGET[lang])
        assert gallery.chain_section_count == 1, f"expected one {lang} correctness-chain section"
        assert gallery.chain_attrs[0].get("aria-labelledby") == "correctness-chain-title", f"missing {lang} labelled section"
        assert gallery.chain_heading_ids.count("correctness-chain-title") == 1, f"missing {lang} chain heading"
        for href in gallery.chain_hrefs:
            if href.startswith("https://github.com/ymm-oss/fsl/blob/"):
                assert href.startswith(PINNED_PREFIX), f"disallowed pinned URL: {href}"
                assert href.removeprefix(PINNED_PREFIX) in BLOB_PATHS, f"disallowed pinned URL: {href}"
            else:
                _resolve_local(gallery_path, href)
        assert Counter(gallery.chain_hrefs) == Counter(EXPECTED_CHAIN_HREFS[lang]), f"unexpected {lang} chain hrefs"
