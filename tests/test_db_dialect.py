# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-db MVP dialect coverage."""

from pathlib import Path

from fslc.cli import run_check, run_db_check, run_verify


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "db"


def _example(name):
    return str(EXAMPLES / name)


def test_db_system_check_accepts_mvp_syntax():
    out = run_check(_example("safe_add_nullable_column.fsl"))

    assert out["result"] == "ok"
    assert out["spec"] == "SafeAddNullableColumn"


def test_db_safe_add_nullable_column_verifies_and_proves():
    path = _example("safe_add_nullable_column.fsl")

    checked = run_db_check(path)
    assert checked["result"] == "verified_under_assumptions"
    assert checked["finding_schema_version"] == "fsl-db-finding.v0"
    assert checked["findings"] == []

    verified = run_verify(path, 8, "warn")
    assert verified["result"] == "verified"

    proved = run_verify(path, 8, "warn", engine="induction")
    assert proved["result"] == "proved"


def test_db_safe_dual_write_backfill_switch_read_drop_old_proves():
    out = run_db_check(
        _example("safe_dual_write_backfill_switch_read_drop_old.fsl"),
        engine="induction",
    )

    assert out["result"] == "verified_under_assumptions"
    assert out["kernel"]["result"] == "proved"
    assert [a["id"] for a in out["assumptions"]] == [
        "DB-ASSUME-ROLLING-SNAPSHOT",
        "DB-ASSUME-CAPABILITY-DECLARATIONS",
    ]


def test_db_unsafe_not_null_before_backfill_returns_stable_finding():
    out = run_db_check(_example("unsafe_not_null_before_backfill.fsl"))

    assert out["result"] == "violated"
    finding = out["findings"][0]
    assert finding["kind"] == "not_null_before_backfill"
    assert finding["failed_rule"] == "not_null_after_backfill"
    assert finding["migration"] == "add_required_email"
    assert finding["schema_element"] == "users.email"
    assert finding["minimal_conflict_set"] == {
        "migration": "add_required_email",
        "schema_element": "users.email",
    }
    assert {candidate["weakens_spec"] for candidate in finding["repair_candidates"]} == {False, True}


def test_db_unsafe_drop_column_with_old_server_returns_read_finding():
    out = run_db_check(_example("unsafe_drop_column_with_old_server.fsl"))

    assert out["result"] == "violated"
    findings = [f for f in out["findings"] if f["schema_element"] == "users.legacy_name"]
    assert findings
    finding = findings[0]
    assert finding["kind"] == "column_removed_while_still_read"
    assert finding["environment"] == "prod"
    assert finding["artifact"] == "server_v1"
    assert finding["migration"] == "drop_legacy_name"
    assert finding["minimal_conflict_set"] == {
        "environment": "prod",
        "artifact": "server_v1",
        "migration": "drop_legacy_name",
        "schema_element": "users.legacy_name",
    }


def test_db_unsafe_drop_column_with_worker_returns_read_finding():
    out = run_db_check(_example("unsafe_drop_column_with_worker.fsl"))

    assert out["result"] == "violated"
    finding = next(f for f in out["findings"] if f["artifact"] == "worker_v1")
    assert finding["kind"] == "column_removed_while_still_read"
    assert finding["witness"]["environment_role"] == "may_exist"


def test_db_reference_errors_are_json_errors(tmp_path):
    path = tmp_path / "bad_ref.fsl"
    path.write_text(
        """dbsystem BadRef {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; }
  }
  migration bad from 0 to 1 { drop users.missing; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
}
""",
        encoding="utf-8",
    )

    out = run_check(str(path))

    assert out["result"] == "error"
    assert out["kind"] == "semantics"
    assert "unknown column 'users.missing'" in out["message"]
