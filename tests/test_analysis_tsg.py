import json
import subprocess
import sys

from fslc.cli import run_analyze


def _write(tmp_path, src, name="case.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def _ids(out, kind=None):
    return {
        n["id"]
        for n in out.get("nodes", [])
        if kind is None or n["kind"] == kind
    }


def _edge_kinds(out):
    return {e["kind"] for e in out.get("edges", [])}


def test_analyze_tsg_kernel_spec_contains_core_nodes_and_edges(tmp_path):
    path = _write(tmp_path, """
spec AnalysisKernel {
  state { x: Int, done: Bool }
  init { x = 0  done = false }
  action step() "REQ-STEP: step moves state" {
    requires not done
    x = x + 1
    done = true
    ensures done == true
  }
  invariant Safe "REQ-SAFE: done implies progress" { done => x >= 1 }
  trans Monotone "REQ-MONO: x never decreases" { x >= old(x) }
  reachable Done "REQ-DONE: done can be reached" { done }
  leadsTo EventuallyDone "REQ-LIVE: work completes" { not done ~> done }
}
""")

    out = run_analyze(str(path), projection="tsg")

    assert out["result"] == "analyzed"
    assert out["projection"] == "tsg"
    assert out["schema_version"] == "tsg.v0"
    assert {
        "state:x",
        "state:done",
        "action:step",
        "guard:step:0",
        "effect:step:0",
        "effect:step:1",
        "ensures:step:0",
        "invariant:Safe",
        "trans:Monotone",
        "reachable:Done",
        "leadsTo:EventuallyDone",
        "requirement:REQ-STEP",
        "requirement:REQ-SAFE",
    }.issubset(_ids(out))
    assert {"declares", "covers", "has_guard", "has_effect", "has_ensures", "reads", "writes", "checks"}.issubset(_edge_kinds(out))
    assert any(e["from"] == "requirement:REQ-STEP" and e["to"] == "action:step" for e in out["edges"])


def test_analyze_tsg_requirements_metadata_and_scenarios(tmp_path):
    path = _write(tmp_path, """
requirements AnalysisRequirements {
  type K = 0..0
  state { x: Bool }
  init { x = false }
  requirement REQ-1 "go works" {
    action go(k: K) {
      requires not x
      x = true
    }
    invariant Done { x => true }
  }
  acceptance REQ-1 "go succeeds" {
    go(0)
    expect x == true
  }
  forbidden FB-1 "go cannot repeat" {
    go(0)
    go(0)
    expect rejected
  }
}
""")

    out = run_analyze(str(path), projection="tsg")

    assert out["result"] == "analyzed"
    assert {"requirement:REQ-1", "action:go", "invariant:Done", "acceptance:REQ-1", "forbidden:FB-1"}.issubset(_ids(out))
    assert any(e["kind"] == "covers" and e["from"] == "requirement:REQ-1" and e["to"] == "acceptance:REQ-1" for e in out["edges"])
    assert any(e["kind"] in {"starts_with", "precedes"} and e["from"] == "acceptance:REQ-1" and e["to"] == "action:go" for e in out["edges"])


def test_analyze_tsg_is_deterministic(tmp_path):
    path = _write(tmp_path, """
spec DeterministicAnalysis {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
""")

    assert run_analyze(str(path), projection="tsg") == run_analyze(str(path), projection="tsg")


def test_analyze_invalid_input_uses_error_envelope(tmp_path):
    path = _write(tmp_path, "spec Broken { state { x: Int } init { x = } }")

    out = run_analyze(str(path), projection="tsg")

    assert out["result"] == "error"
    assert out["kind"] == "parse"
    assert "loc" in out


def test_analyze_cli_exits_zero_for_analyzed(tmp_path):
    path = _write(tmp_path, """
spec AnalyzeCli {
  state { x: Int }
  init { x = 0 }
  action idle() { x = x }
  invariant Any "MODEL: baseline" { true }
}
""")

    proc = subprocess.run(
        [sys.executable, "-m", "fslc", "analyze", str(path), "--projection", "tsg"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert json.loads(proc.stdout)["result"] == "analyzed"
