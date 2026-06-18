from fslc import build_spec, parse, prove, verify


def _spec(src):
    return build_spec(parse(src))


STAGE_PROGRESS = """
spec StageProgress {
  type Case = 0..1
  enum Stage { Submitted, Paid }
  state { stage: Map<Case, Stage> }
  init {
    forall c: Case { stage[c] = Submitted }
  }
  fair action pay(c: Case) {
    requires stage[c] == Submitted
    stage[c] = Paid
  }
  leadsTo ReviewStarts {
    forall c: Case { stage[c] == Submitted ~> stage[c] == Paid }
  }
}
"""


def test_auto_proves_stage_progression_with_synthesized_rank():
    spec = _spec(STAGE_PROGRESS)

    bounded = verify(spec, 1)
    assert bounded["result"] == "verified"
    assert "proved" not in bounded["leads_to"]["ReviewStarts"]

    result = prove(spec, 1, 1)
    assert result["result"] == "proved"
    entry = result["leads_to"]["ReviewStarts"]
    assert entry["proved"] is True
    assert entry["proof"] == "ranking"
    assert entry["synthesized"] is True
    assert entry["completeness"] == "unbounded"


STUTTER_VIOLATION = """
spec StutterViolation {
  state { x: Int }
  init { x = 0 }
  action wait() {
    requires x == 0
    x = 0
  }
  action finish() {
    requires x == 0
    x = 1
  }
  invariant XRange { x >= 0 and x <= 1 }
  leadsTo EventuallyOne { x == 0 ~> x == 1 }
}
"""


def test_synthesis_never_proves_violable_stuttering_leadsto():
    result = prove(_spec(STUTTER_VIOLATION), 1, 2)
    assert result["result"] == "violated"
    assert result["violation_kind"] == "leadsTo"
    assert result["invariant"] == "EventuallyOne"
    assert "leads_to" not in result


NO_TEMPLATE = """
spec NoTemplateFallback {
  state { flag: Bool }
  init { flag = false }
  action set_flag() {
    requires not flag
    flag = true
  }
  leadsTo FlagEventuallySet { not flag ~> flag }
}
"""


MANUAL_RANK = """
spec ManualRank {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant XRange { x >= 0 and x <= 2 }
  leadsTo ReachTwo {
    x < 2 ~> x == 2
    decreases 2 - x
  }
}
"""


def test_fallback_is_bounded_and_manual_decreases_still_author_supplied():
    fallback = prove(_spec(NO_TEMPLATE), 1, 1)
    assert fallback["result"] == "proved"
    entry = fallback["leads_to"]["FlagEventuallySet"]
    assert entry["checked_to_depth"] == 1
    assert "proved" not in entry
    assert "synthesized" not in entry

    manual = prove(_spec(MANUAL_RANK), 1, 1)
    assert manual["result"] == "proved"
    ranked = manual["leads_to"]["ReachTwo"]
    assert ranked["proved"] is True
    assert ranked["proof"] == "ranking"
    assert ranked["decreases"] == "(2 - x)"
    assert "synthesized" not in ranked
