"""FSL v2.0-lite temporal: leadsTo and fair (DESIGN-temporal.md §6)."""
from pathlib import Path

from fslc import parse, build_spec, verify, prove

SPECS = Path(__file__).resolve().parent.parent / "specs"
ROOT = Path(__file__).resolve().parent.parent
PY = ROOT / ".venv" / "bin" / "python"


def run_inline(src, depth=8, **kwargs):
    return verify(build_spec(parse(src)), depth, **kwargs)


def prove_inline(src, depth=8, k_ind=1, **kwargs):
    return prove(build_spec(parse(src)), k_ind, depth, **kwargs)


STUTTER_SPEC = """
spec StutterLeadsto {
  state { phase: Int }
  init { phase = 0 }
  action advance() {
    requires phase < 7
    phase = phase + 1
  }
  action finalize() {
    requires phase == 7
    phase = 8
  }
  invariant PhaseRange { phase >= 0 and phase <= 8 }
  leadsTo MustFinish { phase == 8 ~> phase == 9 }
}
"""

MUTEX_NO_FAIR = """
spec MiniMutex {
  type ProcId = 0..1
  const NPROC = 2
  state {
    holder: Option<ProcId>,
    waiters: Seq<ProcId, NPROC>
  }
  init {
    holder = none
    waiters = Seq {}
  }
  action acquire_free(p: ProcId) {
    requires holder == none
    requires waiters.size() == 0
    holder = some(p)
  }
  action enqueue(p: ProcId) {
    requires holder != none
    requires not waiters.contains(p)
    requires not (holder is some(h) and h == p)
    waiters = waiters.push(p)
  }
  action release_handoff() {
    requires holder != none
    requires waiters.size() > 0
    holder = some(waiters.head())
    waiters = waiters.pop()
  }
  action noop() { }
  invariant HolderNotWaiting {
    holder is some(h) => not waiters.contains(h)
  }
  leadsTo WaiterGetsLock {
    forall p: ProcId {
      waiters.contains(p) ~> (holder is some(h) and h == p)
    }
  }
}
"""

MUTEX_FAIR = MUTEX_NO_FAIR.replace(
    "action release_handoff()",
    "fair action release_handoff()",
)

STUCK_EARLY_SPEC = """
spec StuckEarly {
  state { x: Int, want: Bool }
  init { x = 0  want = false }
  action ask() {
    requires x == 0
    want = true
    x = 1
  }
  invariant T { true }
  leadsTo WantServed { want ~> x == 2 }
}
"""

SEQ_LOOP_SPEC = """
spec SeqTailLoop {
  const N = 3
  state { q: Seq<Int, N> }
  init { q = Seq {1, 2} }
  action touch() {
    requires q.size() < N
    q = q.push(0)
  }
  action restore() {
    requires q.size() > 0
    q = q.pop()
  }
  invariant LenRange { q.size() >= 0 and q.size() <= N }
  leadsTo MustGrow { q.size() == 2 ~> q.size() == 3 }
}
"""


def test_stutter_violation():
    r = run_inline(STUTTER_SPEC)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert r["stutter"] is True
    assert "loop_start" not in r
    assert r["pending_since"] >= 0


def test_stutter_early_deadlock_not_missed_at_depth():
    r = run_inline(STUCK_EARLY_SPEC, depth=6)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert r["stutter"] is True
    assert r["pending_since"] >= 0


def _trace_states_equivalent(trace, i, j):
    return trace[i]["state"] == trace[j]["state"]


def test_lasso_loop_start_matches_trace_tail():
    r = run_inline(MUTEX_NO_FAIR)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert r["stutter"] is False
    loop_start = r["loop_start"]
    trace = r["trace"]
    assert loop_start < len(trace)
    assert _trace_states_equivalent(trace, loop_start, len(trace) - 1)
    assert f"loop from step {loop_start}" in r["hint"]


