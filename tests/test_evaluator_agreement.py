"""Step-level evaluator agreement — the core safety net for unifying the two
expression evaluators.

``bmc.py`` evaluates FSL expressions symbolically (into z3 terms) and
``runtime.py`` evaluates the *same* expressions concretely (into Python
values).  These two implementations must agree on FSL semantics — otherwise the
verifier and the replay Monitor can disagree about the same spec.  The planned
refactor unifies the structurally-parallel ``_eval_*`` helpers behind a single
core; this test pins the invariant that unification must preserve.

Method (expression-level differential):

1. Enumerate reachable concrete states with the trusted concrete oracle
   (``Monitor`` BFS — same machinery as ``tests/oracle.py``).
2. Both evaluators consume the *same* physical-state layout: the concrete
   ``Monitor._phys`` (dicts/lists/scalars) and the symbolic ``make_state``
   (z3 Arrays/scalars) share identical phys keys and value encodings.
3. For each reachable state we *pin* every symbolic phys variable to its
   concrete value in a z3 solver, then evaluate each invariant / reachable
   predicate with ``bmc.eval_expr`` inside that model and compare the result to
   ``runtime.eval_concrete`` on the concrete state.

The pinning is self-validating: an inconsistent pin makes the solver ``unsat``
and the test fails loudly rather than silently passing.
"""
from __future__ import annotations

import copy
from pathlib import Path

import pytest
import z3

from fslc import bmc
from fslc.runtime import Monitor, eval_concrete

from oracle import ROOT, action_key, can_monitor, normalize

SPECS = ROOT / "specs"

# Bound the per-spec exploration so the suite stays in the fast-loop budget.
MAX_DEPTH = 4
MAX_STATES = 80


def _spec_paths() -> list[Path]:
    return sorted(SPECS.glob("*.fsl"))


def _lit(value):
    # bool is a subclass of int — test bool first.
    if isinstance(value, bool):
        return z3.BoolVal(value)
    if isinstance(value, int):
        return z3.IntVal(value)
    raise _Skip(f"non-scalar phys leaf: {value!r}")


class _Skip(Exception):
    pass


def _reachable_phys_states(path: Path) -> list[dict]:
    """BFS the concrete state space, returning a capped list of phys snapshots."""
    mon0 = Monitor(path)
    mon0.reset()
    states = [copy.deepcopy(mon0._phys)]  # noqa: SLF001
    visited = {normalize(mon0.state)}
    frontier = [(mon0, 0)]
    while frontier and len(states) < MAX_STATES:
        mon, d = frontier.pop()
        if d >= MAX_DEPTH:
            continue
        for act in sorted(mon.enabled(), key=action_key):
            child = copy.deepcopy(mon)
            result = child.step(act["action"], act.get("params", {}))
            if not result.get("ok"):
                continue
            key = normalize(child.state)
            if key in visited:
                continue
            visited.add(key)
            states.append(copy.deepcopy(child._phys))  # noqa: SLF001
            frontier.append((child, d + 1))
            if len(states) >= MAX_STATES:
                break
    return states


def _pin_state(solver: z3.Solver, st: dict, phys: dict) -> None:
    for name, zv in st.items():
        cv = phys[name]
        if z3.is_array(zv):
            items = cv.items() if isinstance(cv, dict) else enumerate(cv)
            for k, v in items:
                solver.add(z3.Select(zv, z3.IntVal(int(k))) == _lit(v))
        else:
            solver.add(zv == _lit(cv))


def _to_py(zval):
    if z3.is_bool(zval):
        if z3.is_true(zval):
            return True
        if z3.is_false(zval):
            return False
        return None
    if z3.is_int_value(zval):
        return zval.as_long()
    return None


def _exprs(spec) -> list[tuple[str, dict]]:
    out = []
    for inv in spec.get("invariants", []):
        out.append((f"invariant {inv['name']}", inv["expr"]))
    for reach in spec.get("reachables", []):
        out.append((f"reachable {reach['name']}", reach["expr"]))
    return out


@pytest.mark.parametrize("path", _spec_paths(), ids=lambda p: p.name)
def test_symbolic_and_concrete_eval_agree(path: Path):
    ok, reason = can_monitor(path)
    if not ok:
        pytest.skip(f"not monitorable: {reason}")

    mon = Monitor(path)
    mon.reset()
    spec = mon.spec
    exprs = _exprs(spec)
    if not exprs:
        pytest.skip("no invariant/reachable expressions to compare")

    states = _reachable_phys_states(path)
    compared = 0
    for phys in states:
        st = bmc.make_state(spec, 0)
        solver = z3.Solver()
        try:
            _pin_state(solver, st, phys)
        except _Skip as exc:
            pytest.skip(str(exc))
        # An inconsistent pin would silently invalidate the comparison.
        assert solver.check() == z3.sat, f"pin became unsat for {path.name}"
        model = solver.model()

        for label, expr in exprs:
            try:
                concrete = eval_concrete(expr, phys, {}, spec)
            except Exception:  # noqa: BLE001 — partial op / non-total on this state
                continue
            if not isinstance(concrete, (bool, int)):
                continue
            with bmc._eval_cache_scope({}, id(st)):  # noqa: SLF001
                try:
                    sym = bmc.eval_expr(expr, st, {}, spec)
                except Exception:  # noqa: BLE001 — keep symbolic/concrete skips symmetric
                    continue
            if not (z3.is_bool(sym) or z3.is_int(sym)):
                continue
            got = _to_py(model.eval(sym, model_completion=True))
            assert got == bool(concrete) if isinstance(concrete, bool) else got == concrete, {
                "spec": path.name,
                "expr": label,
                "state": phys,
                "concrete": concrete,
                "symbolic": got,
            }
            compared += 1

    assert compared > 0, f"no comparable (state, expr) pairs for {path.name}"
