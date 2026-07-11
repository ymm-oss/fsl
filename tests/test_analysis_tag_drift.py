# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Declaration tag drift findings and review export (issue #188)."""
from __future__ import annotations

import json
from pathlib import Path

from fslc.cli import _build_arg_parser, run_analyze


SNAPSHOT = Path(__file__).parent / "snapshots" / "tag_review_export.json"


SOURCE = """
spec TagReview {
  type Count = 0..20
  const AUTO_LIMIT = 10
  state { renamed_count: Count }
  init { renamed_count = 0 }

  action increment() "REQ-1: updates `old_count`" {
    renamed_count = renamed_count + 1
  }

  invariant Disjoint "REQ-2: `AUTO_LIMIT` caps `renamed_count`" {
    renamed_count >= 0
  }

  invariant Aligned "REQ-3: `renamed_count` stays nonnegative" {
    renamed_count >= 0
  }
}
"""


def _write(tmp_path):
    path = tmp_path / "tag_review.fsl"
    path.write_text(SOURCE, encoding="utf-8")
    return path


def _findings(out, kind):
    return [item for item in out["findings"] if item["finding_type"] == kind]


def test_tag_drift_findings_are_local_deterministic_signals(tmp_path):
    path = _write(tmp_path)

    out = run_analyze(str(path), profile="ai-review")

    stale = _findings(out, "tag_stale_reference")
    disjoint = _findings(out, "tag_formula_disjoint")
    assert len(stale) == 1
    assert stale[0]["involved_nodes"] == ["action:increment"]
    assert stale[0]["witness"]["identifiers"] == ["old_count"]
    assert len(disjoint) == 1
    assert disjoint[0]["involved_nodes"] == ["invariant:Disjoint"]
    assert disjoint[0]["witness"]["identifiers"] == ["AUTO_LIMIT"]
    assert not any("invariant:Aligned" in item["involved_nodes"] for item in stale + disjoint)
    assert stale[0]["formal_status"] == "not_a_violation"
    assert disjoint[0]["formal_status"] == "not_a_violation"


def test_tag_review_export_snapshot(tmp_path):
    path = _write(tmp_path)

    out = run_analyze(str(path), export_kind="tag-review")

    assert out["result"] == "analyzed"
    assert out["analysis"] == "tag_review"
    assert out["schema_version"] == "tag-review.v0"
    assert [item["node_id"] for item in out["declarations"]] == [
        "action:increment", "invariant:Aligned", "invariant:Disjoint",
    ]
    assert out == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_tag_review_export_cli_contract():
    args = _build_arg_parser().parse_args([
        "analyze", "spec.fsl", "--export", "tag-review",
    ])

    assert args.export_kind == "tag-review"


def test_tag_review_export_rejects_profile_and_batch(tmp_path):
    path = _write(tmp_path)
    other = tmp_path / "other.fsl"
    other.write_text(SOURCE.replace("TagReview", "Other"), encoding="utf-8")

    combined = run_analyze(str(path), profile="ai-review", export_kind="tag-review")
    batch = run_analyze([str(path), str(other)], export_kind="tag-review")

    assert combined["result"] == "error"
    assert combined["kind"] == "semantics"
    assert batch["result"] == "error"
    assert batch["kind"] == "semantics"
