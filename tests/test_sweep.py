# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Scope sweep coverage (#107)."""

from fslc.cli import exit_code, run_sweep, run_verify


SWEEP_SRC = r'''requirements SweepCounterexample {
  entity Case

  state { selected: Map<Case, Bool> }

  init {
    forall c: Case { selected[c] = false }
  }

  action select(c: Case) {
    requires not selected[c]
    selected[c] = true
  }

  invariant AtMostOne {
    count(c: Case where selected[c]) <= 1
  }
}
verify {
  instances Case = 2
}
'''


def _write(tmp_path, src, name):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_sweep_reports_minimal_counterexample_without_changing_verify(tmp_path):
    spec = _write(tmp_path, SWEEP_SRC, "sweep_counterexample.fsl")

    unchanged = run_verify(str(spec), 1, "warn", property_name="AtMostOne")
    assert unchanged["result"] == "verified"
    assert "sweep" not in unchanged

    out = run_sweep(
        str(spec),
        "1..2",
        "warn",
        property_name="AtMostOne",
        instances=["Case=1..2"],
    )
    assert out["result"] == "sweep_failed"
    assert exit_code(out) == 1
    assert "results" in out["sweep"]
    assert "minimal_counterexample" in out["sweep"]

    minimal = out["sweep"]["minimal_counterexample"]
    assert minimal["scope"] == {
        "instances": {"Case": 2},
        "values": {},
        "depth": 2,
    }
    assert minimal["summary"]["result"] == "violated"
    assert minimal["summary"]["invariant"] == "AtMostOne"
    assert minimal["verification"]["trace"][-1]["state"]["selected"] == {
        "0": True,
        "1": True,
    }