def test_lasso_without_fairness():
    r = run_inline(MUTEX_NO_FAIR)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert r["stutter"] is False
    assert "loop_start" in r
    assert "fair action" in r["hint"]


def test_fairness_eliminates_lasso():
    r = run_inline(MUTEX_FAIR)
    assert r["result"] == "verified"
    assert r["leads_to"]["WaiterGetsLock"]["checked_to_depth"] == 8


def test_simultaneous_p_and_q_not_violation():
    src = """
spec Simul {
  state { x: Int }
  init { x = 0 }
  action set() { x = 1 }
  invariant XRange { x >= 0 and x <= 1 }
  leadsTo Immediate { x == 1 ~> x == 1 }
}
"""
    r = run_inline(src)
    assert r["result"] == "verified"
    assert r["leads_to"]["Immediate"]["checked_to_depth"] == 8


def test_forall_leadsto_bindings_on_violation():
    src = """
spec ForallWait {
  type ProcId = 0..1
  const NPROC = 2
  state {
    holder: Option<ProcId>,
    waiters: Seq<ProcId, NPROC>
  }
  init {
    holder = none
    waiters = Seq {}
  }
  action acquire_free(p: ProcId) {
    requires holder == none
    requires waiters.size() == 0
    holder = some(p)
  }
  action enqueue(p: ProcId) {
    requires holder != none
    requires not waiters.contains(p)
    requires not (holder is some(h) and h == p)
    waiters = waiters.push(p)
  }
  action release_handoff() {
    requires holder != none
    requires waiters.size() > 0
    holder = some(waiters.head())
    waiters = waiters.pop()
  }
  action noop() { }
  invariant HolderNotWaiting {
    holder is some(h) => not waiters.contains(h)
  }
  leadsTo AllWaiters {
    forall p: ProcId {
      waiters.contains(p) ~> (holder is some(h) and h == p)
    }
  }
}
"""
    r = run_inline(src)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert "bindings" in r
    assert "p" in r["bindings"]


def test_logical_equiv_seq_tail_loop_detected():
    r = run_inline(SEQ_LOOP_SPEC)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "leadsTo"
    assert r["stutter"] is False
    assert "loop_start" in r
    trace = r["trace"]
    assert _trace_states_equivalent(trace, r["loop_start"], len(trace) - 1)


def test_specs_without_leadsto_output_unchanged():
    baseline_path = SPECS / "cart_v1.fsl"
    ast = parse(baseline_path.read_text(encoding="utf-8"))
    spec = build_spec(ast)
    r = verify(spec, 8)
    assert r["result"] == "verified"
    assert "leads_to" not in r
    keys = {
        "result", "spec", "depth", "invariants_checked", "reachables",
        "action_coverage", "deadlock", "warnings", "note",
    }
    assert keys.issubset(set(r.keys()))
    extra = set(r.keys()) - keys
    assert not extra

    r2 = prove(build_spec(parse((SPECS / "cart_v1.fsl").read_text())), 1, 8)
    assert r2["result"] == "proved"
    assert "leads_to" not in r2


def test_mutex_queue_verify_and_induction_unchanged():
    ast = parse((SPECS / "mutex_queue.fsl").read_text(encoding="utf-8"))
    spec = build_spec(ast)
    r = verify(spec, 8)
    assert r["result"] == "verified"
    assert r["leads_to"]["WaiterGetsLock"]["checked_to_depth"] == 8
    r2 = prove(spec, 1, 8)
    assert r2["result"] == "proved"
    assert r2["leads_to"]["WaiterGetsLock"]["checked_to_depth"] == 8


def test_induction_with_leadsto():
    src = """
spec IndLeadsto {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 3
    x = x + 1
  }
  invariant XRange { x >= 0 and x <= 3 }
  leadsTo ReachThree { x == 2 ~> x == 3 }
}
"""
    r = prove_inline(src)
    assert r["result"] == "proved"
    assert r["leads_to"]["ReachThree"]["checked_to_depth"] == 8
    assert "note" in r
