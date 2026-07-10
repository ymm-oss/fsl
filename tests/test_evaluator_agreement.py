# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

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
and the test fails loudly rather than silently passing. The pin/compare
mechanics live in ``tests/agreement.py`` and are shared with
``tests/test_dialect_conformance.py`` (issue #167), which runs the same check
over the full ``examples/`` corpus, not just ``specs/``.
"""
from __future__ import annotations

from pathlib import Path

import pytest

from fslc.runtime import Monitor

from agreement import assert_expr_agreement
from oracle import ROOT, can_monitor, bfs_oracle

SPECS = ROOT / "specs"

# Bound the per-spec exploration so the suite stays in the fast-loop budget.
MAX_DEPTH = 4
MAX_STATES = 80


def _spec_paths() -> list[Path]:
    return sorted(SPECS.glob("*.fsl"))


@pytest.mark.parametrize("path", _spec_paths(), ids=lambda p: p.name)
def test_symbolic_and_concrete_eval_agree(path: Path):
    ok, reason = can_monitor(path)
    if not ok:
        pytest.skip(f"not monitorable: {reason}")

    spec = Monitor(path).spec
    if not spec.get("invariants") and not spec.get("reachables"):
        pytest.skip("no invariant/reachable expressions to compare")

    oracle = bfs_oracle(path, MAX_DEPTH, collect_phys=MAX_STATES)
    compared = assert_expr_agreement(oracle.phys_snapshots, spec, label=path.name)
    assert compared > 0, f"no comparable (state, expr) pairs for {path.name}"
