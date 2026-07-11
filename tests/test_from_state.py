# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Predictive BMC from a concrete logical-state snapshot (issue #175)."""

import json
import subprocess
import sys
from pathlib import Path

from fslc import build_spec, parse
from fslc.bmc import verify
from fslc.cli import exit_code, run_verify
from fslc.runtime import Monitor


PREDICT_SPEC = """
spec PredictiveCounter {
  type Count = 0..2
  state { count: Count }
  init { count = 0 }
  action increment() {
    requires count < 1
    count = count + 1
  }
  invariant Safe { count <= 1 }
}
"""

SHAPES_SPEC = """
spec SnapshotShapes {
  const CAP = 2
  type Id = 0..1
  enum Phase { Open, Closed }
  struct Item { phase: Phase, owner: Option<Id> }
  state {
    flag: Bool,
    count: Int,
    phase: Phase,
    selected: Option<Id>,
    item: Item,
    rows: Map<Id, Item>,
    seen: Set<Id>,
    queue: Seq<Id, CAP>,
    links: relation Id -> Id
  }
  init {
    flag = false
    count = 0
    phase = Open
    selected = none
    item = Item { phase: Open, owner: none }
    forall i: Id { rows[i] = Item { phase: Open, owner: none } }
    seen = Set {}
    queue = Seq {}
    links = Set {}
  }
  action populate() {
    flag = true
    count = 1
    phase = Closed
    selected = some(1)
    item = Item { phase: Closed, owner: some(1) }
    rows[1] = Item { phase: Closed, owner: some(1) }
    seen = seen.add(1)
    queue = queue.push(1)
    links = links.add(0, 1)
  }
  invariant CountBound { count <= 1 }
}
"""

ROOT = Path(__file__).resolve().parents[1]


def _write_spec(tmp_path, source=PREDICT_SPEC):
    path = tmp_path / "predictive.fsl"
    path.write_text(source, encoding="utf-8")
    return path


def _write_state(tmp_path, state, name="state.json"):
    path = tmp_path / name
    path.write_text(json.dumps(state), encoding="utf-8")
    return path


def test_from_state_finds_step_zero_violation_unreachable_from_spec_init(tmp_path):
    spec_path = _write_spec(tmp_path)
    snapshot_path = _write_state(tmp_path, {"count": 2})

    normal = run_verify(
        str(spec_path), 0, "ignore", use_cache=False
    )
    predicted = run_verify(
        str(spec_path), 0, "ignore", from_state=str(snapshot_path)
    )

    assert normal["result"] == "verified"
    assert predicted["result"] == "violated"
    assert predicted["violation_kind"] == "invariant"
    assert predicted["violated_at_step"] == 0
    assert predicted["trace"][0]["state"] == {"count": 2}
    assert exit_code(predicted) == 1


def test_from_state_can_verify_safe_snapshot_and_stamps_faithfulness(tmp_path):
    spec_path = _write_spec(tmp_path)
    snapshot_path = _write_state(tmp_path, {"count": 1})

    result = run_verify(
        str(spec_path), 1, "ignore", from_state=str(snapshot_path)
    )

    assert result["result"] == "verified"
    assert result["initial_state"] == {
        "source": "snapshot",
        "path": str(snapshot_path),
        "complete": True,
        "replaces_spec_init": True,
    }
    assert result["faithfulness"] == {
        "scope": "bounded_from_snapshot",
        "spec_init": "not_used",
        "induction": "not_applicable",
    }
    assert "cache" not in result


def test_verify_library_accepts_initial_snapshot_without_cli(tmp_path):
    del tmp_path
    spec = build_spec(parse(PREDICT_SPEC))

    result = verify(spec, 0, deadlock_mode="ignore", initial_snapshot={"count": 2})

    assert result["result"] == "violated"
    assert result["violated_at_step"] == 0


def test_monitor_logical_state_json_round_trips_into_from_state(tmp_path):
    spec_path = ROOT / "specs" / "cart_v1.fsl"
    monitor = Monitor(str(spec_path))
    monitor.reset()
    step = monitor.step("add_to_cart", {"u": 0, "i": 1})
    assert step["ok"] is True
    snapshot_path = _write_state(tmp_path, monitor.state)

    result = run_verify(
        str(spec_path), 0, "ignore", from_state=str(snapshot_path),
        exclude_property_names=["SoldOut"],
    )

    assert result["result"] == "verified"
    assert result["initial_state"]["source"] == "snapshot"


