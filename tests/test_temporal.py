"""FSL v2.0-lite temporal: leadsTo and fair (DESIGN-temporal.md §6)."""
import sys
import ast
import subprocess
import tempfile
from pathlib import Path

from fslc import Monitor, parse, build_spec, scenarios, verify, prove
from fslc.cli import run_testgen

SPECS = Path(__file__).resolve().parent.parent / "specs"
ROOT = Path(__file__).resolve().parent.parent
PY = sys.executable


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

SYMMETRIC_TASKS = """
spec SymmetricTasks {
  symmetric type TaskId = 0..2
  enum Status { Pending, Done }
  state { status: Map<TaskId, Status> }
  init {
    forall t: TaskId { status[t] = Pending }
  }
  fair action finish(t: TaskId) {
    requires status[t] == Pending
    status[t] = Done
  }
  action noop() { }
  invariant StatusValid { true }
  leadsTo EveryTaskFinishes {
    forall t: TaskId {
      status[t] == Pending ~> status[t] == Done
    }
  }
}
"""

PLAIN_TASKS = SYMMETRIC_TASKS.replace("symmetric type TaskId", "type TaskId")

SYMMETRIC_TASKS_UNFAIR = SYMMETRIC_TASKS.replace("fair action finish", "action finish")
PLAIN_TASKS_UNFAIR = PLAIN_TASKS.replace("fair action finish", "action finish")


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


def test_symmetric_type_metadata_and_verified_agrees_with_plain_type():
    plain = run_inline(PLAIN_TASKS, depth=6)
    symmetric_spec = build_spec(parse(SYMMETRIC_TASKS))
    symmetric = verify(symmetric_spec, 6)

    assert symmetric_spec["types"]["TaskId"]["symmetric"] is True
    assert symmetric_spec["symmetry"]["TaskId"]["lo"] == 0
    assert symmetric_spec["symmetry"]["TaskId"]["hi"] == 2
    assert plain["result"] == symmetric["result"] == "verified"
    assert plain["leads_to"] == symmetric["leads_to"]


def test_symmetric_enum_metadata():
    src = """
spec SymmetricEnum {
  symmetric enum Worker { A, B, C }
  state { busy: Set<Worker> }
  init { busy = Set {} }
  action mark(w: Worker) { busy = busy.add(w) }
  invariant T { true }
}
"""
    spec = build_spec(parse(src))
    assert spec["types"]["Worker"]["symmetric"] is True
    assert spec["symmetry"]["Worker"]["members"] == ["A", "B", "C"]


def test_symmetric_type_violation_agrees_with_plain_type():
    plain = run_inline(PLAIN_TASKS_UNFAIR, depth=5)
    symmetric = run_inline(SYMMETRIC_TASKS_UNFAIR, depth=5)

    assert plain["result"] == symmetric["result"] == "violated"
    assert plain["violation_kind"] == symmetric["violation_kind"] == "leadsTo"
    assert plain["invariant"] == symmetric["invariant"] == "EveryTaskFinishes"


def test_symmetric_three_entity_leadsto_completes():
    r = run_inline(SYMMETRIC_TASKS, depth=8)
    assert r["result"] == "verified"
    assert r["leads_to"]["EveryTaskFinishes"]["checked_to_depth"] == 8


def test_specs_without_leadsto_output_unchanged():
    baseline_path = SPECS / "cart_v1.fsl"
    ast = parse(baseline_path.read_text(encoding="utf-8"))
    spec = build_spec(ast)
    r = verify(spec, 8)
    assert r["result"] == "verified"
    assert "leads_to" not in r
    keys = {
        "result", "spec", "depth", "invariants_checked", "transitions_checked", "reachables",
        "action_coverage", "deadlock", "warnings", "note", "checked_to_depth",
        "completeness", "cost",
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


def test_mutex_queue_leadsto_response_scenarios_replay_pending_to_satisfied():
    src = (SPECS / "mutex_queue.fsl").read_text(encoding="utf-8")
    spec = build_spec(parse(src))
    r = scenarios(spec, 8, source_lines=src.splitlines())
    assert r["result"] == "scenarios"

    respond = [s for s in r["scenarios"] if s["kind"] == "leadsTo"]
    names = [s["name"] for s in respond]
    assert names == [
        "respond_WaiterGetsLock_p0",
        "respond_WaiterGetsLock_p1",
        "respond_WaiterGetsLock_p2",
    ]
    assert all("__" not in name for name in names)

    mon = Monitor(str(SPECS / "mutex_queue.fsl"))
    for scen in respond:
        p = scen["bindings"]["p"]
        mon.reset()
        states = [mon.state]
        for step in scen["steps"]:
            result = mon.step(step["action"], step["params"])
            assert result["ok"], result
            states.append(mon.state)
        assert states[scen["pending_at"]]["waiters"].count(p) > 0
        assert states[scen["satisfied_at"]]["holder"] == p


def test_leadsto_response_warns_when_antecedent_never_holds_for_binding():
    src = """
spec NeverPending {
  type ProcId = 0..1
  state { x: Int }
  init { x = 0 }
  action stay() { x = 0 }
  invariant Stable { x == 0 }
  leadsTo MaybePending {
    forall p: ProcId {
      (p == 0 or x == 1) ~> x == 0
    }
  }
}
"""
    r = scenarios(build_spec(parse(src)), 4)
    assert r["result"] == "scenarios"
    respond = [s for s in r["scenarios"] if s["kind"] == "leadsTo"]
    assert [s["name"] for s in respond] == ["respond_MaybePending_p0"]
    assert respond[0]["pending_at"] == 0
    assert respond[0]["satisfied_at"] == 0
    assert any(
        "leadsTo MaybePending {'p': 1}: P never holds within depth 4" in w["message"]
        and w.get("hint")
        for w in r["warnings"]
    )


def test_testgen_with_leadsto_scenarios_imports_and_skips():
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_mutex_queue.py"
        gen = run_testgen(str(SPECS / "mutex_queue.fsl"), output=str(out))
        assert gen["result"] == "generated"

        content = out.read_text(encoding="utf-8")
        compile(content, str(out), "exec")
        module = ast.parse(content, filename=str(out))
        test_names = [
            node.name
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name.startswith("test_scenario_")
        ]
        assert "test_scenario_respond_WaiterGetsLock_p0" in test_names
        assert all("__" not in name for name in test_names)

        proc = subprocess.run(
            [str(PY), "-m", "pytest", str(out), "-q"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        assert proc.returncode == 0, proc.stdout + proc.stderr
        assert "skipped" in proc.stdout.lower()
