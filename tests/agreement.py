# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Shared z3-dependent expression-agreement helpers (issue #167).

``tests/oracle.py`` stays Z3-free by charter (its own docstring: "intentionally
avoids Z3"). These helpers pin a concrete ``Monitor._phys`` snapshot into a
symbolic z3 solver and compare ``bmc.eval_expr`` against
``runtime.eval_concrete`` on every invariant/reachable expression — the
expression-level half of the dual-evaluator safety net (CLAUDE.md's "Dual
evaluator + independent oracle"). Used by ``tests/test_evaluator_agreement.py``
(``specs/`` only) and ``tests/test_dialect_conformance.py`` (full corpus).
"""
from __future__ import annotations

import z3

from fslc import bmc
from fslc.runtime import eval_concrete


class PinSkip(Exception):
    """A phys leaf isn't a scalar bmc/runtime can agree on directly."""


def lit(value):
    # bool is a subclass of int -- test bool first.
    if isinstance(value, bool):
        return z3.BoolVal(value)
    if isinstance(value, int):
        return z3.IntVal(value)
    raise PinSkip(f"non-scalar phys leaf: {value!r}")


def _default_for_range(range_sort) -> z3.ExprRef:
    return z3.BoolVal(False) if range_sort == z3.BoolSort() else z3.IntVal(0)


def pin_state(solver: z3.Solver, st: dict, phys: dict) -> None:
    """Pin every phys var to its concrete value. Arrays are pinned as a
    *fully determined* term (``Store`` chain over a constant default), not
    per-key ``Select`` equalities — ``Monitor._phys``'s dict/list always
    enumerates a type's whole finite domain (e.g. every ``Set<Domain>``
    member slot, every ``Map<Domain,_>`` key, every ``Seq`` data slot up to
    its capacity), but a z3 Array is a total function over all integers.
    Per-key equalities leave indices outside that domain free, and a bare
    ``z3.ForAll`` bound check (``set_bounds``, unlike ordinary finite-domain
    ``forall``/``exists`` which unroll to a ground formula — see
    ``assert_expr_agreement``) does quantify over those free indices, letting
    a solver "disagree" via a value no real execution could ever produce."""
    for name, zv in st.items():
        cv = phys[name]
        if z3.is_array(zv):
            sort = zv.sort()
            items = cv.items() if isinstance(cv, dict) else enumerate(cv)
            term = z3.K(sort.domain(), _default_for_range(sort.range()))
            for k, v in items:
                term = z3.Store(term, z3.IntVal(int(k)), lit(v))
            solver.add(zv == term)
        else:
            solver.add(zv == lit(cv))


def spec_exprs(spec) -> list:
    out = []
    for inv in spec.get("invariants", []):
        out.append((f"invariant {inv['name']}", inv["expr"]))
    for reach in spec.get("reachables", []):
        out.append((f"reachable {reach['name']}", reach["expr"]))
    return out


def assert_expr_agreement(phys_snapshots: list, spec: dict, *, label: str) -> int:
    """Pin each phys snapshot into a fresh solver and compare every
    invariant/reachable expression's bmc vs. runtime evaluation. Returns the
    number of (state, expr) pairs actually compared — partial ops and
    non-scalar phys leaves are skipped symmetrically on both evaluators, so a
    low/zero count is not itself a failure (a file with no invariants and no
    reachables legitimately compares nothing).

    Agreement is checked by *proving* it (asserting the pin plus the negated
    equality and checking unsat), not by ``Model.eval``. Some implicit bound
    invariants (``set_bounds`` for ``Set<domain>`` — see ``bmc.py``'s
    ``_eval_expr_uncached``) compile to a genuine ``z3.ForAll`` rather than an
    unrolled ground formula (ordinary user ``forall``/``exists`` over a finite
    domain *is* unrolled into a ground ``And``/``Or``, so this only affects a
    handful of implicit bound checks). ``Model.eval`` does not reliably decide
    a bare quantified term against an unrelated model built only from pin
    constraints, whereas asking the solver to prove no disagreement exists is
    sound (and is exactly the reasoning ``bmc.py`` itself relies on when it
    asserts such a term into the real BMC solve)."""
    exprs = spec_exprs(spec)
    compared = 0
    for phys in phys_snapshots:
        st = bmc.make_state(spec, 0)
        pin = z3.Solver()
        try:
            pin_state(pin, st, phys)
        except PinSkip:
            continue
        # An inconsistent pin would silently invalidate the comparison.
        assert pin.check() == z3.sat, f"pin became unsat for {label}"

        for expr_label, expr in exprs:
            try:
                concrete = eval_concrete(expr, phys, {}, spec)
            except Exception:  # noqa: BLE001 -- partial op / non-total on this state
                continue
            if not isinstance(concrete, (bool, int)):
                continue
            with bmc._eval_cache_scope({}, id(st)):  # noqa: SLF001
                try:
                    sym = bmc.eval_expr(expr, st, {}, spec)
                except Exception:  # noqa: BLE001 -- keep symbolic/concrete skips symmetric
                    continue
            if z3.is_bool(sym):
                expected = z3.BoolVal(bool(concrete))
            elif z3.is_int(sym):
                expected = z3.IntVal(concrete)
            else:
                continue

            disagree = z3.Solver()
            disagree.add(*pin.assertions())
            disagree.add(sym != expected)
            check = disagree.check()
            assert check == z3.unsat, {
                "spec": label, "expr": expr_label, "state": phys,
                "concrete": concrete,
                "disagreement_model": str(disagree.model()) if check == z3.sat else None,
            }
            compared += 1
    return compared
