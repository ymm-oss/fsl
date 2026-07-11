# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Bounded underspecification findings for analyze --profile ai-review (#179)."""
from __future__ import annotations

import json
from pathlib import Path

from fslc.cli import run_analyze


SNAPSHOT = Path(__file__).parent / "snapshots" / "analysis_underspecification.json"
SCHEMA = (
    Path(__file__).resolve().parents[1]
    / "schemas"
    / "fslc"
    / "analysis"
    / "analysis-findings.v0.schema.json"
)


def _write(tmp_path, source):
    path = tmp_path / "case.fsl"
    path.write_text(source, encoding="utf-8")
    return path


def _findings(out, kind):
    return [item for item in out["findings"] if item["finding_type"] == kind]


DIVERGENT = """
requirements ReviewChoice {
  state { ready: Bool, approved: Bool }
  init { ready = true  approved = false }
  requirement REQ-1 "review can approve or reject" {
    action approve() {
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


def test_reachable_choice_with_different_acceptance_outcomes_is_reported(tmp_path):
    out = run_analyze(str(_write(tmp_path, DIVERGENT)), profile="ai-review")
    findings = _findings(out, "divergent_choice")

    assert len(findings) == 1
    finding = findings[0]
    assert finding["involved_nodes"] == ["acceptance:AC-1", "action:approve", "action:reject"]
    assert finding["formal_status"] == "not_a_violation"
    assert finding["evidence_basis"] == "bounded_bmc"
    assert finding["witness"]["bounded_evidence"] == {
        "available": True,
        "depth": 4,
        "reachable_at_step": 0,
    }
    assert finding["witness"]["differing_predicates"] == [
        {"kind": "acceptance", "name": "AC-1"}
    ]
    assert finding["spec_question"].endswith("?")
    assert all(repair["kind"] == "ask_spec_question" for repair in finding["candidate_repairs"])


def test_mutually_exclusive_actions_do_not_report_divergent_choice(tmp_path):
    determined = DIVERGENT.replace(
        "requires ready\n      ready = false\n      approved = false",
        "requires not ready\n      ready = true\n      approved = false",
    )
    out = run_analyze(str(_write(tmp_path, determined)), profile="ai-review")

    assert _findings(out, "divergent_choice") == []


def test_unconstrained_effect_is_bmc_backed_and_suppresses_unread_state(tmp_path):
    path = _write(tmp_path, """
spec FreeAudit {
  state { ready: Bool, audit: Int }
  init { ready = true  audit = 0 }
  action keep() {
    requires ready
    audit = 0
  }
  action bump() {
    requires ready
    audit = 1
  }
  invariant Ready { ready }
}
""")
    out = run_analyze(str(path), profile="ai-review")
    free = _findings(out, "unconstrained_effect")

    assert len(free) == 1
    assert free[0]["involved_nodes"] == ["state:audit", "action:bump", "action:keep"]
    assert free[0]["witness"]["divergent_state"] == ["audit"]
    assert free[0]["witness"]["bounded_evidence"]["available"] is True
    assert free[0]["spec_question"].endswith("?")
    assert not any(
        item["finding_type"] == "unread_state" and item["involved_nodes"] == ["state:audit"]
        for item in out["findings"]
    )


def test_single_deterministic_unread_writer_keeps_structural_finding(tmp_path):
    path = _write(tmp_path, """
spec DeterministicAudit {
  state { ready: Bool, audit: Int }
  init { ready = true  audit = 0 }
  action bump() {
    requires ready
    audit = audit + 1
  }
  invariant Ready { ready }
}
""")
    out = run_analyze(str(path), profile="ai-review")

    assert _findings(out, "unconstrained_effect") == []
    assert any(
        item["finding_type"] == "unread_state" and item["involved_nodes"] == ["state:audit"]
        for item in out["findings"]
    )


def test_underspecification_json_snapshot(tmp_path):
    out = run_analyze(str(_write(tmp_path, DIVERGENT)), profile="ai-review")
    finding = _findings(out, "divergent_choice")[0]
    snapshot = {
        key: finding[key]
        for key in (
            "finding_id",
            "analysis",
            "finding_type",
            "severity",
            "confidence",
            "formal_status",
            "involved_nodes",
            "evidence_basis",
            "spec_question",
            "candidate_repairs",
            "do_not_assume",
        )
    }
    snapshot["witness"] = {
        key: finding["witness"][key]
        for key in ("kind", "bounded_evidence", "actions", "differing_predicates")
    }

    assert snapshot == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_findings_schema_declares_question_and_evidence_basis():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    properties = schema["$defs"]["finding"]["properties"]

    assert properties["spec_question"] == {"type": "string", "pattern": "\\?$"}
    assert properties["evidence_basis"] == {"enum": ["structural", "bounded_bmc"]}
