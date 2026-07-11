# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Spec-digest-bound approval records (issue #190)."""
from __future__ import annotations

import json
from pathlib import Path

from fslc.cli import (
    _build_arg_parser,
    exit_code,
    run_approval_check,
    run_approval_create,
    run_diff_approval,
    run_ledger,
)


SNAPSHOT = Path(__file__).parent / "snapshots" / "approval_record.json"
SCHEMA = Path(__file__).resolve().parents[1] / "schemas" / "fslc" / "approval-record.v0.schema.json"


BASE = """
spec ApprovalLatch {
  state { flag: Bool }
  init { flag = false }
  action enable() "REQ-1: enable once" {
    requires not flag
    flag = true
  }
  invariant EnabledIsBool "REQ-1: flag remains boolean" { flag or not flag }
}
"""


def _write(tmp_path, name, content):
    path = tmp_path / name
    path.write_text(content, encoding="utf-8")
    return path


def _create(tmp_path):
    spec = _write(tmp_path, "approval.fsl", BASE)
    rendered = _write(tmp_path, "approval.html", "<h1>Approved behavior</h1>\n")
    record = tmp_path / "approval.json"
    out = run_approval_create(
        str(spec), str(rendered), "html", "Product Owner",
        approved_at="2026-07-11T12:00:00+09:00",
        command="fslc html approval.fsl -o approval.html",
        output=str(record),
    )
    assert out["result"] == "approval_created", out
    return spec, rendered, record, out


def test_approval_create_and_unchanged_check_are_approved(tmp_path):
    spec, rendered, record, created = _create(tmp_path)

    checked = run_approval_check(str(spec), str(record), str(rendered))

    assert checked["result"] == "approval_checked"
    assert checked["status"] == "approved"
    assert checked["spec_status"] == "approved"
    assert checked["rendering_status"] == "approved"
    assert checked["approved_digest"] == checked["current_digest"]
    assert checked["requirements"] == [{
        "id": "REQ-1",
        "status": "approved",
        "approved_digest": checked["approved_digest"],
    }]
    assert exit_code(created) == 0
    assert exit_code(checked) == 0


def test_spec_and_rendering_changes_are_drifted(tmp_path):
    spec, rendered, record, _created = _create(tmp_path)
    spec.write_text(BASE.replace("requires not flag\n", ""), encoding="utf-8")
    rendered.write_text("<h1>Changed rendering</h1>\n", encoding="utf-8")

    checked = run_approval_check(str(spec), str(record), str(rendered))

    assert checked["status"] == "drifted"
    assert checked["spec_status"] == "drifted"
    assert checked["rendering_status"] == "drifted"
    assert checked["approved_digest"] != checked["current_digest"]
    assert checked["semantic_diff"]["baseline_digest"] == checked["approved_digest"]
    assert exit_code(checked) == 1


def test_approval_record_is_direct_semantic_diff_input(tmp_path):
    spec, _rendered, record, _created = _create(tmp_path)
    spec.write_text(BASE.replace("requires not flag\n", ""), encoding="utf-8")

    diff = run_diff_approval(str(record), str(spec), depth=2)

    assert diff["result"] == "semantic_diff"
    assert "behavior_added" in diff["summary"]
    assert diff["approval"]["baseline_digest"]
    assert diff["approval"]["materialization"] == "embedded_source_snapshot"
    assert diff["old"]["file"].startswith("approval:")


def test_approval_record_snapshots_imported_source_group(tmp_path):
    _write(tmp_path, "child.fsl", """
spec Child {
  type X = 0..0
  state { flag: Bool }
  init { flag = false }
  action enable(x: X) { requires not flag  flag = true }
}
""")
    root = _write(tmp_path, "root.fsl", """
compose Root {
  use Child as child from "child.fsl"
  action enable(x: child.X) = child.enable(x) { }
  internal child.enable
}
""")
    rendered = _write(tmp_path, "root.md", "approved")
    record = tmp_path / "root.approval.json"

    out = run_approval_create(
        str(root), str(rendered), "ledger", "Owner",
        approved_at="2026-07-11T00:00:00Z", output=str(record),
    )

    assert out["result"] == "approval_created"
    assert sorted(out["record"]["baseline"]["files"]) == ["child.fsl", "root.fsl"]


def test_ledger_shows_per_requirement_approval_status(tmp_path):
    spec, _rendered, record, _created = _create(tmp_path)

    approved = run_ledger(str(spec), depth=2, write_file=False, approval=str(record))
    spec.write_text(BASE.replace("requires not flag\n", ""), encoding="utf-8")
    drifted = run_ledger(str(spec), depth=2, write_file=False, approval=str(record))

    assert "| 承認 |" in approved["content"]
    assert "approved" in approved["content"]
    assert "REQ-1" in approved["content"]
    assert "drifted" in drifted["content"]
    assert approved["approval"]["approved_digest"] == drifted["approval"]["approved_digest"]


def test_approval_record_snapshot(tmp_path):
    _spec, _rendered, _record, created = _create(tmp_path)
    snapshot = json.loads(json.dumps(created["record"]))
    digest = snapshot["spec"]["digest"]
    snapshot["spec"]["digest"] = "<digest>"
    snapshot["baseline"]["digest"] = "<digest>"
    snapshot["rendering"]["path"] = "approval.html"
    assert digest
    assert snapshot == json.loads(SNAPSHOT.read_text(encoding="utf-8"))


def test_approval_cli_contract():
    create = _build_arg_parser().parse_args([
        "approval", "create", "spec.fsl", "--rendered", "report.html",
        "--rendering-kind", "html", "--approver", "Owner",
    ])
    check = _build_arg_parser().parse_args([
        "approval", "check", "spec.fsl", "--record", "approval.json",
    ])
    diff = _build_arg_parser().parse_args([
        "diff", "--approval", "approval.json", "spec.fsl",
    ])

    assert create.approval_cmd == "create"
    assert check.approval_cmd == "check"
    assert diff.approval == "approval.json"
    assert diff.old == "spec.fsl" and diff.new is None


def test_approval_schema_and_tampered_baseline_fail_closed(tmp_path):
    spec, _rendered, record, created = _create(tmp_path)
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    assert set(schema["required"]) <= set(created["record"])
    raw = json.loads(record.read_text(encoding="utf-8"))
    raw["baseline"]["files"]["approval.fsl"] = raw["baseline"]["files"]["approval.fsl"].replace(
        "requires not flag", ""
    )
    record.write_text(json.dumps(raw), encoding="utf-8")

    out = run_diff_approval(str(record), str(spec), depth=2)

    assert out["result"] == "error"
    assert out["kind"] == "semantics"
    assert "does not match its approved digest" in out["message"]
