import z3

from fslc import Monitor, build_spec, parse, prove, verify
from fslc.bmc import eval_expr
from fslc.runtime import _euc_div, _euc_mod


def verify_src(src, depth=8):
    return verify(build_spec(parse(src)), depth)


def prove_src(src, k_ind=2, base_depth=8):
    return prove(build_spec(parse(src)), k_ind, base_depth)


MEETING_ROOM_SPEC = """
spec MeetingRooms {
  const ROOMS = 3
  const SLOTS = 4
  type Room = 0..2
  type Cell = 0..11
  state { booked: Map<Cell, Bool> }
  init { forall c: Cell { booked[c] = false } }
  action book(c: Cell) {
    requires not booked[c]
    booked[c] = true
  }
  invariant SlotProjection {
    forall c: Cell { c % SLOTS >= 0 and c % SLOTS < SLOTS }
  }
  reachable Room0Full {
    forall c: Cell { c / SLOTS == 0 => booked[c] }
  }
  reachable Room1Full {
    forall c: Cell { c / SLOTS == 1 => booked[c] }
  }
  reachable Room2Full {
    forall c: Cell { c / SLOTS == 2 => booked[c] }
  }
  reachable Room1Slot2Booked {
    exists c: Cell { c / SLOTS == 1 and c % SLOTS == 2 and booked[c] }
  }
}
"""


def test_meeting_room_flattening_verified_and_proved():
    verified = verify_src(MEETING_ROOM_SPEC, depth=4)
    assert verified["result"] == "verified", verified
    assert set(verified["reachables"]) == {
        "Room0Full",
        "Room1Full",
        "Room2Full",
        "Room1Slot2Booked",
    }

    proved = prove_src(MEETING_ROOM_SPEC, k_ind=2, base_depth=4)
    assert proved["result"] == "proved", proved


def test_z3_and_runtime_divmod_agree_for_small_signed_range():
    spec = build_spec(parse("""
spec EvalOnly {
  state { x: Int }
  init { x = 0 }
  action noop() { }
  invariant T { true }
}
"""))
    for a in range(-7, 8):
        for b in range(-3, 4):
            div_ast = ("bin", "/", ("num", a), ("num", b))
            mod_ast = ("bin", "%", ("num", a), ("num", b))
            z3_div = z3.simplify(eval_expr(div_ast, {}, {}, spec)).as_long()
            z3_mod = z3.simplify(eval_expr(mod_ast, {}, {}, spec)).as_long()
            assert z3_div == _euc_div(a, b), (a, b, z3_div, _euc_div(a, b))
            assert z3_mod == _euc_mod(a, b), (a, b, z3_mod, _euc_mod(a, b))


def test_division_witness_replays_in_runtime_monitor():
    src = """
spec DivReplay {
  state { q: Int, r: Int }
  init { q = 0  r = 0 }
  action calc() {
    q = -7 / 2
    r = -7 % 2
  }
  reachable Hit { q == -4 and r == 1 }
}
"""
    result = verify_src(src, depth=1)
    assert result["result"] == "verified", result
    witness = result["reachables"]["Hit"]["witness"]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    step = witness[-1]["action"]
    outcome = mon.step(step["name"], step["params"])
    assert outcome["ok"] is True, outcome
    assert mon.state == witness[-1]["state"] == {"q": -4, "r": 1}


def test_division_partial_op_in_action_contexts_and_guards():
    unguarded_body = """
spec DivUnguardedBody {
  type D = 0..1
  state { x: Int }
  init { x = 0 }
  action bad(d: D) { x = 10 / d }
}
"""
    r = verify_src(unguarded_body, depth=1)
    assert r["result"] == "violated", r
    assert r["violation_kind"] == "partial_op"
    assert r["invariant"] == "_partial_bad"

    requires_site = """
spec DivRequiresSite {
  type D = 0..1
  state { x: Int }
  init { x = 0 }
  action bad(d: D) { requires 0 / d == 0 }
}
"""
    r = verify_src(requires_site, depth=1)
    assert r["result"] == "violated", r
    assert r["violation_kind"] == "partial_op"

    ensures_site = """
spec DivEnsuresSite {
  type D = 0..1
  state { x: Int }
  init { x = 0 }
  action bad(d: D) { ensures x == old(x) / d }
}
"""
    r = verify_src(ensures_site, depth=1)
    assert r["result"] == "violated", r
    assert r["violation_kind"] == "partial_op"

    guarded_requires = """
spec DivGuardedRequires {
  type D = 0..1
  state { x: Int }
  init { x = 0 }
  action safe(d: D) {
    requires d != 0
    x = 10 / d
  }
}
"""
    assert verify_src(guarded_requires, depth=2)["result"] == "verified"

    guarded_if = """
spec DivGuardedIf {
  type D = 0..1
  state { x: Int }
  init { x = 0 }
  action safe(d: D) {
    if d != 0 {
      x = 10 / d
    }
  }
}
"""
    assert verify_src(guarded_if, depth=2)["result"] == "verified"


def test_euclidean_negative_number_pins():
    assert _euc_div(-7, 2) == -4
    assert _euc_mod(-7, 2) == 1
    assert _euc_div(7, -2) == -3
    assert _euc_mod(7, -2) == 1

    src = """
spec EuclideanPins {
  state { x: Int }
  init { x = 0 }
  action noop() { }
  invariant Pins {
    -7 / 2 == -4 and -7 % 2 == 1 and 7 / -2 == -3 and 7 % -2 == 1
  }
}
"""
    assert verify_src(src, depth=1)["result"] == "verified"


def test_zero_division_is_total_in_property_context():
    src = """
spec ZeroDivProperty {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant DivZeroIsZero { x / 0 == 0 and x % 0 == 0 }
}
"""
    assert verify_src(src, depth=3)["result"] == "verified"
    assert prove_src(src, k_ind=1, base_depth=3)["result"] == "proved"
