# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

import pytest

from fslc import FslError, build_spec, parse, verify
from fslc.runtime import Monitor


def _verify(src, depth=2):
    return verify(build_spec(parse(src)), depth, deadlock_mode="ignore")


def _monitor(src):
    return Monitor(build_spec(parse(src)))


def test_init_if_else_picks_then_branch_in_monitor_and_verifier():
    src = """
spec IfInit {
  type R = 0..3
  state { x: R }
  init { if true { x = 1 } else { x = 2 } }
  action bump() { requires true  x = x }
  invariant Chosen { x == 1 }
}
"""
    mon = _monitor(src)

    assert mon.reset()["x"] == 1
    assert _verify(src)["result"] == "verified"


def test_nested_init_if_composes_branch_conditions():
    src = """
spec NestedIfInit {
  type R = 0..3
  state { x: R }
  init {
    if true {
      if false { x = 1 } else { x = 2 }
    } else {
      x = 3
    }
  }
  action stay() { requires true  x = x }
  invariant Chosen { x == 2 }
}
"""
    mon = _monitor(src)

    assert mon.reset()["x"] == 2
    assert _verify(src)["result"] == "verified"


def test_init_if_inside_forall_range():
    src = """
spec ForallIfInit {
  type I = 0..2
  type R = 0..3
  state { m: Map<I, R> }
  init {
    forall i in 0..2 {
      if i == 1 { m[i] = 2 } else { m[i] = 1 }
    }
  }
  action stay() { requires true  m[0] = m[0] }
  invariant Chosen { m[0] == 1 and m[1] == 2 and m[2] == 1 }
}
"""
    mon = _monitor(src)

    assert mon.reset()["m"] == {"0": 1, "1": 2, "2": 1}
    assert _verify(src)["result"] == "verified"


def test_init_if_chosen_branch_controls_invariant_result():
    good = """
spec IfInitGood {
  type R = 0..3
  state { x: R }
  init { if false { x = 1 } else { x = 2 } }
  action stay() { requires true  x = x }
  invariant Chosen { x == 2 }
}
"""
    bad = good.replace("IfInitGood", "IfInitBad").replace("x == 2", "x == 1")

    assert _verify(good)["result"] == "verified"
    out = _verify(bad)
    assert out["result"] == "violated"
    assert out["violation_kind"] == "invariant"
    assert out["violated_at_step"] == 0


def test_init_if_non_bool_condition_deferred_to_verify():
    # Init expression types are not checked at model-build time (consistent with
    # every other init type error); a non-Bool condition is caught symbolically
    # at verify via require_bool.
    src = """
spec InitIfType {
  type R = 0..3
  state { x: R }
  init { if 0 { x = 1 } else { x = 2 } }
  action stay() { requires true  x = x }
}
"""
    spec = build_spec(parse(src))  # model check passes

    with pytest.raises(FslError) as exc:
        verify(spec, 2, deadlock_mode="ignore")

    assert exc.value.kind == "type"
    assert "if condition must be Bool" in str(exc.value)
