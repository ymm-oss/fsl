"""Tests for the `forbidden` (must-forbid) block in the requirements dialect."""
from fslc.cli import run_check, run_scenarios, run_verify
from fslc.model import build_spec
from fslc.parser import parse_src
from fslc.acceptance import validate_forbidden


def _spec_from(src):
    ast, display = parse_src(src, ".")
    return build_spec(ast, display)


# cancel guarded to Paid only: cancel-after-ship is not enabled (case a).
GUARDED = r'''requirements OrderReq {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  requirement REQ-1 "lifecycle" {
    action pay(o: OrderId)    { requires order[o] == Cart  order[o] = Paid }
    action ship(o: OrderId)   { requires order[o] == Paid  order[o] = Shipped }
    action cancel(o: OrderId) { requires order[o] == Paid  order[o] = Cancelled }
  }
  forbidden FB-1 "出荷後のキャンセルは拒否される" {
    pay(0)  ship(0)
    cancel(0)
    expect rejected
  }
}'''

# cancel guard weakened to also allow Shipped: the forbidden op is accepted.
WEAKENED = GUARDED.replace(
    "action cancel(o: OrderId) { requires order[o] == Paid  order[o] = Cancelled }",
    "action cancel(o: OrderId) { requires order[o] == Paid or order[o] == Shipped  order[o] = Cancelled }",
)

# forbidden whose setup step (ship) is not enabled from init.
SETUP_BROKEN = GUARDED.replace(
    "    pay(0)  ship(0)\n    cancel(0)",
    "    ship(0)\n    cancel(0)",
)

# case (b): the last step is enabled but executing it breaks a type bound.
CASE_B = r'''requirements CapReq {
  type OrderId = 0..1
  type Cnt = 0..1
  enum OSt { Cart, Paid }
  state { order: Map<OrderId, OSt>, paid_count: Cnt }
  init { forall o: OrderId { order[o] = Cart }  paid_count = 0 }
  requirement REQ-1 "cap" {
    action pay(o: OrderId) { requires order[o] == Cart  order[o] = Paid  paid_count = paid_count + 1 }
  }
  forbidden FB-1 "上限超過の支払いは拒否される" {
    pay(0)
    pay(1)
    expect rejected
  }
}'''

NO_STEPS = r'''requirements EmptyReq {
  type OrderId = 0..1
  enum OSt { Cart, Paid }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  requirement REQ-1 "x" { action pay(o: OrderId) { requires order[o] == Cart  order[o] = Paid } }
  forbidden FB-1 "no steps" { expect rejected }
}'''


def _w(tmp_path, src):
    p = tmp_path / "f.fsl"
    p.write_text(src, encoding="utf-8")
    return str(p)


def test_guarded_forbidden_passes_check_and_emits_scenario(tmp_path):
    path = _w(tmp_path, GUARDED)
    assert run_check(path)["result"] == "ok"

    sc = run_scenarios(path, depth=6, deadlock_mode="ignore")
    assert sc["result"] == "scenarios"
    forb = [s for s in sc["scenarios"] if s.get("kind") == "forbidden"]
    assert len(forb) == 1
    s = forb[0]
    assert s["name"] == "forbidden_FB-1"
    assert s["forbidden_step"]["action"] == "cancel"
    assert s["rejected_by"] == "requires_failed"     # case (a): not enabled
    assert s["requirement"] == {"id": "FB-1", "text": "出荷後のキャンセルは拒否される"}


def test_guarded_forbidden_does_not_block_verify(tmp_path):
    path = _w(tmp_path, GUARDED)
    assert run_verify(path, depth=6, deadlock_mode="ignore")["result"] == "verified"


def test_weakened_guard_makes_forbidden_op_accepted(tmp_path):
    path = _w(tmp_path, WEAKENED)
    out = run_check(path)
    assert out["result"] == "error"
    assert out["kind"] == "forbidden"
    assert out["id"] == "FB-1"
    # the wrongly-accepted trace ends with the operation that should be rejected
    assert out["accepted_trace"][-1]["action"] == "cancel"
    # the forbidden gate also fires under verify, before BMC
    assert run_verify(path, depth=6, deadlock_mode="ignore")["kind"] == "forbidden"


def test_broken_setup_is_distinct_error(tmp_path):
    path = _w(tmp_path, SETUP_BROKEN)
    out = run_check(path)
    assert out["result"] == "error"
    assert out["kind"] == "forbidden_setup"     # not a forbidden violation
    assert out["failed_step"] == 0


def test_case_b_rejected_by_violation_coincides_with_verify_violated(tmp_path):
    # At check time the forbidden is satisfied (the last step is rejected by a
    # type-bound violation). But a reachable-violating step means the spec
    # itself fails verify — `rejected_by` surfaces this distinction.
    spec = _spec_from(CASE_B)
    result = validate_forbidden(spec)
    assert result["ok"] is True
    assert result["scenarios"][0]["rejected_by"] == "type_bound"

    path = _w(tmp_path, CASE_B)
    assert run_check(path)["result"] == "ok"
    assert run_verify(path, depth=4, deadlock_mode="ignore")["result"] == "violated"


def test_forbidden_requires_at_least_one_step(tmp_path):
    path = _w(tmp_path, NO_STEPS)
    out = run_check(path)
    assert out["result"] == "error"
    assert out["kind"] == "forbidden"
