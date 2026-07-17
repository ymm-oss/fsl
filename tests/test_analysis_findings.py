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


def test_unread_state_uses_transitive_relevance_closure(tmp_path):
    path = _write(tmp_path, """
spec StateRelevance {
  state {
    source: Int,
    derived: Int,
    observed: Int,
    dead_source: Int,
    dead_derived: Int,
    audit: Int
  }
  init {
    source = 0
    derived = 0
    observed = 0
    dead_source = 0
    dead_derived = 0
    audit = 0
  }
  action update_source() {
    source = source + 1
  }
  action derive() {
    derived = source + 1
  }
  action observe() {
    observed = derived + 1
  }
  action update_dead_source() {
    dead_source = dead_source + 1
  }
  action derive_dead() {
    dead_derived = dead_source + 1
  }
  action audit_event() "REQ-AUDIT: audit is externally consumed" {
    audit = audit + 1
  }
  invariant ObservedMatters { observed >= 0 }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    unread = _findings(out, "unread_state")
    unread_nodes = {tuple(f["involved_nodes"]) for f in unread}
    unconstrained = _findings(out, "unconstrained_effect")
    unconstrained_nodes = {f["involved_nodes"][0] for f in unconstrained}

    # The bounded semantic probe upgrades these two structural unread findings
    # to unconstrained_effect and suppresses the duplicate report.
    assert "state:dead_source" in unconstrained_nodes
    assert "state:dead_derived" in unconstrained_nodes
    assert ("state:dead_source",) not in unread_nodes
    assert ("state:dead_derived",) not in unread_nodes
    assert ("state:source",) not in unread_nodes
    assert ("state:derived",) not in unread_nodes
    assert ("state:observed",) not in unread_nodes
    assert ("state:audit",) not in unread_nodes
    assert all(f["witness"]["kind"] == "state_influences_no_check" for f in unread)
    assert all(f["formal_status"] == "not_a_violation" for f in unread)


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
    unconstrained = _findings(out, "unconstrained_effect")

    # The reachable broad-vs-guarded choice produces different unconstrained y
    # successors, so the semantic question supersedes the structural no-guard report.
    assert not any(f["involved_nodes"] == ["action:broad"] for f in unguarded)
    assert any(f["involved_nodes"][0] == "state:y" for f in unconstrained)
    assert not any(f["involved_nodes"] == ["action:guarded"] for f in unguarded)
    assert all(f["candidate_repairs"] and f["do_not_assume"] for f in unguarded)


def test_conservation_candidate_from_counter_effects(tmp_path):
    path = _write(tmp_path, """
spec ConservationCandidate {
  state { stock: Int, reserved: Int, audit: Int }
  init { stock = 2  reserved = 0  audit = 0 }
  action reserve() {
    stock = stock - 2
    reserved = reserved + 1
  }
  action release() {
    stock = stock + 2
    reserved = reserved - 1
  }
  action audit_event() {
    audit = audit + 1
  }
  invariant Any "MODEL: baseline" { true }
}
""")

    out = run_analyze(str(path), profile="ai-review")
    candidates = _findings(out, "conservation_candidate")

    assert len(candidates) == 1
    candidate = candidates[0]
    assert candidate["formal_status"] == "not_a_violation"
    assert candidate["witness"]["kind"] == "weighted_sum_conservation_candidate"
    assert candidate["witness"]["expression"] == "2*reserved + stock"
    assert candidate["witness"]["weights"] == {"reserved": 2, "stock": 1}
    assert {"state:reserved", "state:stock", "action:reserve", "action:release"}.issubset(
        set(candidate["involved_nodes"])
    )
    assert all(item["weighted_sum_delta"] == 0 for item in candidate["witness"]["action_net_effects"])
    assert "verify" in candidate["candidate_repairs"][0]["template"]
