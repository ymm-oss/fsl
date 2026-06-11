import json
import subprocess
import sys

from fslc import build_spec, parse, prove, scenarios, verify


def _spec(src):
    return build_spec(parse(src))


def test_tagged_invariant_violation_has_requirement():
    src = """
spec MetaInv {
  state { x: Int }
  init { x = 0 }
  action bump() { x = 1 }
  invariant StaysZero "REQ-1: x remains zero" { x == 0 }
}
"""
    r = verify(_spec(src), 1)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "invariant"
    assert r["requirement"] == {"id": "REQ-1", "text": "x remains zero"}


def test_tagged_action_coverage_false_has_requirement():
    src = """
spec MetaCoverage {
  state { x: Int }
  init { x = 0 }
  action blocked() "REQ-2: blocked action must be reachable" {
    requires x == 1
    requires x == 2
    x = 3
  }
  action idle() { x = 0 }
  invariant I { true }
}
"""
    r = verify(_spec(src), 2, source_lines=src.splitlines())
    assert r["result"] == "verified"
    assert r["action_coverage"]["blocked"]["covered"] is False
    assert r["action_coverage"]["blocked"]["requirement"] == {
        "id": "REQ-2",
        "text": "blocked action must be reachable",
    }


def test_unknown_cti_has_requirement():
    src = """
spec MetaCti {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() {
    requires x < 4
    x = x + 1
    y = y + 1
  }
  invariant UpperBound "REQ-3: y remains within the proven bound" { y <= 4 }
}
"""
    r = prove(_spec(src), k_ind=1, base_depth=4, deadlock_mode="ignore")
    assert r["result"] == "unknown_cti"
    assert r["requirement"] == {
        "id": "REQ-3",
        "text": "y remains within the proven bound",
    }


def test_scenarios_requirements_for_reach_respond_and_cover():
    src = """
spec MetaScenarios {
  state { x: Int }
  init { x = 0 }
  action go() "REQ-A: go is coverable" {
    requires x == 0
    x = 1
  }
  action stay() {
    requires x == 1
    x = 1
  }
  invariant I { true }
  reachable Done "REQ-R: done can be reached" { x == 1 }
  leadsTo EventuallyDone "REQ-L: initial work completes" { x == 0 ~> x == 1 }
}
"""
    r = scenarios(_spec(src), 2)
    assert r["result"] == "scenarios"
    by_name = {s["name"]: s for s in r["scenarios"]}
    assert by_name["reach_Done"]["requirement"] == {
        "id": "REQ-R",
        "text": "done can be reached",
    }
    assert by_name["respond_EventuallyDone"]["requirement"] == {
        "id": "REQ-L",
        "text": "initial work completes",
    }
    assert by_name["cover_go"]["requirement"] == {
        "id": "REQ-A",
        "text": "go is coverable",
    }


def test_tag_without_colon_has_null_text():
    src = """
spec MetaNoColon {
  state { x: Int }
  init { x = 0 }
  action idle() { x = 0 }
  invariant Impossible "REQ-NOCOLON" { false }
}
"""
    r = verify(_spec(src), 0)
    assert r["result"] == "violated"
    assert r["requirement"] == {"id": "REQ-NOCOLON", "text": None}


def test_check_accepts_tag_syntax(tmp_path):
    src = """
spec MetaCheck {
  state { x: Int }
  init { x = 0 }
  fair action tick() "REQ-A: tagged fair action" { x = x }
  invariant I "REQ-I: tagged invariant" { true }
  reachable R "REQ-R: tagged reachable" { x == 0 }
  leadsTo L "REQ-L: tagged leadsto" { x == 0 ~> x == 0 }
}
"""
    path = tmp_path / "meta_check.fsl"
    path.write_text(src, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, "-m", "fslc", "check", str(path)],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert json.loads(proc.stdout)["result"] == "ok"
