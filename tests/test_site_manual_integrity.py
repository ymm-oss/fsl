# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Static integrity checks for the bounded bilingual correctness-chain route.

These hand-authored manual pages are intentionally separate from the generated
reference snapshot test. The check parses static markup and inspects pinned objects in checked-out
Git history without fetching remote URLs; it does not claim browser or assistive-technology behavior.
"""
from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
import subprocess

import pytest

ROOT = Path(__file__).resolve().parent.parent / "docs" / "intro"
REPOSITORY_ROOT = ROOT.parent.parent
PINNED_COMMIT = "f565aeb9c46daa28a927ecad79bfdb72e44b6bb7"
PINNED_PREFIX = f"https://github.com/ymm-oss/fsl/blob/{PINNED_COMMIT}/"
BLOB_PATHS = (
    "examples/e2e/1_business.fsl", "examples/e2e/2_requirements.fsl",
    "examples/e2e/3_design.fsl", "examples/e2e/3_refines_2.fsl",
    "examples/e2e/impl/expense.py", "examples/e2e/impl/test_conformance.py",
)
HOME_TARGET = {lang: f"examples.{lang}.html#correctness-chain" for lang in ("en", "ja")}
EXPECTED_CHAIN_HREFS = {
    lang: [
        f"business-layer.{lang}.html", PINNED_PREFIX + BLOB_PATHS[0],
        f"requirements-layer.{lang}.html", PINNED_PREFIX + BLOB_PATHS[1],
        f"design-layer.{lang}.html", *(PINNED_PREFIX + path for path in BLOB_PATHS[2:]),
        f"language.{lang}.html#12-the-bridge-to-implementation",
        f"language.{lang}.html#12-the-bridge-to-implementation",
    ]
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


def _assert_unique_chain_ids(lang, page):
    for identifier in ("correctness-chain", "correctness-chain-title"):
        count = page.ids.count(identifier)
        assert count == 1, f"expected one {lang} {identifier} id, got {count}"


def _assert_chain_hrefs(lang, hrefs):
    assert hrefs == EXPECTED_CHAIN_HREFS[lang], f"unexpected {lang} chain href order"


def _assert_pinned_blobs(paths=BLOB_PATHS):
    for path in paths:
        result = subprocess.run(
            ["git", "cat-file", "-t", f"{PINNED_COMMIT}:{path}"], cwd=REPOSITORY_ROOT,
            capture_output=True, check=False, text=True,
        )
        assert result.returncode == 0 and result.stdout.strip() == "blob", (
            f"pinned object is not a blob: {PINNED_COMMIT}:{path}; "
            f"produced type={result.stdout.strip() or '<absent>'}, exit={result.returncode}"
        )


def test_manual_correctness_chain_integrity():
    for lang in ("en", "ja"):
        home_path, gallery_path = ROOT / f"index.{lang}.html", ROOT / f"examples.{lang}.html"
        home, gallery = _parse(home_path), _parse(gallery_path)
        assert home.all_hrefs.count(HOME_TARGET[lang]) == 1, f"missing {lang} home href"
        _resolve_local(home_path, HOME_TARGET[lang])
        _assert_unique_chain_ids(lang, gallery)
        assert gallery.chain_section_count == 1, f"expected one {lang} correctness-chain section"
        assert gallery.chain_attrs[0].get("aria-labelledby") == "correctness-chain-title", f"missing {lang} labelled section"
        assert gallery.chain_heading_ids.count("correctness-chain-title") == 1, f"missing {lang} chain heading"
        for href in gallery.chain_hrefs:
            if href.startswith("https://github.com/ymm-oss/fsl/blob/"):
                assert href.startswith(PINNED_PREFIX), f"disallowed pinned URL: {href}"
                assert href.removeprefix(PINNED_PREFIX) in BLOB_PATHS, f"disallowed pinned URL: {href}"
            else:
                _resolve_local(gallery_path, href)
        _assert_chain_hrefs(lang, gallery.chain_hrefs)
    _assert_pinned_blobs()


def test_manual_integrity_rejects_missing_local_fragment(tmp_path):
    target = tmp_path / "target.html"
    target.write_text("<html><body></body></html>", encoding="utf-8")
    with pytest.raises(AssertionError, match="missing local fragment: target.html#missing"):
        _resolve_local(tmp_path / "home.html", "target.html#missing")


def test_manual_integrity_rejects_reordered_chain_links():
    hrefs = list(EXPECTED_CHAIN_HREFS["en"])
    hrefs[0], hrefs[1] = hrefs[1], hrefs[0]
    with pytest.raises(AssertionError, match="unexpected en chain href order"):
        _assert_chain_hrefs("en", hrefs)


def test_manual_integrity_rejects_duplicate_chain_id():
    for identifier in ("correctness-chain", "correctness-chain-title"):
        page = ManualPageParser()
        page.feed(f'<section id="correctness-chain"><h2 id="correctness-chain-title"></h2></section><div id="{identifier}"></div>')
        with pytest.raises(AssertionError, match=f"expected one en {identifier} id, got 2"):
            _assert_unique_chain_ids("en", page)


def test_manual_integrity_rejects_missing_or_nonblob_pin():
    with pytest.raises(AssertionError, match="pinned object is not a blob"):
        _assert_pinned_blobs(("examples/e2e/missing.fsl",))
    with pytest.raises(AssertionError, match="pinned object is not a blob"):
        _assert_pinned_blobs(("examples/e2e",))
