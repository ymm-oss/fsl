from fslc import build_spec, parse, verify


def run_inline(src, depth=8):
    return verify(build_spec(parse(src)), depth)


def test_forall_over_set_reports_member_binding():
    src = """
spec SetQuant {
  type User = 0..2
  state { active: Set<User> }
  init { active = Set {0, 2} }
  action noop() { }
  invariant NoUserTwo { forall u in active { u < 2 } }
}
"""
    out = run_inline(src)
    assert out["result"] == "violated"
    assert out["violation_kind"] == "invariant"
    assert out["violating_bindings"] == [{"u": 2}]


def test_exists_over_seq_uses_live_prefix_only():
    src = """
spec SeqQuant {
  const CAP = 3
  type Item = 0..2
  state { q: Seq<Item, CAP> }
  init { q = Seq {0, 1} }
  action noop() { }
  invariant HasOne { exists x in q { x == 1 } }
  invariant NoTwo { forall x in q { x != 2 } }
}
"""
    out = run_inline(src)
    assert out["result"] == "verified"


def test_unique_and_exactly_one_predicates():
    src = """
spec OnePredicates {
  type User = 0..2
  state { locked: Map<User, Bool>, active: Set<User> }
  init {
    forall u: User { locked[u] = u < 2 }
    active = Set {0}
  }
  action noop() { }
  invariant AtMostOneLocked { unique(u: User where locked[u]) }
  invariant ExactlyOneActive { exactlyOne(u in active) }
}
"""
    out = run_inline(src)
    assert out["result"] == "violated"
    assert out["invariant"] == "AtMostOneLocked"


def test_leadsto_within_reports_deadline_miss():
    src = """
spec WithinDeadline {
  type Step = 0..3
  state { x: Step }
  init { x = 0 }
  action inc() {
    requires x < 3
    x = x + 1
  }
  invariant Range { true }
  leadsTo TooFast { x == 1 ~> within 1 x == 3 }
}
"""
    out = run_inline(src, depth=3)
    assert out["result"] == "violated"
    assert out["violation_kind"] == "leadsTo"
    assert out["within"] == 1
    assert out["deadline"] == out["pending_since"] + 1


def test_unless_sugar_rejects_early_drop_before_release():
    src = """
spec UnlessSugar {
  state { held: Bool, released: Bool }
  init { held = true  released = false }
  action drop() { held = false }
  action release() { released = true  held = false }
  unless HeldUnlessReleased { held unless released }
}
"""
    out = run_inline(src)
    assert out["result"] == "violated"
    assert out["violation_kind"] == "trans"
    assert out["invariant"] == "HeldUnlessReleased"


def test_until_sugar_adds_progress_obligation():
    src = """
spec UntilSugar {
  state { held: Bool, released: Bool }
  init { held = true  released = false }
  action wait() { held = true }
  until HeldUntilReleased { held until released }
}
"""
    out = run_inline(src, depth=4)
    assert out["result"] == "violated"
    assert out["violation_kind"] == "leadsTo"
    assert out["invariant"] == "HeldUntilReleased"
