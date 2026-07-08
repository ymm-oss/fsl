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


FLICKERING_HELPFUL_SRC = r'''spec HelpfulFlickering {
  type Phase = 0..9
  type Credit = 0..1
  state { phase: Phase, done: Bool, credit: Credit }
  init { phase = 0  done = false  credit = 1 }

  fair action helpEven() {
    requires phase % 2 == 0
    requires done == false
    done = true
    credit = 0
  }

  fair action helpOdd() {
    requires phase % 2 == 1
    requires done == false
    done = true
    credit = 0
  }

  action rotate() {
    requires done == false
    phase = (phase + 1) % 10
  }

  leadsTo Finishes {
    done == false ~> done == true
    helpful helpEven()
    helpful helpOdd()
    decreases credit
  }
}
'''


FLICKERING_HELPFUL_SYNTHESIZED_SRC = r'''spec HelpfulFlickeringSynthesized {
  type Phase = 0..9
  type Credit = 0..1
  state { phase: Phase, credit: Credit }
  init { phase = 0  credit = 0 }

  fair action helpEven() {
    requires phase % 2 == 0
    requires credit == 0
    credit = 1
  }

  fair action helpOdd() {
    requires phase % 2 == 1
    requires credit == 0
    credit = 1
  }

  action rotate() {
    requires credit == 0
    phase = (phase + 1) % 10
  }

  leadsTo Finishes {
    credit == 0 ~> credit == 1
    helpful helpEven()
    helpful helpOdd()
  }
}
'''


PUMPED_MEASURE_SRC = r'''spec HelpfulPumpedMeasure {
  state { x: Int }
  init { x = 5 }

  fair action work() {
    requires x > 0
    x = x - 1
  }

  action pump() {
    requires x > 0
    x = x + 2
  }

  invariant NonNeg { x >= 0 }

  leadsTo Finishes {
    x > 0 ~> x == 0
    helpful work()
    decreases x
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


def test_multiple_helpful_actions_with_flickering_enabledness_is_not_falsely_proved(tmp_path):
    # Regression (soundness): two `helpful` actions whose enabledness
    # alternates by phase parity used to make induction report "proved" --
    # `_prove_leadsto_rank_no_deadlock` only checked that *some* helpful
    # match is enabled at every pending state (true here, disjunctively),
    # but neither helpEven nor helpOdd is ever *continuously* enabled, so
    # weak fairness never actually obligates either one to fire. `rotate`
    # can cycle forever without either ever firing, so `Finishes` is
    # genuinely false -- BMC finds the fair counterexample at a depth
    # beyond the default induction base check, while induction must now
    # honestly report unknown_cti instead of a false "proved".
    spec = _write(tmp_path, FLICKERING_HELPFUL_SRC, "flickering_helpful.fsl")

    induction_out = run_verify(str(spec), 8, "warn", engine="induction", k_ind=1)
    assert induction_out["result"] == "unknown_cti"
    assert induction_out["rank_failure"] == "helpful_action_enabledness_not_sticky"
    assert induction_out["helpful_actions"][0]["name"] in {"helpEven", "helpOdd"}

    bmc_out = run_verify(str(spec), 12, "warn", engine="bmc")
    assert bmc_out["result"] == "violated"
    assert bmc_out["violation_kind"] == "leadsTo"
    assert bmc_out["invariant"] == "Finishes"


def test_non_helpful_action_pumping_the_measure_is_not_falsely_proved(tmp_path):
    # Regression (soundness, single-helpful-action case -- unrelated to
    # sticky/flickering): _prove_leadsto_rank_progress's helpful branch only
    # required a non-helpful action to keep the obligation pending
    # (`Or(q_next, p_next)`), not to avoid *increasing* the measure. `work`
    # is fair and always enabled while pending (so no_deadlock/sticky/
    # fairness all trivially hold with only one helpful match), but the
    # non-fair `pump` action can add more to `x` than `work` ever removes,
    # so a fair execution (work fires regularly, satisfying its own
    # fairness) can still drive `x` to infinity and never reach 0. Induction
    # must report unknown_cti; BMC finds the genuine fair counterexample.
    spec = _write(tmp_path, PUMPED_MEASURE_SRC, "pumped_measure.fsl")

    induction_out = run_verify(str(spec), 8, "warn", engine="induction", k_ind=1)
    assert induction_out["result"] == "unknown_cti"
    assert induction_out["rank_failure"] == "non_helpful_action_increases_measure"
    assert induction_out["last_action"]["name"] == "pump"

    bmc_out = run_verify(str(spec), 8, "warn", engine="bmc")
    assert bmc_out["result"] == "violated"
    assert bmc_out["violation_kind"] == "leadsTo"
    assert bmc_out["invariant"] == "Finishes"


def test_synthesized_measure_does_not_falsely_claim_unbounded_ranking_proof(tmp_path):
    # Regression: the same flickering-enabledness unsoundness reached the
    # *auto-synthesized* measure path too, via a separate loop in
    # _prove_ranked_leadstos (candidates from _synthesize_leadsto_measures)
    # that originally called lower_bound/helpful_fairness/no_deadlock/progress
    # but not the new helpful_sticky check. Without sticky wired into both
    # loops, this spec's synthesized measure `1 - credit` passed every other
    # check and induction reported leads_to.Finishes with
    # `proof:"ranking", synthesized:true, completeness:"unbounded"` -- a
    # false unbounded claim for a leadsTo that is genuinely violated (same
    # flickering helpEven/helpOdd pattern, no explicit `decreases`).
    spec = _write(tmp_path, FLICKERING_HELPFUL_SYNTHESIZED_SRC, "flickering_synth.fsl")

    induction_out = run_verify(str(spec), 8, "warn", engine="induction", k_ind=1)
    assert induction_out["result"] == "proved"  # base invariants only, not the leadsTo
    finishes = induction_out["leads_to"]["Finishes"]
    assert "proof" not in finishes
    assert finishes.get("completeness") != "unbounded"

    bmc_out = run_verify(str(spec), 12, "warn", engine="bmc")
    assert bmc_out["result"] == "violated"
    assert bmc_out["violation_kind"] == "leadsTo"
    assert bmc_out["invariant"] == "Finishes"
