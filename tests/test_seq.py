"""FSL v1.1 Seq<T, N> tests (DESIGN-seq.md §8)."""
import json
import subprocess
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify, prove, scenarios, FslError
from fslc.cli import run_check

ROOT = Path(__file__).resolve().parent.parent
PY = ROOT / ".venv" / "bin" / "python"
SPECS = ROOT / "specs"


def verify_src(src, depth=8):
    return verify(build_spec(parse(src)), depth)


def check_src(src, name="_seq_check.fsl"):
    path = SPECS / name
    path.write_text(src, encoding="utf-8")
    try:
        return run_check(str(path))
    finally:
        path.unlink(missing_ok=True)


FIFO_SPEC = """
spec FifoQueue {
  type JobId = 0..2
  const CAP = 2
  state { queue: Seq<JobId, CAP> }
  init { queue = Seq {} }
  action enqueue(j: JobId) {
    requires queue.size() < CAP
    queue = queue.push(j)
  }
  action dequeue() {
    requires queue.size() > 0
    queue = queue.pop()
  }
  invariant FifoOrder {
    forall k in 0..CAP-1 {
      k < queue.size() => queue.at(k) >= 0
    }
  }
  reachable HasTwo { queue.size() == 2 }
  reachable SawFirst {
    queue.size() == 2 and queue.at(0) == 0 and queue.at(1) == 1
  }
  reachable AfterDequeue {
    queue.size() == 1 and queue.at(0) == 1
  }
}
"""

PARTIAL_UNGUARDED = """
spec PartialUnguarded {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action bad_pop() {
    queue = queue.pop()
  }
  action bad_head() {
    requires queue.head() == 0
  }
}
"""

PARTIAL_GUARDED = """
spec PartialGuarded {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action safe_pop() {
    requires queue.size() > 0
    queue = queue.pop()
  }
}
"""

REQUIRES_HEAD_IDIOM = """
spec RequiresHeadIdiom {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action inspect() {
    requires queue.size() > 0
    requires queue.head() == 0
  }
  action fill() {
    requires queue.size() < 2
    queue = queue.push(0)
  }
}
"""


def test_fifo_basic_push_head_pop_size():
  """§8.1: push×2 → head, pop → second head, size consistency."""
  r = verify_src(FIFO_SPEC, depth=6)
  assert r["result"] == "verified"
  saw = r["reachables"]["SawFirst"]
  assert saw["witness"][-1]["state"]["queue"] == [0, 1]
  after = r["reachables"]["AfterDequeue"]
  assert after["witness"][-1]["state"]["queue"] == [1]


def test_full_push_type_bound():
  """§8.2: capacity 2, push 3 times → type_bound on _bounds_queue."""
  src = """
spec FullPush {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action push_a() { queue = queue.push(0) }
  action push_b() { queue = queue.push(1) }
  action push_c() { queue = queue.push(0) }
}
"""
  r = verify_src(src, depth=4)
  assert r["result"] == "violated"
  assert r["violation_kind"] == "type_bound"
  assert r["invariant"] == "_bounds_queue"


def test_empty_pop_head_partial_op_unguarded():
  """§8.3: unguarded pop/head → partial_op with loc and hint."""
  r_pop = verify_src(PARTIAL_UNGUARDED.replace("bad_head", "noop").replace(
      "action bad_head() {\n    requires queue.head() == 0\n  }", ""), depth=2)
  assert r_pop["result"] == "violated"
  assert r_pop["violation_kind"] == "partial_op"
  assert r_pop["invariant"] == "_partial_bad_pop"
  assert r_pop["hint"] == "guard the action with requires q.size() > 0 (or bound the index)"
  assert r_pop.get("loc") is not None
  assert r_pop.get("trace")

  r_head = verify_src("""
spec PartialHead {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action bad_head() {
    requires queue.head() == 0
  }
}
""", depth=2)
  assert r_head["result"] == "violated"
  assert r_head["violation_kind"] == "partial_op"
  assert r_head["invariant"] == "_partial_bad_head"


def test_empty_pop_guarded_verified():
  """§8.3: guarded pop → verified."""
  r = verify_src(PARTIAL_GUARDED, depth=4)
  assert r["result"] == "verified"


def test_requires_head_guard_idiom_no_partial_op():
  """§8.4: requires size>0 + requires head==0 disabled on empty, not partial_op."""
  r = verify_src(REQUIRES_HEAD_IDIOM, depth=4)
  assert r["result"] == "verified"


def test_if_guard_partial_op_not_spurious():
  """BUG15: if q.size() > 0 { head/pop } verified; unguarded bad() still partial_op."""
  guarded = """
spec SeqIf {
  type K = 0..1
  state { q: Seq<K, 2>, x: Int }
  init { q = Seq { 1 }  x = 0 }
  action drain() {
    if q.size() > 0 {
      x = q.head()
      q = q.pop()
    }
  }
  invariant T { true }
}
"""
  r = verify_src(guarded, depth=4)
  assert r["result"] == "verified"

  unguarded = """
spec SeqBad {
  type K = 0..1
  state { q: Seq<K, 2> }
  init { q = Seq {} }
  action bad() { q = q.pop() }
}
"""
  r_bad = verify_src(unguarded, depth=2)
  assert r_bad["result"] == "violated"
  assert r_bad["violation_kind"] == "partial_op"
  assert r_bad["invariant"] == "_partial_bad"