def test_all_monitor_state_shapes_round_trip_into_snapshot_constraints(tmp_path):
    spec_path = _write_spec(tmp_path, SHAPES_SPEC)
    monitor = Monitor(str(spec_path))
    monitor.reset()
    step = monitor.step("populate", {})
    assert step["ok"] is True
    snapshot_path = _write_state(tmp_path, monitor.state)

    result = run_verify(
        str(spec_path), 0, "ignore", from_state=str(snapshot_path)
    )

    assert result["result"] == "verified", result
    assert result["initial_state"]["complete"] is True


def test_composed_monitor_display_keys_are_accepted_by_library_snapshot():
    monitor = Monitor(str(ROOT / "specs" / "order_system.fsl"))
    snapshot = monitor.reset()
    excluded = [item["name"] for item in monitor.spec.get("reachables", [])]

    result = verify(
        monitor.spec,
        0,
        deadlock_mode="ignore",
        exclude_property_names=excluded,
        initial_snapshot=snapshot,
    )

    assert result["result"] == "verified", result


def test_from_state_requires_complete_exact_state_shape(tmp_path):
    spec_path = _write_spec(tmp_path)
    missing = _write_state(tmp_path, {}, "missing.json")
    extra = _write_state(tmp_path, {"count": 0, "other": 1}, "extra.json")

    missing_result = run_verify(str(spec_path), 0, "ignore", from_state=str(missing))
    extra_result = run_verify(str(spec_path), 0, "ignore", from_state=str(extra))

    assert missing_result["result"] == "error"
    assert missing_result["kind"] == "type"
    assert "missing state variable 'count'" in missing_result["message"]
    assert extra_result["result"] == "error"
    assert extra_result["kind"] == "type"
    assert "unknown state variable 'other'" in extra_result["message"]


def test_from_state_rejects_wrong_type_and_out_of_range_value(tmp_path):
    spec_path = _write_spec(tmp_path)
    wrong_type = _write_state(tmp_path, {"count": "1"}, "wrong.json")
    out_of_range = _write_state(tmp_path, {"count": 3}, "range.json")

    wrong_result = run_verify(str(spec_path), 0, "ignore", from_state=str(wrong_type))
    range_result = run_verify(str(spec_path), 0, "ignore", from_state=str(out_of_range))

    assert wrong_result["result"] == "error"
    assert wrong_result["kind"] == "type"
    assert "state.count" in wrong_result["message"]
    assert range_result["result"] == "error"
    assert range_result["kind"] == "type"
    assert "out of range [0..2]" in range_result["message"]


def test_from_state_is_bmc_only_and_rejects_induction(tmp_path):
    spec_path = _write_spec(tmp_path)
    snapshot_path = _write_state(tmp_path, {"count": 1})

    result = run_verify(
        str(spec_path), 1, "ignore", engine="induction", from_state=str(snapshot_path)
    )

    assert result["result"] == "error"
    assert result["kind"] == "semantics"
    assert "BMC" in result["message"]
    assert exit_code(result) == 2


def test_from_state_reports_invalid_json_as_io_error(tmp_path):
    spec_path = _write_spec(tmp_path)
    snapshot_path = tmp_path / "broken.json"
    snapshot_path.write_text('{"count":', encoding="utf-8")

    result = run_verify(
        str(spec_path), 0, "ignore", from_state=str(snapshot_path)
    )

    assert result["result"] == "error"
    assert result["kind"] == "io"
    assert "invalid state JSON" in result["message"]


def test_verify_cli_accepts_from_state(tmp_path):
    spec_path = _write_spec(tmp_path)
    snapshot_path = _write_state(tmp_path, {"count": 2})

    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "verify",
            str(spec_path),
            "--depth",
            "0",
            "--deadlock",
            "ignore",
            "--from-state",
            str(snapshot_path),
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    result = json.loads(proc.stdout)
    assert proc.returncode == 1, proc.stderr
    assert result["result"] == "violated"
    assert result["initial_state"]["replaces_spec_init"] is True
