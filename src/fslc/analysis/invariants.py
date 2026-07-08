# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Structural invariant candidates for ``fslc analyze``."""
from __future__ import annotations

from fractions import Fraction
from math import gcd

from .tsg import lvalue_root


def conservation_candidates(spec, max_candidates=8):
    """Return structural weighted-sum conservation candidates.

    The analysis is intentionally syntactic: only scalar ``Int`` state updated as
    ``x = x + k`` / ``x = x - k`` with an integer literal or const ``k`` participates.
    """
    counters = sorted(
        name for name, ty in (spec.get("state") or {}).items()
        if ty == ("int",)
    )
    if len(counters) < 2:
        return []

    consts = spec.get("consts") or {}
    excluded = set()
    action_rows = []
    for action in sorted(spec.get("actions") or [], key=lambda a: a["name"]):
        deltas = {}
        for stmt in action.get("stmts") or []:
            _scan_stmt(stmt, set(counters), consts, deltas, excluded, nested=False)
        action_rows.append((f"action:{action['name']}", deltas))

    eligible = [
        counter for counter in counters
        if counter not in excluded
        and any(deltas.get(counter, 0) != 0 for _action, deltas in action_rows)
    ]
    if len(eligible) < 2:
        return []

    matrix = [
        [deltas.get(counter, 0) for counter in eligible]
        for _action, deltas in action_rows
    ]
    basis = _integer_nullspace(matrix)
    candidates = []
    seen = set()
    for vector in basis:
        support = [idx for idx, value in enumerate(vector) if value != 0]
        if len(support) < 2:
            continue
        touching_actions = []
        for action_id, deltas in action_rows:
            row = {eligible[idx]: deltas.get(eligible[idx], 0) for idx in support}
            row = {name: delta for name, delta in row.items() if delta != 0}
            if not row:
                continue
            weighted = sum(vector[eligible.index(name)] * delta for name, delta in row.items())
            touching_actions.append({
                "action": action_id,
                "deltas": row,
                "weighted_sum_delta": weighted,
            })
        if len(touching_actions) < 2:
            continue

        weights = {eligible[idx]: vector[idx] for idx in support}
        marker = tuple(sorted(weights.items()))
        if marker in seen:
            continue
        seen.add(marker)
        candidates.append({
            "expression": _format_weighted_sum(weights),
            "weights": weights,
            "states": [f"state:{name}" for name in sorted(weights)],
            "actions": touching_actions,
            "excluded_counters": sorted(excluded),
        })
        if len(candidates) >= max_candidates:
            break
    return candidates


def _scan_stmt(stmt, counters, consts, deltas, excluded, nested):
    if not isinstance(stmt, tuple) or not stmt:
        return
    tag = stmt[0]
    if tag == "assign":
        root = lvalue_root(stmt[1])
        if root not in counters:
            return
        if nested or stmt[1] != ("var", root):
            excluded.add(root)
            return
        delta = _assignment_delta(root, stmt[2], consts)
        if delta is None:
            excluded.add(root)
            return
        deltas[root] = deltas.get(root, 0) + delta
        return
    if tag == "if":
        for child in stmt[2]:
            _scan_stmt(child, counters, consts, deltas, excluded, nested=True)
        for child in stmt[3]:
            _scan_stmt(child, counters, consts, deltas, excluded, nested=True)
        return
    if tag == "forall_stmt":
        for child in stmt[2]:
            _scan_stmt(child, counters, consts, deltas, excluded, nested=True)


def _assignment_delta(name, expr, consts):
    if not isinstance(expr, tuple) or not expr:
        return None
    if expr[0] != "bin":
        return None
    op = expr[1]
    left = expr[2]
    right = expr[3]
    if op == "+":
        if _is_var(left, name):
            return _int_literal_or_const(right, consts)
        if _is_var(right, name):
            return _int_literal_or_const(left, consts)
        return None
    if op == "-" and _is_var(left, name):
        value = _int_literal_or_const(right, consts)
        return -value if value is not None else None
    return None


def _is_var(expr, name):
    return isinstance(expr, tuple) and expr == ("var", name)


def _int_literal_or_const(expr, consts):
    if not isinstance(expr, tuple) or not expr:
        return None
    if expr[0] == "num":
        return expr[1]
    if expr[0] == "var" and expr[1] in consts:
        return consts[expr[1]]
    if expr[0] == "neg":
        value = _int_literal_or_const(expr[1], consts)
        return -value if value is not None else None
    return None


