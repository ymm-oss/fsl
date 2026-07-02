# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Tests for the builtin `Bool` action parameter type (#68).

`normalize_params` in `fslc.model` used to only resolve user-declared types
(`types_meta`), so `Bool` — already a first-class scalar type for state
(`is_scalar_type`, `domain_range`) — was rejected as a parameter type. This
exercises the fix end to end: parsing, BMC enumeration/verification, the
bare-boolean idiom in `requires` (`b` / `not b`), assignment into
`Map<_, Bool>` state, trace display, and concrete-Monitor replay agreement.

`Int` stays rejected (unbounded, cannot be enumerated) but with an improved
hint pointing at range parameters.
"""
from __future__ import annotations

from fslc.cli import run_check, run_verify
from fslc.model import build_spec
from fslc.parser import parse_src
from fslc.runtime import Monitor


def _spec_from(src):
    ast, display = parse_src(src, ".")
    return build_spec(ast, display)


def _write(tmp_path, src, name="spec.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


# `setFlag`'s guard uses the Bool param bare (`b`) and negated (`not b`) —
# exactly the idiom that must be a first-class z3 Bool at BMC-eval time, not
# an Int carrier, or `requires` blows up combining it with `and`/`or`/`z3.And`.
UNSAFE_SRC = """\
spec BoolParamUnsafe {
  type Id = 0..0

  state {
    flag: Map<Id, Bool>
  }

  init {
    forall i: Id {
      flag[i] = false
    }
  }

  action setFlag(i: Id, b: Bool) {
    requires b or not flag[i]
    flag[i] = b
  }

  invariant NeverSet {
    forall i: Id { not flag[i] }
  }
}
"""

# Same shape, but the guard only ever allows `b = false`, so the invariant
# can never be violated: a true-negative check that verify doesn't cry wolf
# just because a Bool param is in play.
SAFE_SRC = """\
spec BoolParamSafe {
  type Id = 0..0

  state {
    flag: Map<Id, Bool>
  }

  init {
    forall i: Id {
      flag[i] = false
    }
  }

  action setFlag(i: Id, b: Bool) {
    requires not b
    flag[i] = b
  }

  invariant NeverSet {
    forall i: Id { not flag[i] }
  }
}
"""

INT_PARAM_SRC = """\
spec IntParam {
  action noop(x: Int) {
    requires true
  }
}
"""


def test_bool_param_action_passes_check(tmp_path):
    path = _write(tmp_path, UNSAFE_SRC)
    result = run_check(str(path))
    assert result["result"] == "ok", result


def test_bool_guarded_invariant_is_violated_with_bool_param_in_counterexample(tmp_path):
    path = _write(tmp_path, UNSAFE_SRC)
    result = run_verify(str(path), 3, "warn")

    assert result["result"] == "violated", result
    assert result["invariant"] == "NeverSet"

    trace = result["trace"]
    action_steps = [e for e in trace if "action" in e]
    assert action_steps, trace
    last = action_steps[-1]["action"]
    assert last["name"] == "setFlag"
    assert last["params"] == {"i": 0, "b": True}


def test_bool_guarded_invariant_holds_when_bool_param_is_constrained_false(tmp_path):
    path = _write(tmp_path, SAFE_SRC)
    result = run_verify(str(path), 3, "warn")

    assert result["result"] == "verified", result


def test_bool_param_map_assignment_and_trace_replay_agree(tmp_path):
    """The BMC witness trace for the violation above, replayed step by step
    through the concrete Monitor, must reach the same state BMC reports and —
    on the final, invariant-violating step — the concrete Monitor must
    independently flag the *same* invariant with the *same* Bool param value.
    This is the dual-evaluator agreement invariant this repo pins for every
    feature (see tests/test_evaluator_agreement.py and tests/oracle.py, which
    exists specifically to catch a symbolic-only false negative)."""
    path = _write(tmp_path, UNSAFE_SRC)
    result = run_verify(str(path), 3, "warn")
    assert result["result"] == "violated", result

    action_steps = [e for e in result["trace"] if "action" in e]
    assert action_steps

    mon = Monitor(str(path))
    mon.reset()
    for entry in action_steps[:-1]:
        step_result = mon.step(entry["action"]["name"], entry["action"]["params"])
        assert step_result["ok"], step_result
        assert mon.state == entry["state"]

    last = action_steps[-1]
    step_result = mon.step(last["action"]["name"], last["action"]["params"])
    assert step_result["ok"] is False
    assert step_result["kind"] == "invariant"
    assert step_result["name"] == result["invariant"]
    assert step_result["params"] == last["action"]["params"]


def test_int_param_still_errors_with_range_parameter_hint(tmp_path):
    path = _write(tmp_path, INT_PARAM_SRC)
    result = run_check(str(path))

    assert result["result"] == "error"
    assert result["kind"] == "type"
    assert "unknown parameter type 'Int'" in result["message"]
    assert "range parameter" in result["hint"]


def test_bool_param_normalized_to_zero_one_range():
    spec = _spec_from(UNSAFE_SRC)
    act = next(a for a in spec["actions"] if a["name"] == "setFlag")
    b_param = next(p for p in act["params"] if p[0] == "b")
    assert b_param == ("b", 0, 1, "Bool")
