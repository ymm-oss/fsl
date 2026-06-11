"""FSL v2.1: Option<scalar> fields inside struct."""
import json

import pytest

from fslc import FslError, Monitor, build_spec, parse, prove, verify


OPTION_STRUCT_SPEC = """
spec OptionStruct {
  type K = 0..1
  type Qty = 0..3

  enum St { Free, Held }
  struct Slot { item: Option<K>, qty: Qty, st: St }

  state {
    s: Slot,
    slots: Map<K, Slot>
  }

  init {
    s = Slot { item: none, qty: 0, st: Free }
    forall k: K { slots[k] = Slot { item: none, qty: 0, st: Free } }
  }

  action hold(k: K) {
    requires s.item == none
    requires slots[k].item == none
    s = Slot { item: some(k), qty: 1, st: Held }
    slots[k] = Slot { item: some(k), qty: 1, st: Held }
  }

  action move(k: K) {
    requires s.item is some(x)
    requires x == k
    s.item = none
    slots[k].item = some(x)
  }

  action clear(k: K) {
    requires slots[k].item is some(x)
    requires slots[k] == Slot { item: some(x), qty: 1, st: Held }
    slots[k] = Slot { item: none, qty: 0, st: Free }
  }

  invariant FreeNone {
    s.st == Free => s.item == none
  }

  invariant StructEqUsesLogicalOption {
    forall k: K {
      slots[k].st == Held => slots[k] == Slot { item: some(k), qty: 1, st: Held }
    }
  }

  reachable HasMapItem {
    slots[0].item != none
  }
}
"""


def _spec(src=OPTION_STRUCT_SPEC):
    return build_spec(parse(src))


def test_option_struct_verify_json_and_physical_names_hidden():
    spec = _spec()
    assert [p["phys"] for p in spec["phys_vars"]] == [
        "s__item__present",
        "s__item__value",
        "s__qty",
        "s__st",
        "slots__item__present",
        "slots__item__value",
        "slots__qty",
        "slots__st",
    ]

    r = verify(spec, 3)
    assert r["result"] == "verified"
    witness = r["reachables"]["HasMapItem"]["witness"]
    assert witness[0]["state"]["s"]["item"] is None
    assert witness[0]["state"]["slots"]["0"]["item"] is None
    assert witness[-1]["state"]["slots"]["0"]["item"] == 0
    assert json.dumps(r).find("__present") == -1
    assert json.dumps(r).find("__value") == -1


def test_option_struct_induction_proved():
    r = prove(_spec(), k_ind=1, base_depth=3)
    assert r["result"] == "proved"
    assert r["engine"] == "induction"


def test_option_struct_monitor_state_matches_display():
    mon = Monitor(_spec())
    assert mon.state == {
        "s": {"item": None, "qty": 0, "st": "Free"},
        "slots": {
            "0": {"item": None, "qty": 0, "st": "Free"},
            "1": {"item": None, "qty": 0, "st": "Free"},
        },
    }
    r = mon.step("hold", {"k": 0})
    assert r["ok"] is True
    assert r["changes"]["slots[0][item]"] == {"from": None, "to": 0}
    assert mon.state["s"]["item"] == 0
    assert mon.state["slots"]["0"]["item"] == 0
    r = mon.step("move", {"k": 0})
    assert r["ok"] is True
    assert r["changes"]["s[item]"] == {"from": 0, "to": None}
    assert mon.state["s"]["item"] is None
    assert mon.state["slots"]["0"]["item"] == 0
    r = mon.step("clear", {"k": 0})
    assert r["ok"] is True
    assert r["changes"]["slots[0][item]"] == {"from": 0, "to": None}
    assert r["changes"]["slots[0][qty]"] == {"from": 1, "to": 0}
    assert r["changes"]["slots[0][st]"] == {"from": "Held", "to": "Free"}
    assert mon.state["slots"]["0"]["item"] is None


def test_option_struct_present_value_bounds_violate_when_some_out_of_range():
    src = """
spec BadOptionStructBound {
  type K = 0..1
  struct S { v: Option<K> }
  state { s: S }
  init { s = S { v: none } }
  action bad() { s.v = some(2) }
  invariant Trivial { true }
}
"""
    r = verify(_spec(src), 1)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "type_bound"
    assert r["invariant"] == "_bounds_s"


def test_option_struct_some_equality_is_type_error_bug10():
    src = """
spec BadSomeEq {
  type K = 0..1
  struct S { v: Option<K> }
  state { s: S }
  init { s = S { v: none } }
  action bad() {
    requires s.v == some(0)
  }
  invariant I { true }
}
"""
    with pytest.raises(FslError) as exc:
        verify(_spec(src), 1)
    assert exc.value.kind == "type"
    assert "Option == and !=" in str(exc.value)
