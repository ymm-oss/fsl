# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-db MVP dialect coverage."""

from pathlib import Path

from fslc.cli import run_check, run_db_check, run_db_import, run_db_observe, run_verify


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "db"


def _example(name):
    return str(EXAMPLES / name)


def _finding_kinds(out):
    return {finding["kind"] for finding in out["findings"]}


def _assumption_ids(out):
    return {assumption["id"] for assumption in out["assumptions"]}


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


def test_db_destructive_drop_requires_annotation():
    out = run_db_check(_example("unsafe_destructive_drop_without_annotation.fsl"))

    assert out["result"] == "violated"
    finding = out["findings"][0]
    assert finding["kind"] == "destructive_migration_unannotated"
    assert finding["failed_rule"] == "destructive_operations_annotated"
    assert finding["schema_element"] == "users.legacy_name"
    assert {candidate["kind"] for candidate in finding["repair_candidates"]} == {
        "annotation_change",
        "compat_shim",
    }


def test_db_destructive_annotation_does_not_weaken_compatibility():
    safe = run_db_check(_example("safe_destructive_drop_with_annotation.fsl"))
    assert safe["result"] == "verified_under_assumptions"

    unsafe = run_db_check(_example("unsafe_annotated_drop_with_old_server.fsl"))
    assert unsafe["result"] == "violated"
    assert "column_removed_while_still_read" in _finding_kinds(unsafe)
    assert "destructive_migration_unannotated" not in _finding_kinds(unsafe)


def test_db_preservation_and_rollback_rules_report_bounded_findings():
    safe_rename = run_db_check(_example("safe_rename_preservation.fsl"))
    assert safe_rename["result"] == "verified_under_assumptions"
    assert "DB-ASSUME-BOUNDED-ROW-MODEL" in _assumption_ids(safe_rename)

    lossy_split = run_db_check(_example("unsafe_lossy_split_preservation.fsl"))
    assert lossy_split["result"] == "violated"
    assert lossy_split["findings"][0]["kind"] == "data_preservation_loss"
    assert lossy_split["findings"][0]["failed_rule"] == "data_preserved"
    assert "DB-ASSUME-BOUNDED-ROW-MODEL" in _assumption_ids(lossy_split)

    lossy_merge = run_db_check(_example("unsafe_lossy_merge_preservation.fsl"))
    assert lossy_merge["result"] == "violated"
    assert lossy_merge["findings"][0]["kind"] == "data_preservation_loss"
    assert lossy_merge["findings"][0]["failed_rule"] == "data_preserved"

    unannotated_split = run_db_check(_example("unsafe_split_without_annotation.fsl"))
    assert unannotated_split["result"] == "violated"
    assert unannotated_split["findings"][0]["kind"] == "preservation_transform_unannotated"

    safe_rollback = run_db_check(_example("safe_rollback_equivalence.fsl"))
    assert safe_rollback["result"] == "verified_under_assumptions"
    assert "DB-ASSUME-BOUNDED-ROW-MODEL" in _assumption_ids(safe_rollback)

    rollback_drop = run_db_check(_example("unsafe_rollback_drop.fsl"))
    assert rollback_drop["result"] == "violated"
    assert rollback_drop["findings"][0]["kind"] == "rollback_not_equivalent"
    assert rollback_drop["findings"][0]["failed_rule"] == "rollback_equivalent"


def test_db_api_and_offline_compatibility_are_environment_scoped():
    safe = run_db_check(_example("safe_api_offline_compat.fsl"))
    assert safe["result"] == "verified_under_assumptions"
    assert "DB-ASSUME-OFFLINE-TTL-FINITE" in _assumption_ids(safe)

    missing_response = run_db_check(_example("unsafe_api_response_field_removed.fsl"))
    assert missing_response["result"] == "violated"
    assert missing_response["findings"][0]["kind"] == "api_response_field_missing"
    assert missing_response["findings"][0]["result"] == "violated"

    rejected_offline = run_db_check(_example("unsafe_offline_payload_rejected.fsl"))
    assert rejected_offline["result"] == "violated"
    finding = rejected_offline["findings"][0]
    assert finding["kind"] == "offline_payload_not_accepted"
    assert finding["witness"]["ttl_ticks"] == 2
    assert "DB-ASSUME-OFFLINE-TTL-FINITE" in _assumption_ids(rejected_offline)


def test_db_runtime_observation_reports_observed_mismatch_not_formal_violation():
    out = run_db_observe(
        _example("runtime_observation_target.fsl"),
        _example("runtime_observation_mismatch.json"),
    )

    assert out["result"] == "observed_mismatch"
    assert out["formal_result"] == "not_run"
    assert out["observation_schema_version"] == "fsl-db-observation.v0"
    assert _finding_kinds(out) == {
        "declared_unused_but_observed",
        "legacy_api_still_called",
    }
    assert {finding["result"] for finding in out["findings"]} == {"observed_mismatch"}
    assert "DB-ASSUME-OBSERVABILITY-COVERAGE" in _assumption_ids(out)


def test_db_sql_importer_emits_checkable_dbsystem(tmp_path):
    output = tmp_path / "imported.fsl"
    imported = run_db_import(
        _example("minimal_import.sql"),
        name="ImportedFromSql",
        output=str(output),
    )

    assert imported["result"] == "imported"
    assert imported["output"] == str(output)
    checked = run_db_check(str(output))
    assert checked["result"] == "verified_under_assumptions"


def test_db_sql_importer_reports_unsupported_constructs():
    imported = run_db_import(_example("unsupported_import.sql"), name="ImportedFromSql")

    assert imported["result"] == "imported_with_warnings"
    assert imported["warnings"][0]["kind"] == "unsupported_sql"
    assert "CREATE INDEX" in imported["warnings"][0]["statement"]
