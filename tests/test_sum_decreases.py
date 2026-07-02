# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Regression coverage for `decreases sum(...)` (#91).

`docs/LANGUAGE.md` and `skills/fsl/reference.md` wrongly claimed no `sum()`
aggregate is usable in a ranking `decreases` measure, steering readers to
hand-written sums (`decreases level[0] + level[1]`) that only scale to
domains small enough to enumerate by hand. The kernel's bounded
`sum(x: T of expr [where expr])` aggregate (§3 of both docs) has always been
usable there — this file is the regression coverage that was missing when
that false claim was written, using only *existing* syntax (no source
changes accompany this fix).

Contrast: the per-entity trap the docs correctly describe (`decreases
level[c]`) still fails under interleaving — see
`test_per_entity_measure_still_fails_under_interleaving` below, mirroring
the assertion shape in `tests/test_induction.py`.
"""
from fslc.cli import run_verify

KERNEL_SUM_SRC = r'''spec SumRankKernel {
  type Case = 0..2
  state { level: Map<Case, 0..3> }
  init { forall c: Case { level[c] = 3 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  invariant NonNeg { forall c: Case { level[c] >= 0 } }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    decreases sum(k: Case of level[k])
  }
}
'''

KERNEL_SUM_PER_ENTITY_SRC = r'''spec SumRankKernelPerEntity {
  type Case = 0..2
  state { level: Map<Case, 0..3> }
  init { forall c: Case { level[c] = 3 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  invariant NonNeg { forall c: Case { level[c] >= 0 } }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    decreases level[c]
  }
}
'''

KERNEL_SUM_WHERE_SRC = r'''spec SumWhereKernel {
  type Case = 0..2
  state { level: Map<Case, 0..3> }
  init { forall c: Case { level[c] = 3 } }
  action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  invariant BoundedSum { sum(k: Case of level[k] where level[k] > 0) <= 9 }
}
'''

REQ_SUM_SRC = r'''requirements SumRankRequirements {
  entity Case
  number Level

  state { level: Map<Case, Level> }

  init {
    forall c: Case { level[c] = 2 }
  }

  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    decreases sum(k: Case of level[k])
  }
}
verify {
  instances Case = 3
  values Level = 0..3
}
'''


def _write(tmp_path, src, name):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_kernel_sum_measure_proves_unbounded(tmp_path):
    spec = _write(tmp_path, KERNEL_SUM_SRC, "sum_kernel.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "proved"
    entry = out["leads_to"]["Responds"]
    assert entry["proved"] is True
    assert entry["proof"] == "ranking"
    assert entry["completeness"] == "unbounded"
    assert entry["decreases"] == "sum(k: Case of level[k])"


def test_requirements_dialect_sum_measure_proves_unbounded(tmp_path):
    spec = _write(tmp_path, REQ_SUM_SRC, "sum_req.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "proved"
    entry = out["leads_to"]["Responds"]
    assert entry["proved"] is True
    assert entry["completeness"] == "unbounded"


def test_requirements_dialect_sum_measure_composes_with_instances_override(tmp_path):
    # The measure is instances-count independent (#86): overriding the CLI
    # instance count still proves unbounded with the same decreases clause.
    spec = _write(tmp_path, REQ_SUM_SRC, "sum_req_override.fsl")
    out = run_verify(
        str(spec), 1, "warn", engine="induction", k_ind=1, instances=["Case=5"]
    )
    assert out["result"] == "proved"
    entry = out["leads_to"]["Responds"]
    assert entry["proved"] is True
    assert entry["completeness"] == "unbounded"
    assert out["bounds_overrides"] == {"instances": {"Case": 5}, "values": {}}


def test_sum_where_aggregate_verified_under_bmc(tmp_path):
    spec = _write(tmp_path, KERNEL_SUM_WHERE_SRC, "sum_where.fsl")
    out = run_verify(str(spec), 3, "warn")
    assert out["result"] == "verified"


def test_per_entity_measure_still_fails_under_interleaving(tmp_path):
    # Contrast regression (kept honest by docs/LANGUAGE.md): a per-entity
    # measure is a different, still-broken idiom from the sum measure above.
    spec = _write(tmp_path, KERNEL_SUM_PER_ENTITY_SRC, "sum_kernel_per_entity.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "unknown_cti"
    assert out["violation_kind"] == "leadsTo_rank"
    assert out["rank_failure"] == "non_decreasing_action"
    assert out["invariant"] == "Responds"
    assert out["measure"] == "level[c]"
    assert out["last_action"]["name"] == "step"
    assert out["measure_after"] >= out["measure_before"]
