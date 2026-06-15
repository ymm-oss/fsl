"""Transition invariant (`trans`) tests."""

import pytest

from fslc import FslError, build_spec, parse, prove, verify


def _spec(src):
    return build_spec(parse(src))


def test_trans_catches_sticky_property_violation():
    src = """
spec StickyBad {
  enum Status { A, B }
  state { status: Status }
  init { status = A }
  action stay() {
    status = A
  }
  action break_sticky() {
    status = B
  }
  trans Sticky { old(status) == A => status == A }
}
"""
    r = verify(_spec(src), 2)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "trans"
    assert r["trans"] == "Sticky"
    assert r["invariant"] == "Sticky"
    assert r["violated_at_step"] == 1
    assert r["last_action"]["name"] == "break_sticky"
    assert r["transitions_checked"] == ["Sticky"]


def test_trans_sticky_property_verified_and_proved_without_violating_action():
    src = """
spec StickyGood {
  enum Status { A, B }
  state { status: Status }
  init { status = A }
  action stay() {
    status = A
  }
  trans Sticky { old(status) == A => status == A }
}
"""
    spec = _spec(src)
    verified = verify(spec, 3)
    assert verified["result"] == "verified"
    assert verified["transitions_checked"] == ["Sticky"]

    proved = prove(spec, 1, 3)
    assert proved["result"] == "proved"
    assert proved["transitions_checked"] == ["Sticky"]


def test_old_allowed_inside_trans_and_broken_consequent_violates():
    src = """
spec OldInTrans {
  state { x: Int }
  init { x = 0 }
  action noop() {
    x = x
  }
  trans Broken { old(x) == 0 => x == 1 }
}
"""
    r = verify(_spec(src), 1)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "trans"
    assert r["violated_at_step"] == 1


def test_old_outside_ensures_or_trans_has_clear_error():
    src = """
spec OldInInvariant {
  state { x: Int }
  init { x = 0 }
  action noop() {
    x = x
  }
  invariant BadOld { old(x) == x }
}
"""
    with pytest.raises(FslError) as exc:
        verify(_spec(src), 1)
    assert "old() is only allowed in ensures or trans clauses" in str(exc.value)


def test_trans_unknown_identifier_errors_like_invariant_expression():
    src = """
spec TransUnknownIdentifier {
  state { x: Int }
  init { x = 0 }
  action noop() {
    x = x
  }
  trans BadName { old(x) == missing }
}
"""
    with pytest.raises(FslError) as exc:
        verify(_spec(src), 1)
    assert "unknown identifier 'missing'" in str(exc.value)


def test_induction_trans_step_case_blocks_proved_when_bmc_depth_passes():
    src = """
spec TransInductionCti {
  state { x: Int }
  init { x = 0 }
  action stay() {
    requires x == 0
    x = 0
  }
  action ghost_jump() {
    requires x != 0
    x = x + 2
  }
  invariant NonNegative { x >= 0 }
  trans MaxStepOne { x <= old(x) + 1 }
}
"""
    spec = _spec(src)
    bounded = verify(spec, 2, deadlock_mode="ignore")
    assert bounded["result"] == "verified"

    r = prove(spec, 1, 2, deadlock_mode="ignore")
    assert r["result"] == "unknown_cti"
    assert r["trans"] == "MaxStepOne"
    assert r["invariant"] == "MaxStepOne"
    assert r["cti"]["violated_at"] == 1
    assert r["transitions_checked"] == ["MaxStepOne"]
