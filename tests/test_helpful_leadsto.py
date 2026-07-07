# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Helpful-action ranking coverage for per-entity leadsTo (#106)."""

from fslc.cli import run_verify


HELPFUL_SRC = r'''spec HelpfulPerEntity {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }

  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  action idle(c: Case) {
    level[c] = level[c]
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
'''


NONFAIR_HELPFUL_SRC = HELPFUL_SRC.replace("fair action step", "action step")


BLOCKED_HELPFUL_SRC = r'''spec BlockedHelpful {
  type Case = 0..1
  type Level = 0..2
  state {
    level: Map<Case, Level>,
    gate: Map<Case, Bool>
  }
  init {
    forall c: Case {
      level[c] = 2
      gate[c] = false
    }
  }

  fair action step(c: Case) {
    requires gate[c]
    requires level[c] > 0
    level[c] = level[c] - 1
  }

  invariant NonNeg { forall c: Case { level[c] >= 0 } }

  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
'''


def _write(tmp_path, src, name):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_helpful_per_entity_measure_proves_under_interleaving(tmp_path):
    spec = _write(tmp_path, HELPFUL_SRC, "helpful.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "proved"
    proof = out["leads_to"]["Responds"]
    assert proof["proof"] == "ranking"
    assert proof["decreases"] == "level[c]"
    assert proof["helpful"] == ["step(c)"]


def test_helpful_does_not_create_fairness(tmp_path):
    spec = _write(tmp_path, NONFAIR_HELPFUL_SRC, "nonfair_helpful.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "unknown_cti"
    assert out["rank_failure"] == "progress_action_not_fair"
    assert out["helpful_actions"][0]["name"] == "step"
    assert "helpful only identifies" in out["hint"]


def test_blocked_helpful_action_is_reported(tmp_path):
    spec = _write(tmp_path, BLOCKED_HELPFUL_SRC, "blocked_helpful.fsl")
    out = run_verify(str(spec), 1, "warn", engine="induction", k_ind=1)
    assert out["result"] == "unknown_cti"
    assert out["rank_failure"] == "helpful_action_not_enabled"
    assert out["helpful"] == ["step(c)"]
    assert out["bindings"] == {"c": 0}
