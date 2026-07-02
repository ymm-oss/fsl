# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Issue #74: monotone-counter auxiliary-invariant suggestions on k-induction CTIs.

k-induction `unknown_cti` results often show a "ghost state" — a state
variable that only ever moves in one direction across the CTI trace but
starts on the unreachable side of its concrete initial value (a huge or
negative counter no real execution could produce). The generic `_CTI_HINT`
says to add *some* auxiliary invariant but not which one; these tests pin
the heuristic that computes a concrete suggestion by diffing the CTI trace
against the concrete initial state (`runtime.Monitor.reset()`), for both a
scalar `Int` counter and a `Map<K, Int>` counter.

This is post-processing only (trace analysis + one concrete `reset()`) — no
solver/engine semantics change, so verdicts (`result`, `k_used`, ...) are
untouched; only `hint` and the new `suggested_invariants` field are additive.
"""
from fslc import parse, build_spec, prove

CTI_HINT = (
    "this state sequence satisfies all invariants but leads to a violation; "
    "the start state may be unreachable — add an auxiliary invariant that excludes it, "
    "then re-run"
)

# `bump()` grows `audit` normally from init (audit=0) forever, so the spec is
# non-vacuous. `step()` is the only action that can make `Sync` fail, and its
# guard (`audit < -100`) is never satisfiable from any real execution — but
# k-induction's k=1 step only assumes invariants hold at the prior state, not
# reachability, so Z3 is free to pick a fictitious `audit < -100` start for
# that prior state. That "ghost" start is exactly the false-start CTI #74
# targets: audit only increases in the 2-state trace, and its step-0 value is
# necessarily below the true initial value (0).
GHOST_SCALAR = """
spec GhostCounter {
  state { audit: Int, y: Int }
  init { audit = 0  y = 0 }

  action bump() {
    audit = audit + 1
  }

  action step() {
    requires audit < -100
    audit = audit + 1
    y = y + 1
  }

  invariant Sync { y <= 4 }
}
"""

# Same idiom, but audit is a per-key Map<Case, Int> counter (the Map<Case,
# Int> audit counter from the issue). All keys share the same init value (0),
# and only the key touched by the CTI's `step` action is trace-monotone.
GHOST_MAP = """
spec GhostMapCounter {
  type Case = 0..1

  state {
    audit: Map<Case, Int>,
    y: Int
  }

  init {
    forall c: Case { audit[c] = 0 }
    y = 0
  }

  action bump(c: Case) {
    audit[c] = audit[c] + 1
  }

  action step(c: Case) {
    requires audit[c] < -100
    audit[c] = audit[c] + 1
    y = y + 1
  }

  invariant Sync { y <= 4 }
}
"""

# Negative fixture: x is trace-monotone (guard `x < 4`) but explicitly
# bounded below by the guard (`x >= 0`), so its CTI start can never be below
# its true initial value (0) — deterministically, regardless of which
# concrete x0 Z3 happens to pick. No suggestion should fire.
BOUNDED_NO_GHOST = """
spec SyncBounded {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() {
    requires x >= 0 and x < 4
    x = x + 1
    y = y + 1
  }
  invariant Sync { y <= 4 }
}
"""


def prove_inline(src, k_ind=1, depth=8):
    return prove(build_spec(parse(src)), k_ind, depth)


def test_scalar_monotone_counter_suggests_lower_bound():
    r = prove_inline(GHOST_SCALAR)
    assert r["result"] == "unknown_cti"
    assert r["invariant"] == "Sync"
    assert r["suggested_invariants"] == ["audit >= 0"]
    assert r["hint"].startswith(CTI_HINT)
    assert "audit" in r["hint"]
    assert "audit >= 0" in r["hint"]

    cti0 = r["cti"]["states"][0]["state"]
    assert cti0["audit"] < 0  # the ghost start the suggestion targets


def test_scalar_monotone_counter_suggestion_closes_the_loop():
    """Adding the suggested invariant makes k-induction prove Sync."""
    with_aux = GHOST_SCALAR.replace(
        "invariant Sync { y <= 4 }",
        "invariant Sync { y <= 4 }\n  invariant AuditNonNeg { audit >= 0 }",
    )
    r = prove_inline(with_aux)
    assert r["result"] == "proved"
    assert r["k_used"]["Sync"] == 1
    assert r["k_used"]["AuditNonNeg"] == 1


def test_map_monotone_counter_suggests_forall_lower_bound():
    r = prove_inline(GHOST_MAP)
    assert r["result"] == "unknown_cti"
    assert r["invariant"] == "Sync"
    assert r["suggested_invariants"] == ["forall k: Case { audit[k] >= 0 }"]
    assert r["hint"].startswith(CTI_HINT)
    assert "audit" in r["hint"]

    cti0 = r["cti"]["states"][0]["state"]["audit"]
    touched_key = next(k for k in r["cti"]["states"][1]["changes"] if k.startswith("audit["))
    key = touched_key.split("[")[1].rstrip("]")
    assert cti0[key] < 0  # the ghost start the suggestion targets


def test_map_monotone_counter_suggestion_closes_the_loop():
    with_aux = GHOST_MAP.replace(
        "invariant Sync { y <= 4 }",
        "invariant Sync { y <= 4 }\n  invariant AuditNonNeg { forall k: Case { audit[k] >= 0 } }",
    )
    r = prove_inline(with_aux)
    assert r["result"] == "proved"
    assert r["k_used"]["Sync"] == 1
    assert r["k_used"]["AuditNonNeg"] == 1


def test_bounded_counter_yields_no_suggestion():
    """x is trace-monotone but never below its true init — no ghost start."""
    r = prove_inline(BOUNDED_NO_GHOST)
    assert r["result"] == "unknown_cti"
    assert r["invariant"] == "Sync"
    assert "suggested_invariants" not in r
    assert r["hint"] == CTI_HINT