def _integer_nullspace(matrix):
    if not matrix:
        return []
    rows = [list(row) for row in matrix if any(value != 0 for value in row)]
    if not rows:
        return []
    try:
        rows, pivots = _bareiss_echelon(rows)
    except ArithmeticError:
        return _fraction_nullspace(rows)
    width = len(rows[0]) if rows else len(matrix[0])
    if not pivots:
        return []
    return _nullspace_from_echelon(rows, pivots, width)


def _bareiss_echelon(matrix):
    rows = [list(row) for row in matrix]
    width = len(rows[0])
    pivot_row = 0
    previous_pivot = 1
    pivots = []
    for col in range(width):
        pivot = None
        for cand in range(pivot_row, len(rows)):
            if rows[cand][col] != 0:
                pivot = cand
                break
        if pivot is None:
            continue
        rows[pivot_row], rows[pivot] = rows[pivot], rows[pivot_row]
        pivot_value = rows[pivot_row][col]
        for ridx in range(pivot_row + 1, len(rows)):
            factor = rows[ridx][col]
            if factor == 0:
                continue
            new_row = []
            for value, pivot_part in zip(rows[ridx], rows[pivot_row]):
                updated = pivot_value * value - factor * pivot_part
                if previous_pivot != 1:
                    if updated % previous_pivot != 0:
                        raise ArithmeticError("Bareiss division was not exact")
                    updated = updated // previous_pivot
                new_row.append(updated)
            new_row[col] = 0
            rows[ridx] = new_row
        pivots.append(col)
        previous_pivot = pivot_value
        pivot_row += 1
        if pivot_row == len(rows):
            break
    return rows[:pivot_row], pivots


def _nullspace_from_echelon(rows, pivots, width):
    free_cols = [col for col in range(width) if col not in pivots]
    basis = []
    for free in free_cols:
        vector = [Fraction(0) for _ in range(width)]
        vector[free] = Fraction(1)
        for ridx in reversed(range(len(pivots))):
            pivot_col = pivots[ridx]
            row = rows[ridx]
            tail = sum(Fraction(row[col]) * vector[col] for col in range(pivot_col + 1, width))
            vector[pivot_col] = -tail / Fraction(row[pivot_col])
        int_vector = _primitive_integer_vector(vector)
        if int_vector and any(value != 0 for value in int_vector):
            basis.append(int_vector)
    return sorted(basis, key=lambda v: (sum(1 for item in v if item), v))


def _fraction_nullspace(matrix):
    rows = [[Fraction(value) for value in row] for row in matrix if any(value != 0 for value in row)]
    if not rows:
        return []
    width = len(rows[0])
    row_idx = 0
    pivots = []
    for col in range(width):
        pivot = None
        for cand in range(row_idx, len(rows)):
            if rows[cand][col] != 0:
                pivot = cand
                break
        if pivot is None:
            continue
        rows[row_idx], rows[pivot] = rows[pivot], rows[row_idx]
        pivot_value = rows[row_idx][col]
        rows[row_idx] = [value / pivot_value for value in rows[row_idx]]
        for ridx in range(len(rows)):
            if ridx == row_idx or rows[ridx][col] == 0:
                continue
            factor = rows[ridx][col]
            rows[ridx] = [
                value - factor * pivot_part
                for value, pivot_part in zip(rows[ridx], rows[row_idx])
            ]
        pivots.append(col)
        row_idx += 1
        if row_idx == len(rows):
            break
    return _nullspace_from_echelon(rows, pivots, width)


def _primitive_integer_vector(vector):
    denom = 1
    for value in vector:
        denom = _lcm(denom, value.denominator)
    ints = [int(value * denom) for value in vector]
    common = 0
    for value in ints:
        common = gcd(common, abs(value))
    if common:
        ints = [value // common for value in ints]
    first = next((value for value in ints if value != 0), 0)
    if first < 0:
        ints = [-value for value in ints]
    return ints


def _lcm(a, b):
    if a == 0 or b == 0:
        return 0
    return abs(a * b) // gcd(a, b)


def _format_weighted_sum(weights):
    parts = []
    for name, weight in sorted(weights.items()):
        if weight == 0:
            continue
        magnitude = abs(weight)
        term = name if magnitude == 1 else f"{magnitude}*{name}"
        if not parts:
            parts.append(term if weight > 0 else f"-{term}")
        else:
            parts.append(f"+ {term}" if weight > 0 else f"- {term}")
    return " ".join(parts)
