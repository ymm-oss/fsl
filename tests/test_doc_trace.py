# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Canonical Markdown document trace checks (issue #192)."""
from __future__ import annotations

import json
from pathlib import Path

from fslc.cli import _build_arg_parser, run_check, run_verify


SNAPSHOT = Path(__file__).parent / "snapshots" / "doc_trace_warnings.json"
REQ1 = "flag can be enabled The flag moves from false to true."


def _write(tmp_path, name, content):
    path = tmp_path / name
    path.write_text(content, encoding="utf-8")
    return path


def _spec(source_tag="", tag_text=REQ1, extra=""):
    meta = f' "source: requirements.md"' if source_tag else ""
    return f"""
spec DocTrace{meta} {{
  state {{ flag: Bool }}
  init {{ flag = false }}
  action enable() "REQ-1: {tag_text}" {{
    requires not flag
    flag = true
  }}
  invariant BoolFlag "REQ-1: {tag_text}" {{ flag or not flag }}
  {extra}
}}
"""


def _docs(extra=""):
    # Tests below use explicit text where needed; this helper preserves the
    # canonical title+body normalization contract.
    return f"""# Product requirements

## REQ-1: flag can be enabled
The flag moves from false to true.

{extra}
"""


def _doc_warnings(out):
    return [
        item for item in out.get("warnings", [])
        if item.get("kind") in {"missing_formalization", "ghost_requirement", "stale_tag"}
    ]


def test_consistent_canonical_doc_has_no_trace_warnings(tmp_path):
    spec = _write(tmp_path, "spec.fsl", _spec())
    docs = _write(tmp_path, "requirements.md", _docs())

    checked = run_check(str(spec), docs=str(docs))
    verified = run_verify(str(spec), 2, "ignore", docs=str(docs), use_cache=False)

    assert checked["result"] == "ok"
    assert verified["result"] == "verified"
    assert _doc_warnings(checked) == []
    assert _doc_warnings(verified) == []


def test_bidirectional_id_and_freshness_warnings_include_evidence(tmp_path):
    spec = _write(
        tmp_path,
        "spec.fsl",
        _spec(
            tag_text="old flag wording",
            extra='invariant Ghost "REQ-3: removed requirement" { true }',
        ),
    )
    docs = _write(tmp_path, "requirements.md", _docs(extra="""
## REQ-2: audit is retained
Audit history remains available.
"""))

    out = run_check(str(spec), docs=str(docs))
    warnings = _doc_warnings(out)

    assert [item["kind"] for item in warnings] == [
        "missing_formalization", "ghost_requirement", "stale_tag", "stale_tag",
    ]
    stale = [item for item in warnings if item["kind"] == "stale_tag"]
    assert all(item["old_text"] == "old flag wording" for item in stale)
    assert all(item["new_text"] == REQ1 for item in stale)
    normalized = json.loads(json.dumps(warnings))
    for item in normalized:
        item["document"] = "requirements.md"
    assert normalized == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_source_tag_auto_discovers_doc_relative_to_spec(tmp_path):
    nested = tmp_path / "nested"
    nested.mkdir()
    spec = _write(nested, "spec.fsl", _spec(source_tag=True))
    _write(nested, "requirements.md", _docs())

    out = run_check(str(spec))

    assert out["result"] == "ok"
    assert _doc_warnings(out) == []


def test_doc_bytes_participate_in_verify_cache_key(tmp_path, monkeypatch):
    monkeypatch.setenv("FSLC_CACHE", "on")
    monkeypatch.setenv("FSLC_CACHE_DIR", str(tmp_path / "cache"))
    spec = _write(tmp_path, "spec.fsl", _spec())
    docs = _write(tmp_path, "requirements.md", _docs())

    first = run_verify(str(spec), 2, "ignore", docs=str(docs))
    second = run_verify(str(spec), 2, "ignore", docs=str(docs))
    docs.write_text(_docs().replace("false to true", "off to on"), encoding="utf-8")
    after_doc_edit = run_verify(str(spec), 2, "ignore", docs=str(docs))

    assert "cache" not in first
    assert second["cache"]["hit"] is True
    assert "cache" not in after_doc_edit
    assert any(item["kind"] == "stale_tag" for item in after_doc_edit["warnings"])


def test_default_without_docs_or_source_tag_is_unchanged(tmp_path):
    spec = _write(tmp_path, "spec.fsl", _spec())

    implicit = run_check(str(spec))
    explicit_none = run_check(str(spec), docs=None)

    assert implicit == explicit_none
    assert _doc_warnings(implicit) == []


def test_docs_cli_contract():
    check = _build_arg_parser().parse_args([
        "check", "spec.fsl", "--docs", "requirements.md",
    ])
    verify = _build_arg_parser().parse_args([
        "verify", "spec.fsl", "--docs", "requirements.md",
    ])

    assert check.docs == "requirements.md"
    assert verify.docs == "requirements.md"
