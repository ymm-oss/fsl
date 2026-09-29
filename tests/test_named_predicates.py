# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Named predicate frontend sugar (issue #187)."""
from __future__ import annotations

import textwrap

from fslc.bmc import prove, scenarios, verify
from fslc.cli import run_check
from fslc.model import build_spec
from fslc.parser import parse


def _spec(source):
    return build_spec(parse(textwrap.dedent(source)))


def _without_locations(value):
    if isinstance(value, dict):
        return {
            key: _without_locations(item)
            for key, item in value.items()
            if key not in ("loc", "cost")
        }
    if isinstance(value, list):
        return [_without_locations(item) for item in value]
    return value


EXPANDED = """
spec Predicates {
  type X = 0..1
  state { x: X }
  init { x = 0 }
  action stay() { x = x }
  invariant Initial { x == 0 }
  reachable StartsEligible { x == 0 }
}
"""

WITH_DEF = """
spec Predicates {
  type X = 0..1
  state { x: X }
  init { x = 0 }
  def eligible(value: X) = value == 0
  action stay() { x = x }
  invariant Initial { eligible(x) }
  reachable StartsEligible { eligible(x) }
}
"""


def test_def_and_manual_expansion_have_identical_engine_results():
    sugar = _spec(WITH_DEF)
    manual = _spec(EXPANDED)

    assert _without_locations(verify(sugar, 2)) == _without_locations(verify(manual, 2))
    assert _without_locations(prove(sugar, 1, 2)) == _without_locations(prove(manual, 1, 2))
    assert _without_locations(scenarios(sugar, 2)) == _without_locations(scenarios(manual, 2))


def test_expansion_leaves_only_existing_kernel_expression_nodes():
    ast = parse(textwrap.dedent(WITH_DEF))

    rendered = repr(ast)
    assert "('def'," not in rendered
    assert "('call'," not in rendered
    assert "__def" not in rendered


def test_recursive_and_mutually_recursive_defs_are_rejected(tmp_path):
    direct = tmp_path / "direct.fsl"
    direct.write_text(textwrap.dedent("""
        spec Direct {
          state { ok: Bool }
          init { ok = true }
          def loop(v: Bool) = loop(v)
          action stay() { ok = ok }
          invariant Safe { ok }
        }
    """), encoding="utf-8")
    mutual = tmp_path / "mutual.fsl"
    mutual.write_text(textwrap.dedent("""
        spec Mutual {
          state { ok: Bool }
          init { ok = true }
          def first(v: Bool) = second(v)
          def second(v: Bool) = first(v)
          action stay() { ok = ok }
          invariant Safe { ok }
        }
    """), encoding="utf-8")

    direct_out = run_check(str(direct))
    mutual_out = run_check(str(mutual))

    assert direct_out["result"] == "error"
    assert direct_out["kind"] == "semantics"
    assert "loop -> loop" in direct_out["message"]
    assert mutual_out["result"] == "error"
    assert "first -> second -> first" in mutual_out["message"]


def test_undefined_predicate_and_arity_mismatch_are_diagnostics(tmp_path):
    undefined = tmp_path / "undefined.fsl"
    undefined.write_text(textwrap.dedent("""
        spec Undefined {
          state { ok: Bool }
          init { ok = true }
          action stay() { ok = ok }
          invariant Safe { missing(ok) }
        }
    """), encoding="utf-8")
    arity = tmp_path / "arity.fsl"
    arity.write_text(textwrap.dedent("""
        spec Arity {
          state { ok: Bool }
          init { ok = true }
          def valid(v: Bool) = v
          action stay() { ok = ok }
          invariant Safe { valid(ok, ok) }
        }
    """), encoding="utf-8")

    undefined_out = run_check(str(undefined))
    arity_out = run_check(str(arity))

    assert undefined_out["kind"] == "name"
    assert undefined_out["loc"] == {"line": 6, "column": 20}
    assert "undefined predicate 'missing'" in undefined_out["message"]
    assert arity_out["kind"] == "type"
    assert "expects 1 argument(s), got 2" in arity_out["message"]


def test_capture_risk_is_rejected_without_synthetic_name_leak(tmp_path):
    path = tmp_path / "capture.fsl"
    path.write_text(textwrap.dedent("""
        spec Capture {
          type X = 0..1
          state { x: X }
          init { x = 0 }
          def differs(v: X) = forall x: X { v != x }
          action stay() { x = x }
          invariant Safe { differs(x) }
        }
    """), encoding="utf-8")

    out = run_check(str(path))

    assert out["result"] == "error"
    assert "would capture variable 'x'" in out["message"]
    assert "__def" not in out["message"]


# The capture check is not redundant with #1119's scoping fix: `def` expansion
# substitutes argument expressions into the definition body with
# `substitute()` / `_subst_expr()`, neither of which alpha-renames. The two
# specs below are the two possible expansions of the rejected call in
# `test_capture_risk_changes_the_verdict_not_only_the_names`'s `Cap` spec.
# They disagree, so the check defends a change of meaning, not a naming
# preference (#1139).
CAPTURE_ALPHA_RENAMED = """
spec Cap {
  type R = 0..2
  state { a: Map<R,Bool>, done: Bool }
  init { forall r: R { a[r] = (r != 0) }  done = false }
  def allBelow(x: R) = forall k: R where k < x { a[k] }
  action act(r: R) { requires r > 0 and allBelow(r)  done = true }
  invariant NeverDone { not done }
}
"""

CAPTURE_NAIVE_SUBSTITUTION = """
spec Cap {
  type R = 0..2
  state { a: Map<R,Bool>, done: Bool }
  init { forall r: R { a[r] = (r != 0) }  done = false }
  action act(r: R) { requires r > 0 and (forall r: R where r < r { a[r] })  done = true }
  invariant NeverDone { not done }
}
"""


def test_capture_risk_changes_the_verdict_not_only_the_names(tmp_path):
    """#1139: the rejected call's two expansions give opposite verdicts."""
    path = tmp_path / "cap.fsl"
    path.write_text(textwrap.dedent("""
        spec Cap {
          type R = 0..2
          state { a: Map<R,Bool>, done: Bool }
          init { forall r: R { a[r] = (r != 0) }  done = false }
          def allBelow(x: R) = forall r: R where r < x { a[r] }
          action act(r: R) { requires r > 0 and allBelow(r)  done = true }
          invariant NeverDone { not done }
        }
    """), encoding="utf-8")

    rejected = run_check(str(path))
    assert rejected["result"] == "error"
    assert "would capture variable 'r'" in rejected["message"]

    # Capture-avoiding expansion: the guard is never satisfiable, because
    # a[0] is false and every r > 0 ranges over it.
    renamed = verify(_spec(CAPTURE_ALPHA_RENAMED), 3)
    assert renamed["result"] == "verified", renamed

    # Naive (capturing) expansion: `forall r where r < r` is vacuously true,
    # so act(1) fires and the invariant falls. This is the meaning the check
    # refuses to produce silently.
    captured = verify(_spec(CAPTURE_NAIVE_SUBSTITUTION), 3)
    assert captured["result"] == "violated", captured
    assert captured["last_action"]["params"] == {"r": 1}, captured
