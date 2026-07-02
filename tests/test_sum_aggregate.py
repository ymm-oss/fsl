# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""`sum k: T [where ...] { body }` aggregate expression (#91, Phase 1 of #72).

Per-entity liveness measures (`decreases level[c]` inside
`forall c: Case { level[c] > 0 ~> level[c] == 0 }`) always fail the ranking
discipline under interleaving: an action that advances a *different* entity
leaves the bound entity's measure unchanged, which the discipline forbids
(`rank_failure: "non_decreasing_action"`). The documented workaround was a
hand-written global-sum measure (`level[0] + level[1] + level[2]`) that has to
be rewritten every time the entity count changes. `sum` generalizes that
idiom: FSL domains are always bounded, so `sum` enumerates the binder's
domain and folds `+` — no solver-side novelty, and instances-independent.

These tests do NOT touch any `.fsl` under specs/ or examples/, so the corpus
snapshot is unaffected by this file.
"""
from __future__ import annotations

import z3

from fslc import build_spec, parse, prove, verify, FslError
from fslc.cli import run_verify
from fslc.bmc import eval_expr, make_state
from fslc.runtime import Monitor, eval_concrete


def _spec(src):
    return build_spec(parse(src))


def _write(tmp_path, src, name="spec.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


# ---------------------------------------------------------------------------
# 1. Per-entity measure fails ranking (regression baseline); sum measure
#    proves the SAME leadsTo unbounded.
# ---------------------------------------------------------------------------

PER_CASE_REQ = """
requirements SumDemoBaseline {{
  entity Case

  state {{ level: Map<Case, Int> }}
  init {{ forall c: Case {{ level[c] = 2 }} }}

  action step(c: Case) {{
    requires level[c] > 0
    level[c] = level[c] - 1
  }}

  invariant NonNeg {{ forall c: Case {{ level[c] >= 0 }} }}

  leadsTo Responds {{
    forall c: Case {{ level[c] > 0 ~> level[c] == 0 }}
    decreases {measure}
  }}
}}
verify {{
  instances Case = 3
}}
"""


def test_percase_measure_fails_ranking_baseline():
    """Regression: the documented per-entity trap still fails this way."""
    spec = _spec(PER_CASE_REQ.format(measure="level[c]"))
    r = prove(spec, 1, 8)
    assert r["result"] == "unknown_cti"
    assert r["violation_kind"] == "leadsTo_rank"
    assert r["rank_failure"] == "non_decreasing_action"


def test_sum_measure_proves_unbounded_leadsto():
    """The escape: `sum` over the same per-entity quantity proves unbounded."""
    spec = _spec(PER_CASE_REQ.format(measure="sum k: Case { level[k] }"))
    r = prove(spec, 1, 8)
    assert r["result"] == "proved"
    assert r["completeness"] == "unbounded"
    entry = r["leads_to"]["Responds"]
    assert entry["proved"] is True
    assert entry["proof"] == "ranking"
    assert entry["completeness"] == "unbounded"
    assert entry["decreases"] == "sum k: Case { level[k] }"


# ---------------------------------------------------------------------------
# 2. Instances-independence: same spec, same measure, different entity
#    counts — via the `verify { instances Case = N }` block and via the
#    #86 CLI `--instances` override. No measure edit either way.
# ---------------------------------------------------------------------------

SUM_MEASURE_REQ = PER_CASE_REQ.format(measure="sum k: Case { level[k] }")


def test_sum_measure_instances_independent_via_verify_block(tmp_path):
    five_instances = SUM_MEASURE_REQ.replace(
        "instances Case = 3", "instances Case = 5"
    )
    for src in (SUM_MEASURE_REQ, five_instances):
        spec = _spec(src)
        r = prove(spec, 1, 8)
        assert r["result"] == "proved"
        assert r["leads_to"]["Responds"]["completeness"] == "unbounded"
        assert r["leads_to"]["Responds"]["decreases"] == "sum k: Case { level[k] }"


def test_sum_measure_instances_independent_via_cli_override(tmp_path):
    path = _write(tmp_path, SUM_MEASURE_REQ)
    out = run_verify(str(path), 8, "warn", engine="induction", k_ind=1, instances=["Case=5"])
    assert out["result"] == "proved"
    assert out["bounds_overrides"] == {"instances": {"Case": 5}, "values": {}}
    assert out["leads_to"]["Responds"]["completeness"] == "unbounded"
    assert out["leads_to"]["Responds"]["decreases"] == "sum k: Case { level[k] }"


# ---------------------------------------------------------------------------
# 3. `where` clause: conditional sum, checked via `verify` on an invariant,
#    and cross-checked directly between bmc.eval_expr (symbolic) and
#    runtime.eval_concrete (concrete) on the same pinned state.
# ---------------------------------------------------------------------------

SUM_WHERE_INVARIANT = """
spec SumWhereInvariant {
  type Case = 0..2
  state { level: Map<Case, Int> }
  init { forall c: Case { level[c] = 0 } }
  action bump(c: Case) {
    requires level[c] < 2
    level[c] = level[c] + 1
  }
  invariant Bound { (sum k: Case where level[k] > 0 { level[k] }) <= 6 }
}
"""


def test_sum_where_clause_invariant_verifies():
    spec = _spec(SUM_WHERE_INVARIANT)
    r = verify(spec, 6)
    assert r["result"] == "verified"
    assert r["invariants_checked"] == ["Bound"]


def _assert_bmc_monitor_agree(src, expr, tmp_path, name="agree.fsl"):
    """Pin the Monitor's concrete init state into a z3 solver and check that
    bmc.eval_expr and runtime.eval_concrete cannot disagree on `expr` there —
    the dual-evaluator invariant this repo requires for every new construct."""
    path = _write(tmp_path, src, name)
    mon = Monitor(str(path))
    mon.reset()
    phys = mon._phys  # noqa: SLF001 - internal, but this is exactly what both evaluators consume
    spec = mon.spec

    concrete = eval_concrete(expr, phys, {}, spec)

    sym_state = make_state(spec, 0)
    solver = z3.Solver()
    for name, value in phys.items():
        if isinstance(value, dict):
            for k, v in value.items():
                solver.add(z3.Select(sym_state[name], k) == (z3.BoolVal(v) if isinstance(v, bool) else v))
        elif isinstance(value, bool):
            solver.add(sym_state[name] == z3.BoolVal(value))
        else:
            solver.add(sym_state[name] == value)
    symbolic = eval_expr(expr, sym_state, {}, spec)

    solver.push()
    solver.add(symbolic != concrete)
    assert solver.check() == z3.unsat, "bmc.eval_expr and runtime.eval_concrete disagree on sum"
    return concrete


def test_sum_where_clause_bmc_monitor_agree(tmp_path):
    expr = (
        "quant_sum",
        ("binder_typed", "k", "Case", ("bin", ">", ("index", ("var", "level"), ("var", "k")), ("num", 0))),
        ("index", ("var", "level"), ("var", "k")),
    )
    concrete = _assert_bmc_monitor_agree(SUM_WHERE_INVARIANT, expr, tmp_path)
    assert concrete == 0  # init state: every level[k] == 0, so the where filter admits nothing


# ---------------------------------------------------------------------------
# 4. Type errors: Bool body and unknown binder type are check-time errors.
# ---------------------------------------------------------------------------

SUM_BOOL_BODY_REQ = """
spec SumBoolBody {
  type Case = 0..2
  state { flag: Map<Case, Bool> }
  init { forall c: Case { flag[c] = false } }
  action set(c: Case) {
    requires not flag[c]
    flag[c] = true
  }
  leadsTo Responds {
    forall c: Case { not flag[c] ~> flag[c] }
    decreases sum k: Case { flag[k] }
  }
}
"""


def test_sum_bool_body_is_type_error():
    try:
        _spec(SUM_BOOL_BODY_REQ)
        assert False, "expected FslError for Bool sum body"
    except FslError as e:
        assert e.kind == "type"
        assert "Int" in str(e) or "bool" in str(e)


SUM_UNKNOWN_BINDER_TYPE = """
spec SumUnknownBinder {
  type Case = 0..2
  state { level: Map<Case, Int> }
  init { forall c: Case { level[c] = 0 } }
  action step(c: Case) {
    requires level[c] >= 0
    level[c] = level[c] + 1
  }
  invariant Bad { (sum k: Bogus { level[k] }) >= 0 }
}
"""


def test_sum_unknown_binder_type_is_type_error():
    try:
        _spec(SUM_UNKNOWN_BINDER_TYPE)
        assert False, "expected FslError for unknown binder type"
    except FslError as e:
        assert e.kind == "type"
        assert "Bogus" in str(e)


# ---------------------------------------------------------------------------
# 5. Nested sum: fixed, hand-computed expectation.
# ---------------------------------------------------------------------------

NESTED_SUM_SPEC = """
spec NestedSum {
  type A = 0..1
  type B = 0..1
  state { m: Map<A, Int> }
  init { forall a: A { m[a] = 1 } }
  action touch() {
    requires true
    m[0] = m[0]
  }
  invariant NestedCheck { (sum i: A { sum j: B { m[i] } }) == 4 }
}
"""


def test_nested_sum_evaluates_correctly():
    # m[i] == 1 for i in {0, 1}; inner `sum j: B { m[i] }` is m[i] * |B| == 2;
    # outer sum over A of that is 2 + 2 == 4.
    spec = _spec(NESTED_SUM_SPEC)
    r = verify(spec, 1)
    assert r["result"] == "verified"
    assert r["invariants_checked"] == ["NestedCheck"]


# ---------------------------------------------------------------------------
# 6. Kernel spec (non-dialect) usage over a `type Id = 0..2` domain.
# ---------------------------------------------------------------------------

KERNEL_SUM_SPEC = """
spec KernelSumDemo {
  type Id = 0..2
  state { v: Map<Id, Int> }
  init { forall i: Id { v[i] = i } }
  action touch() {
    requires true
    v[0] = v[0]
  }
  invariant SumCheck { (sum i: Id { v[i] }) == 3 }
}
"""


def test_kernel_spec_sum_over_domain_type():
    spec = _spec(KERNEL_SUM_SPEC)
    r = verify(spec, 1)
    assert r["result"] == "verified"
    assert r["invariants_checked"] == ["SumCheck"]


# ---------------------------------------------------------------------------
# 7. `sum` is a contextual keyword (like forall/exists/count): a state
#    variable literally named `sum` still checks cleanly when it never
#    appears immediately before `(` or a binder.
# ---------------------------------------------------------------------------

SUM_NAMED_STATE_VAR = """
spec SumNamedVar {
  state { sum: Int }
  init { sum = 0 }
  action step() {
    requires sum < 5
    sum = sum + 1
  }
  invariant NonNeg { sum >= 0 }
}
"""


def test_state_var_named_sum_does_not_crash():
    spec = _spec(SUM_NAMED_STATE_VAR)
    r = verify(spec, 3)
    assert r["result"] == "verified"
