# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Production JSONL log replay through refinement mapping syntax (issue #174)."""

import json
import subprocess
import sys

from fslc import build_spec, parse
from fslc.cli import exit_code, run_replay
from fslc.log_replay import build_log_mapping
from fslc.parser import parse_refinement
from fslc.refine import build_refinement


COUNTER_SPEC = """
spec Counter {
  type Count = 0..2
  state { count: Count }
  init { count = 0 }
  action add(delta: Count) {
    requires delta == 1
    count = count + delta
  }
  invariant WithinBound { count <= 2 }
}
"""

PROD_SPEC = """
spec ProdCounter {
  type Count = 0..2
  state { raw_count: Count }
  init { raw_count = 0 }
  action increment(amount: Count) {
    raw_count = raw_count + amount
  }
}
"""

MAPPING = """
refinement ProdCounterToCounter {
  impl ProdCounter
  abs Counter
  map count = raw_count
  action increment(amount) -> add(amount)
}
"""


def _write_fixture(tmp_path, records):
    spec_path = tmp_path / "counter.fsl"
    mapping_path = tmp_path / "log_mapping.fsl"
    log_path = tmp_path / "events.jsonl"
    spec_path.write_text(COUNTER_SPEC, encoding="utf-8")
    mapping_path.write_text(MAPPING, encoding="utf-8")
    log_path.write_text(
        "".join(json.dumps(record) + "\n" for record in records),
        encoding="utf-8",
    )
    return spec_path, log_path, mapping_path


def test_log_replay_maps_jsonl_action_and_state_to_monitor(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 1}},
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 2}},
        ],
    )

    result = run_replay(
        str(spec_path),
        from_log=str(log_path),
        mapping_path=str(mapping_path),
    )

    assert result["result"] == "conformant", result
    assert result["source"] == "jsonl_mapping"
    assert result["steps_checked"] == 2
    assert result["final_state"] == {"count": 2}
    assert result["mapping"] == "ProdCounterToCounter"
    assert exit_code(result) == 0


def test_log_replay_reports_first_state_mismatch_with_record_and_line(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 0}},
            {"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 2}},
        ],
    )

    result = run_replay(
        str(spec_path),
        from_log=str(log_path),
        mapping_path=str(mapping_path),
    )

    assert result["result"] == "nonconformant"
    assert result["failed_at_event"] == 0
    assert result["failed_at_record"] == 0
    assert result["log_line"] == 1
    assert result["violation"]["kind"] == "state_mismatch"
    assert result["violation"]["action"] == "add"
    assert result["violation"]["expected_state"] == {"count": 1}
    assert result["violation"]["observed_state"] == {"count": 0}
    assert result["violation"]["mismatches"] == [
        {"path": "count", "expected": 1, "observed": 0}
    ]
    assert exit_code(result) == 1


def test_log_replay_reports_action_rejection_at_source_record(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [
            {"action": "increment", "params": {"amount": 2}, "state": {"raw_count": 2}},
        ],
    )

    result = run_replay(
        str(spec_path),
        from_log=str(log_path),
        mapping_path=str(mapping_path),
    )

    assert result["result"] == "nonconformant"
    assert result["failed_at_record"] == 0
    assert result["log_line"] == 1
    assert result["violation"]["kind"] == "requires_failed"
    assert result["violation"]["source_action"] == "increment"
    assert result["violation"]["mapped_action"] == "add"


def test_log_mapping_uses_same_parser_and_mapping_expressions_as_refine():
    target = build_spec(parse(COUNTER_SPEC))
    impl = build_spec(parse(PROD_SPEC))
    tree = parse_refinement(MAPPING)

    log_mapping = build_log_mapping(tree, target)
    refinement = build_refinement(tree, impl, target)

    assert log_mapping["maps"]["count"]["expr"] == refinement["maps"]["count"]["expr"]
    assert (
        log_mapping["actions"]["increment"]["arg_exprs"]
        == refinement["actions"]["increment"]["arg_exprs"]
    )
    assert (
        log_mapping["actions"]["increment"]["abs_action"]
        == refinement["actions"]["increment"]["abs_action"]
    )


def test_log_replay_requires_full_observed_state(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [{"action": "increment", "params": {"amount": 1}, "state": {}}],
    )

    result = run_replay(
        str(spec_path),
        from_log=str(log_path),
        mapping_path=str(mapping_path),
    )

    assert result["result"] == "nonconformant"
    assert result["failed_at_record"] == 0
    assert result["violation"]["kind"] == "log_mapping"
    assert "raw_count" in result["violation"]["message"]


def test_log_replay_rejects_invalid_jsonl_with_line_number(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(tmp_path, [])
    log_path.write_text('{"action":"increment"}\nnot-json\n', encoding="utf-8")

    result = run_replay(
        str(spec_path),
        from_log=str(log_path),
        mapping_path=str(mapping_path),
    )

    assert result["result"] == "error"
    assert result["kind"] == "io"
    assert "line 2" in result["message"]


def test_log_replay_supports_indexed_maps_and_enum_values(tmp_path):
    spec = """
spec Tickets {
  type TicketId = 0..1
  enum Status { Open, Closed }
  state { status: Map<TicketId, Status> }
  init { forall t: TicketId { status[t] = Open } }
  action close(ticket: TicketId) {
    requires status[ticket] == Open
    status[ticket] = Closed
  }
}
"""
    mapping = """
refinement ServiceLogToTickets {
  impl ServiceLog
  abs Tickets
  map status[t: TicketId] = statuses[t]
  action ticket_closed(id) -> close(id)
}
"""
    spec_path = tmp_path / "tickets.fsl"
    mapping_path = tmp_path / "mapping.fsl"
    log_path = tmp_path / "events.jsonl"
    spec_path.write_text(spec, encoding="utf-8")
    mapping_path.write_text(mapping, encoding="utf-8")
    log_path.write_text(
        json.dumps({
            "action": "ticket_closed",
            "params": {"id": 1},
            "state": {"statuses": {"0": "Open", "1": "Closed"}},
        }) + "\n",
        encoding="utf-8",
    )

    result = run_replay(
        str(spec_path), from_log=str(log_path), mapping_path=str(mapping_path)
    )

    assert result["result"] == "conformant", result
    assert result["final_state"] == {"status": {"0": "Open", "1": "Closed"}}


def test_log_replay_supports_json_object_field_access(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [{
            "action": "increment",
            "params": {"amount": 1},
            "state": {"snapshot": {"raw_count": 1}},
        }],
    )
    mapping_path.write_text(
        MAPPING.replace("map count = raw_count", "map count = snapshot.raw_count"),
        encoding="utf-8",
    )

    result = run_replay(
        str(spec_path), from_log=str(log_path), mapping_path=str(mapping_path)
    )

    assert result["result"] == "conformant", result


def test_replay_cli_accepts_from_log_and_mapping(tmp_path):
    spec_path, log_path, mapping_path = _write_fixture(
        tmp_path,
        [{"action": "increment", "params": {"amount": 1}, "state": {"raw_count": 1}}],
    )

    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "replay",
            str(spec_path),
            "--from-log",
            str(log_path),
            "--mapping",
            str(mapping_path),
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    assert proc.returncode == 0, proc.stderr
    assert json.loads(proc.stdout)["result"] == "conformant"
