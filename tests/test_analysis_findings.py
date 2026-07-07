from fslc.cli import run_analyze


def _write(tmp_path, src, name="case.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def _findings(out, finding_type):
    return [f for f in out.get("findings", []) if f["finding_type"] == finding_type]


def test_disconnected_requirement_positive_and_negative(tmp_path):
    path = _write(tmp_path, """
requirements RequirementConnectivity {
  state { x: Int }
  init { x = 0 }
  action idle() "REQ-OK: idle is modeled" { x = x }
  invariant Tagged "REQ-OK: tagged property" { true }
  requirement REQ-MISSING "not formalized yet" {
  }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    disconnected = _findings(out, "disconnected_requirement")

    assert out["result"] == "analyzed"
    assert any("requirement:REQ-MISSING" in f["involved_nodes"] for f in disconnected)
    assert not any("requirement:REQ-OK" in f["involved_nodes"] for f in disconnected)
    assert all(f["formal_status"] == "not_a_violation" for f in out["findings"])


def test_unanchored_property_positive_and_negative(tmp_path):
    path = _write(tmp_path, """
spec PropertyAnchors {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Floating { true }
  invariant AnchoredByState { x >= 0 }
  invariant Tagged "REQ-TAG: tagged property" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    unanchored = _findings(out, "unanchored_property")

    assert any(f["involved_nodes"] == ["invariant:Floating"] for f in unanchored)
    assert not any("invariant:AnchoredByState" in f["involved_nodes"] for f in unanchored)
    assert not any("invariant:Tagged" in f["involved_nodes"] for f in unanchored)


def test_progressless_cycle_is_flagged_for_tagged_cycle_with_non_english_text(tmp_path):
    path = _write(tmp_path, """
spec RetryLoop {
  enum St { Pending, Failed }
  state { st: St }
  init { st = Pending }
  action fail() "REQ-RETRY: 失敗した処理を再試行対象にする" {
    requires st == Pending
    st = Failed
  }
  action retry() "REQ-RETRY: 再試行で保留状態へ戻す" {
    requires st == Failed
    st = Pending
  }
  invariant Tagged "MODEL: baseline" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    cycles = _findings(out, "progressless_cycle")

    assert len(cycles) == 1
    assert cycles[0]["witness"]["kind"] == "representative_cycle"
    assert "action:fail" in cycles[0]["involved_nodes"]
    assert "action:retry" in cycles[0]["involved_nodes"]
    assert cycles[0]["witness"]["attached_progress"] == []
    assert cycles[0]["formal_status"] == "not_a_violation"


def test_progressless_cycle_is_suppressed_by_leadsto_story(tmp_path):
    path = _write(tmp_path, """
spec RetryLoopWithProgress {
  enum St { Pending, Failed }
  state { st: St }
  init { st = Pending }
  action fail() "REQ-RETRY: 失敗した処理を再試行対象にする" {
    requires st == Pending
    st = Failed
  }
  action retry() "REQ-RETRY: 再試行で保留状態へ戻す" {
    requires st == Failed
    st = Pending
  }
  invariant Tagged "MODEL: baseline" { true }
  leadsTo EventuallyFailed "REQ-L: retry loop has an explicit response" {
    st == Pending ~> st == Failed
  }
}
""")

    out = run_analyze(str(path), profile="ai-review")

    assert _findings(out, "progressless_cycle") == []


def test_progressless_cycle_ignores_unanchored_cycle_even_with_english_terms(tmp_path):
    path = _write(tmp_path, """
spec OrdinaryRepetition {
  enum St { Pending, Failed }
  state { st: St }
  init { st = Pending }
  action fail() {
    requires st == Pending
    st = Failed
  }
  action retry() {
    requires st == Failed
    st = Pending
  }
  invariant Tagged "MODEL: baseline" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")

    assert _findings(out, "progressless_cycle") == []


def test_acyclic_spec_has_no_progressless_cycle(tmp_path):
    path = _write(tmp_path, """
spec Acyclic {
  enum St { Open, Done }
  state { st: St }
  init { st = Open }
  action finish() {
    requires st == Open
    st = Done
  }
  invariant Tagged "MODEL: baseline" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")

    assert _findings(out, "progressless_cycle") == []
    assert out == run_analyze(str(path), profile="ai-review")


def test_unwritten_state_positive_and_negative(tmp_path):
    path = _write(tmp_path, """
spec StateWrites {
  state { active: Bool, frozen: Bool }
  init { active = false  frozen = false }
  action activate() {
    requires not active
    active = true
  }
  invariant FrozenReadable { frozen == false }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    unwritten = _findings(out, "unwritten_state")

    assert any(f["involved_nodes"] == ["state:frozen"] for f in unwritten)
    assert not any(f["involved_nodes"] == ["state:active"] for f in unwritten)
    assert all(f["formal_status"] == "not_a_violation" for f in unwritten)


def test_unguarded_action_positive_and_negative(tmp_path):
    path = _write(tmp_path, """
spec ActionGuards {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action guarded() {
    requires x == 0
    x = x + 1
  }
  action broad() {
    y = y + 1
  }
  invariant Any "MODEL: baseline" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    unguarded = _findings(out, "unguarded_action")

    assert any(f["involved_nodes"] == ["action:broad"] for f in unguarded)
    assert not any(f["involved_nodes"] == ["action:guarded"] for f in unguarded)
    assert all(f["candidate_repairs"] and f["do_not_assume"] for f in unguarded)
