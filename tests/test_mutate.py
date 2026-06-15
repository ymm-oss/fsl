import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import run_mutate
from fslc.mutate import mutate_file


ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
EXAMPLES = ROOT / "examples"


def _write(tmp_path, text):
    path = tmp_path / "case.fsl"
    path.write_text(text, encoding="utf-8")
    return path


def _mutant(out, op, target):
    for mutant in out["mutants"]:
        if mutant["op"] == op and mutant["target"] == target:
            return mutant
    raise AssertionError(f"missing mutant {op} {target}")


def test_cart_checkout_stock_guard_removal_killed_by_bounds_stock():
    out = mutate_file(str(SPECS / "cart_v1.fsl"), depth=4, max_mutants=1000)
    mutant = _mutant(out, "requires_remove", "checkout requires #2")
    assert mutant["status"] == "killed"
    assert mutant["killed_by"] == "_bounds_stock"


def test_type_bound_plus_one_rebuilds_implicit_bounds(tmp_path):
    path = _write(tmp_path, """
spec BoundRebuild {
  type Count = 0..1
  state { x: Count }
  init { x = 0 }
  action set(v: Count) {
    x = v
  }
  invariant Tight "REQ-TIGHT: x stays below expanded bound" { x <= 1 }
}
""")
    out = mutate_file(str(path), depth=2, max_mutants=1000)
    mutant = _mutant(out, "type_bound_hi_plus1", "type Count hi")
    assert mutant["status"] == "killed"
    assert mutant["killed_by"] == "Tight"


def test_thinned_invariant_spec_reports_behavior_survivor_loc(tmp_path):
    path = _write(tmp_path, """
spec Thin {
  type Count = 0..3
  state { x: Count }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant Loose { x <= 3 }
}
""")
    out = mutate_file(str(path), depth=3, max_mutants=1000)
    mutant = _mutant(out, "requires_remove", "inc requires #1")
    assert mutant["status"] == "survived"
    assert mutant["loc"] == {"line": 7, "column": 5}


def test_by_requirement_empty_formalization_warning(tmp_path):
    path = _write(tmp_path, """
requirements ReqStress {
  type Count = 0..4
  state { x: Count }
  init { x = 0 }
  requirement REQ-1 "guards the increment" {
    action inc() {
      requires x < 3
      x = x + 1
    }
    invariant Max { x <= 3 }
  }
  requirement REQ-2 "empty formalization" {
    invariant Trivial { x >= 0 }
  }
}
""")
    out = mutate_file(str(path), depth=4, by_requirement=True, max_mutants=1000)
    assert out["by_requirement"]["REQ-1"]["kills"] > 0
    assert out["by_requirement"]["REQ-2"] == {
        "kills": 0,
        "warning": "empty_formalization",
    }
    assert any("observed lower bound" in note for note in out["notes"])


def test_baseline_violated_spec_is_refused(tmp_path):
    path = _write(tmp_path, """
spec Broken {
  type Count = 0..2
  state { x: Count }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant TooLow { x == 0 }
}
""")
    out = mutate_file(str(path), depth=1, max_mutants=1000)
    assert out["result"] == "violated"
    assert "mutants" not in out


def test_coverage_false_survivor_annotated(tmp_path):
    path = _write(tmp_path, """
spec DeadAction {
  type Count = 0..2
  state { x: Count }
  init { x = 0 }
  action dead() {
    requires x > 2
    x = x + 1
  }
  invariant NonNegative { x >= 0 }
}
""")
    out = mutate_file(str(path), depth=2, max_mutants=1000)
    mutant = _mutant(out, "assignment_remove", "dead assignment")
    assert mutant["status"] == "survived"
    assert mutant["note"] == "action dead at baseline — survival expected"


def test_max_mutants_truncation_noted():
    out = mutate_file(str(SPECS / "cart_v1.fsl"), depth=4, max_mutants=3)
    assert out["summary"]["total"] == 3
    assert any(note.startswith("mutant cap 3 reached:") for note in out["notes"])


def test_mutate_cli_exit_zero_for_baseline_refusal(tmp_path):
    path = _write(tmp_path, """
spec Broken {
  type Count = 0..1
  state { x: Count }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant TooLow { x == 0 }
}
""")
    proc = subprocess.run(
        [sys.executable, "-m", "fslc.cli", "mutate", str(path), "--depth", "1"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.returncode == 0, proc.stderr
    assert json.loads(proc.stdout)["result"] == "violated"


def test_corpus_stability_no_crash_or_serialization_error():
    paths = sorted(SPECS.glob("*.fsl")) + sorted(EXAMPLES.rglob("*.fsl"))
    skipped = {"parse_", "error_", "violated_", "refinement_failed_"}
    for path in paths:
        if any(path.name.startswith(prefix) for prefix in skipped):
            continue
        # the intentionally-flawed injected corpus is handled by the dedicated
        # test_injection_bench.py (running mutate again here is redundant and slow).
        if "gallery/injected" in path.as_posix():
            continue
        out = run_mutate(str(path), depth=2, max_mutants=5)
        json.dumps(out)
        assert out["result"] in {
            "mutated",
            "verified",
            "violated",
            "reachable_failed",
            "refinement_failed",
            "error",
        }, path