def test_if_else_branch_partial_op_guard():
  """BUG15: else branch with head only when non-empty is verified."""
  src = """
spec SeqIfElse {
  type K = 0..1
  state { q: Seq<K, 2>, x: Int }
  init { q = Seq {}  x = -1 }
  action peek() {
    if q.size() == 0 {
      x = -1
    } else {
      x = q.head()
    }
  }
  invariant T { true }
}
"""
  r = verify_src(src, depth=4)
  assert r["result"] == "verified"


def test_at_forall_guard_invariant_verified():
  """§8.5: log invariant with forall k < log.size() => log.at(k) <= k."""
  r = verify_src(FIFO_SPEC, depth=8)
  assert r["result"] == "verified"


def test_seq_equality_requires():
  """§8.6: q.push(1) == q2 style requires works."""
  src = """
spec SeqEq {
  type Id = 0..1
  state { q: Seq<Id, 2>, q2: Seq<Id, 2> }
  init { q = Seq {}  q2 = Seq {} }
  action setup() {
    requires q.size() < 2 and q2.size() < 2
    q = q.push(1)
    q2 = q2.push(1)
  }
  action check() {
    requires q == q2
  }
  action mismatch() {
    requires q.size() < 2
    q = q.push(0)
    requires q == q2
  }
}
"""
  r = verify_src(src, depth=4)
  assert r["result"] == "verified"


def test_contains():
  """§8.7: contains membership."""
  src = """
spec SeqContains {
  type Id = 0..2
  state { q: Seq<Id, 3> }
  init { q = Seq { 1, 2 } }
  reachable HasOne { q.contains(1) }
  reachable NoZero { not q.contains(0) }
  action noop() { }
}
"""
  r = verify_src(src, depth=2)
  assert r["result"] == "verified"
  assert r["reachables"]["HasOne"]["witness"][-1]["state"]["q"] == [1, 2]


def test_json_display_array_no_internals():
  """§8.8: state as array; no __data/__len in JSON."""
  src = """
spec SeqJson {
  type Id = 0..1
  state { q: Seq<Id, 2> }
  init { q = Seq {} }
  action grow() {
    requires q.size() == 0
    q = q.push(0).push(1)
  }
  reachable Full { q.size() == 2 }
}
"""
  r = verify_src(src, depth=3)
  blob = json.dumps(r)
  assert "__data" not in blob
  assert "__len" not in blob
  w = r["reachables"]["Full"]["witness"][-1]
  assert w["state"]["q"] == [0, 1]
  changes = [e for e in r["reachables"]["Full"]["witness"] if "changes" in e]
  assert changes
  assert "queue" in changes[-1]["changes"] or "q" in changes[-1]["changes"]


def test_check_rejects_illegal_types():
  """§8.9: struct Seq field, Map<K,Seq>, Map<K,Set<K>>, literal > N."""
  struct_seq = """
spec BadStructSeq {
  type K = 0..1
  struct S { items: Seq<K, 2> }
  state { s: S }
  init { s.items = Seq {} }
  action noop() { }
}
"""
  out = check_src(struct_seq)
  assert out["result"] == "error"
  assert out["kind"] == "type"

  map_seq = """
spec BadMapSeq {
  type K = 0..1
  state { m: Map<K, Seq<K, 2>> }
  init { m[0] = Seq {} }
  action noop() { }
}
"""
  out = check_src(map_seq)
  assert out["result"] == "error"
  assert out["kind"] == "type"

  map_set = """
spec BadMapSet {
  type K = 0..1
  state { m: Map<K, Set<K>> }
  init { }
  action noop() { }
}
"""
  out = check_src(map_set)
  assert out["result"] == "error"
  assert out["kind"] == "type"

  overflow = """
spec BadLiteral {
  type K = 0..1
  state { q: Seq<K, 2> }
  init { q = Seq { 0, 1, 2 } }
  action noop() { }
}
"""
  out = check_src(overflow)
  assert out["result"] == "error"
  assert out["kind"] == "type"
  assert "capacity" in out["message"].lower() or "elements" in out["message"].lower()


def test_induction_fifo_proved():
  """§8.10: FIFO spec proved by induction (_bounds_q in premises)."""
  r = prove(build_spec(parse(FIFO_SPEC)), k_ind=1, base_depth=8)
  assert r["result"] == "proved"
  assert r["engine"] == "induction"
  assert "_bounds_queue" in r["invariants_checked"]


def test_scenarios_fifo_cover_and_reach():
  """§8.11: cover_* and reach_* scenarios for FIFO spec."""
  r = scenarios(build_spec(parse(FIFO_SPEC)), depth=6)
  assert r["result"] == "scenarios"
  names = {s["name"] for s in r["scenarios"]}
  assert "reach_HasTwo" in names
  assert "cover_enqueue" in names
  assert "cover_dequeue" in names
