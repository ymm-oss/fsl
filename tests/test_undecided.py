# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Intentional undecision metadata and presentation (issue #189)."""
from __future__ import annotations

import json
from pathlib import Path

from fslc.cli import run_analyze, run_html, run_ledger, run_verify
from fslc.model import build_spec
from fslc.parser import parse_src
from fslc.undecided import undecided_declarations


SNAPSHOT = Path(__file__).parent / "snapshots" / "undecided.json"


SOURCE = """
requirements ReviewChoice {
  state { ready: Bool, approved: Bool }
  init { ready = true  approved = false }
  requirement REQ-1 "review can approve or reject" {
    action approve() "REQ-1: undecided: approval policy awaits owner decision" {
      requires ready
      ready = false
      approved = true
    }
    action reject() {
      requires ready
      ready = false
      approved = false
    }
  }
  acceptance AC-1 "approval path ends approved" {
    approve()
    expect approved
  }
}
"""


def _write(tmp_path, source=SOURCE, name="undecided.fsl"):
    path = tmp_path / name
    path.write_text(source, encoding="utf-8")
    return path


def _divergent(out):
    return next(item for item in out["findings"] if item["finding_type"] == "divergent_choice")


def test_undecided_marker_acknowledges_but_does_not_suppress_finding(tmp_path):
    path = _write(tmp_path)

    finding = _divergent(run_analyze(str(path), profile="ai-review"))

    assert finding["acknowledged"] is True
    assert finding["acknowledged_by"] == [{
        "kind": "action",
        "name": "approve",
        "node_id": "action:approve",
        "text": "approval policy awaits owner decision",
        "requirements": ["REQ-1"],
        "loc": {"line": 6, "column": 5},
        "verification_semantics": "metadata_only",
    }]
    assert finding["witness"]["bounded_evidence"]["available"] is True


def test_same_finding_without_marker_is_unacknowledged(tmp_path):
    source = SOURCE.replace(
        ' "REQ-1: undecided: approval policy awaits owner decision"',
        "",
    )
    finding = _divergent(run_analyze(str(_write(tmp_path, source)), profile="ai-review"))

    assert finding["acknowledged"] is False
    assert finding["acknowledged_by"] == []


def test_ledger_and_html_render_undecided_list(tmp_path):
    path = _write(tmp_path)

    ledger = run_ledger(str(path), depth=2, write_file=False)
    html = run_html(str(path), depth=2, write_file=False)

    assert ledger["result"] == "generated"
    assert "## 未決定一覧" in ledger["content"]
    assert "approval policy awaits owner decision" in ledger["content"]
    assert "REQ-1" in ledger["content"]
    assert html["result"] == "generated"
    assert 'id="undecided"' in html["content"]
    assert "Undecided Items" in html["content"]
    assert "approval policy awaits owner decision" in html["content"]


def test_marker_is_metadata_only_for_verification(tmp_path):
    tagged = _write(tmp_path, name="tagged.fsl")
    plain = _write(
        tmp_path,
        SOURCE.replace(' "REQ-1: undecided: approval policy awaits owner decision"', ""),
        name="plain.fsl",
    )

    tagged_result = run_verify(str(tagged), 2, "ignore", use_cache=False)
    plain_result = run_verify(str(plain), 2, "ignore", use_cache=False)

    assert tagged_result["result"] == plain_result["result"] == "verified"
    assert tagged_result["invariants_checked"] == plain_result["invariants_checked"]
    assert tagged_result["action_coverage"] == plain_result["action_coverage"]


def test_undecided_snapshot(tmp_path):
    path = _write(tmp_path)
    source = path.read_text(encoding="utf-8")
    ast, display_names = parse_src(source, str(path.parent))
    declarations = undecided_declarations(build_spec(ast, display_names))
    finding = _divergent(run_analyze(str(path), profile="ai-review"))
    snapshot = {
        "declarations": declarations,
        "finding": {
            "finding_type": finding["finding_type"],
            "acknowledged": finding["acknowledged"],
            "acknowledged_by": finding["acknowledged_by"],
            "evidence_basis": finding["evidence_basis"],
        },
    }

    assert snapshot == json.loads(SNAPSHOT.read_text(encoding="utf-8"))
