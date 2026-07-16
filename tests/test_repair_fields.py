# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Issue #23: the `trace_type` repair-routing discriminator + requirement parity.

`trace_type` is added by the CLI envelope (cli._envelope), so these go through
the `run_*` wrappers, not the core `verify()`/`refine()` functions.
"""
from pathlib import Path

from fslc.cli import run_check, run_verify, run_refine


ROOT = Path(__file__).resolve().parents[1]
NFR = ROOT / "examples" / "nfr"
INJECTED = ROOT / "examples" / "gallery" / "injected"


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src, encoding="utf-8")
    return p


def test_trace_type_sla_violation(tmp_path):
    src = (NFR / "sla_worker.fsl").read_text(encoding="utf-8").replace(
        "    urgent start, finish\n", "")
    p = _write(tmp_path, "sla_no_urgent.fsl", src)
    r = run_verify(str(p), 10, "ignore")
    assert r["result"] == "violated"
    # an SLA deadline is distinguished from a structural invariant
    assert r["trace_type"] == "sla"
    # backward-compat: existing fields untouched
    assert r["requirement"] == {"id": "NFR-1", "text": "submitted requests finish within 4 ticks"}
    assert r["violation_kind"] == "invariant"
    assert r["trace"]


def test_trace_type_plain_invariant(tmp_path):
    p = _write(tmp_path, "inv.fsl", """
spec PlainInv {
  state { x: Int }
  init { x = 0 }
  action bump() { x = 1 }
  invariant StaysZero { x == 0 }
}
""")
    r = run_verify(str(p), 2, "warn")
    assert r["result"] == "violated"
    assert r["trace_type"] == "invariant"


def test_trace_type_reachable_failed(tmp_path):
    p = _write(tmp_path, "unreach.fsl", """
spec Unreach {
  state { x: Bool }
  init { x = false }
  action noop() { requires x  x = true }
  reachable XTrue { x }
}
""")
    r = run_verify(str(p), 5, "warn")
    assert r["result"] == "reachable_failed"
    assert r["trace_type"] == "reachable"


def test_trace_type_refinement_failed_and_requirement_hoisted(tmp_path):
    impl = _write(tmp_path, "impl.fsl", """
spec ImplR {
  type N = 0..2
  state { x: N }
  init { x = 0 }
  action bump() "REQ-9: bump advances x" { requires x < 2  x = x + 1 }
}
""")
    abs_ = _write(tmp_path, "abs.fsl", """
spec AbsR {
  type N = 0..2
  state { x: N }
  init { x = 0 }
  action bump() { requires x == 0  x = 1 }
}
""")
    mapping = _write(tmp_path, "m.fsl", """
refinement M { impl ImplR  abs AbsR
  map x = x
  action bump() -> bump()
}
""")
    r = run_refine(str(impl), str(abs_), str(mapping), depth=4)
    assert r["result"] == "refinement_failed"
    assert r["trace_type"] == "refinement"
    # requirement is hoisted to the root (parity with verify violations)
    assert r["requirement"] == {"id": "REQ-9", "text": "bump advances x"}


def test_trace_type_acceptance_channel():
    # A boundary flip requires the independent acceptance lane.
    r = run_check(str(INJECTED / "return_system__boundary_flip.fsl"))
    assert r["result"] == "error"
    assert r["trace_type"] == "acceptance"


def test_trace_type_forbidden_channel():
    # Guard weakening requires the independent forbidden lane.
    r = run_check(str(INJECTED / "return_system__guard_weakening.fsl"))
    assert r["result"] == "error"
    assert r["trace_type"] == "forbidden"


def test_no_trace_type_on_passing_results():
    # a clean verify has no counterexample → no trace_type
    r = run_verify(str(NFR / "sla_worker.fsl"), 8, "ignore")
    assert r["result"] == "verified"
    assert "trace_type" not in r


def test_no_trace_type_on_spec_error(tmp_path):
    p = _write(tmp_path, "bad.fsl", "spec X { state { a: Bogus } init {} }")
    r = run_check(str(p))
    assert r["result"] == "error"
    # parse/type/semantics errors are not counterexamples
    assert "trace_type" not in r
