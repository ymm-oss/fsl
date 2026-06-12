from __future__ import annotations

import re
import textwrap
from pathlib import Path

from fslc.cli import run_verify
from oracle import ROOT


COUNTER = ROOT / "examples" / "gallery" / "valid" / "tiny_bounded_counter.fsl"
VIOLATED_COUNTER = ROOT / "examples" / "gallery" / "errors" / "violated_invariant_counter.fsl"


def _write(tmp_path, name, src):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_guard_removal_from_proved_counter_exposes_type_bound(tmp_path):
    original = run_verify(str(COUNTER), depth=4, deadlock_mode="ignore", engine="induction")
    assert original["result"] == "proved"

    src = COUNTER.read_text(encoding="utf-8")
    mutated = src.replace("    requires n < 3\n", "")
    path = _write(tmp_path, "counter_guard_removed.fsl", mutated)

    result = run_verify(str(path), depth=5, deadlock_mode="ignore")
    assert result["result"] == "violated", result
    assert result["violation_kind"] == "type_bound"


def test_semantics_preserving_renames_and_reordering_keep_verdict(tmp_path):
    base = textwrap.dedent(
        """
        spec RenameStable {
          type Count = 0..2
          state { n: Count }
          init { n = 0 }
          action inc() { requires n < 2  n = n + 1 }
          action dec() { requires n > 0  n = n - 1 }
          invariant Inside { n >= 0 and n <= 2 }
          reachable Top { n == 2 }
        }
        """
    )
    renamed = textwrap.dedent(
        """
        spec RenameStable {
          type Count = 0..2
          state { value: Count }
          init { value = 0 }
          reachable RenamedTop { value == 2 }
          invariant RenamedInside { value >= 0 and value <= 2 }
          action down() { requires value > 0  value = value - 1 }
          action up() { requires value < 2  value = value + 1 }
        }
        """
    )
    base_result = run_verify(str(_write(tmp_path, "base.fsl", base)), depth=4, deadlock_mode="ignore")
    renamed_result = run_verify(str(_write(tmp_path, "renamed.fsl", renamed)), depth=4, deadlock_mode="ignore")

    assert base_result["result"] == "verified"
    assert renamed_result["result"] == base_result["result"]
    assert {v["witnessed_at_step"] for v in renamed_result["reachables"].values()} == {
        v["witnessed_at_step"] for v in base_result["reachables"].values()
    }


def test_depth_monotonicity_for_safety_only_verified_spec(tmp_path):
    src = textwrap.dedent(
        """
        spec SafetyOnly {
          type N = 0..2
          state { n: N }
          init { n = 0 }
          action inc() { requires n < 2  n = n + 1 }
          action dec() { requires n > 0  n = n - 1 }
          invariant InBounds { n >= 0 and n <= 2 }
        }
        """
    )
    path = _write(tmp_path, "safety_only.fsl", src)
    assert run_verify(str(path), depth=4, deadlock_mode="ignore")["result"] == "verified"
    for depth in range(0, 4):
        assert run_verify(str(path), depth=depth, deadlock_mode="ignore")["result"] == "verified"


def test_minimum_violation_depth_is_stable_when_bound_increases():
    depths = []
    for depth in range(1, 5):
        result = run_verify(str(VIOLATED_COUNTER), depth=depth, deadlock_mode="ignore")
        assert result["result"] == "violated"
        assert result["violation_kind"] == "invariant"
        depths.append(result["violated_at_step"])
    assert depths == [1, 1, 1, 1]


def test_renaming_invariant_and_reachable_labels_does_not_change_verdict(tmp_path):
    src = COUNTER.read_text(encoding="utf-8")
    renamed = re.sub(r"invariant NeverAboveCap", "invariant RenamedCap", src)
    renamed = re.sub(r"reachable HitCap", "reachable RenamedHitCap", renamed)
    result = run_verify(str(_write(tmp_path, "renamed_labels.fsl", renamed)), depth=4, deadlock_mode="ignore")
    assert result["result"] == "verified"
    assert set(result["invariants_checked"]) == {"_bounds_n", "RenamedCap"}
    assert set(result["reachables"]) == {"RenamedHitCap"}
