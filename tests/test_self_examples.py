"""examples/self — verify fslc's own design contracts in FSL."""
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from fslc.cli import run_check, run_verify


ROOT = Path(__file__).resolve().parents[1]
E = ROOT / "examples/self"


# All three verify cleanly under --deadlock warn: intended terminal states are
# declared with a terminal { } block (fslc_session / fslc_monitor) or the spec
# has an always-enabled action (refinement_algebra). No --deadlock ignore needed.
CASES = [
    ("fslc_session.fsl", "warn"),
    ("fslc_monitor.fsl", "warn"),
    ("refinement_algebra.fsl", "warn"),
]


def test_terminal_block_suppresses_intended_deadlock_warnings():
    """Thanks to terminal { }, intended terminal states do not raise deadlock warnings."""
    for filename in ("fslc_session.fsl", "fslc_monitor.fsl"):
        out = run_verify(str(E / filename), depth=8, deadlock_mode="warn")
        assert out["result"] == "verified"
        dl = [w for w in out.get("warnings", []) if "deadlock" in w.get("message", "")]
        assert dl == [], (filename, dl)


@pytest.mark.parametrize(("filename", "deadlock_mode"), CASES)
def test_self_example_check_verify_and_induction(filename, deadlock_mode):
    path = str(E / filename)

    assert run_check(path)["result"] == "ok"
    assert run_verify(path, depth=8, deadlock_mode=deadlock_mode)["result"] == "verified"
    assert run_verify(
        path, depth=8, deadlock_mode=deadlock_mode, engine="induction"
    )["result"] == "proved"


def _run_fslc_verify(*args):
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT) + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [sys.executable, "-m", "fslc", "verify", *args],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def test_verify_property_selects_single_invariant():
    proc = _run_fslc_verify(
        str(E / "refinement_algebra.fsl"),
        "--depth",
        "8",
        "--property",
        "SafetyPropagates",
    )
    assert proc.returncode == 0, proc.stderr
    out = json.loads(proc.stdout)
    assert out["result"] == "verified"
    assert out["invariants_checked"] == ["SafetyPropagates"]


def test_verify_property_missing_invariant_is_usage_error():
    proc = _run_fslc_verify(
        str(E / "refinement_algebra.fsl"),
        "--depth",
        "8",
        "--property",
        "NoSuchInv",
    )
    assert proc.returncode == 2
    out = json.loads(proc.stdout)
    assert out["result"] == "error"
    assert out["kind"] == "usage"
    assert out["message"].startswith("no such property: NoSuchInv")


def test_verify_property_targets_a_trans():
    """--property resolves a `trans` declaration, not just invariants."""
    proc = _run_fslc_verify(
        str(E / "fslc_monitor.fsl"), "--depth", "8", "--property", "RejectIsSticky",
    )
    assert proc.returncode == 0, proc.stderr
    out = json.loads(proc.stdout)
    assert out["result"] == "verified"
    assert out["transitions_checked"] == ["RejectIsSticky"]
    # the other property kinds are not checked when one trans is selected
    assert out["invariants_checked"] == []


def test_induction_property_on_non_invariant_explains_engine_limit():
    """`--engine induction --property <trans>` gives a clear engine-scope error."""
    proc = _run_fslc_verify(
        str(E / "fslc_monitor.fsl"), "--engine", "induction",
        "--property", "RejectIsSticky",
    )
    assert proc.returncode == 2
    out = json.loads(proc.stdout)
    assert out["result"] == "error"
    assert out["kind"] == "usage"
    assert "is a trans" in out["message"]
    assert "induction engine cannot prove" in out["message"]


def test_verify_property_targets_a_reachable():
    """--property resolves a `reachable` declaration in isolation."""
    proc = _run_fslc_verify(
        str(E / "fslc_monitor.fsl"), "--depth", "8", "--property", "ReachConformant",
    )
    assert proc.returncode == 0, proc.stderr
    out = json.loads(proc.stdout)
    assert out["result"] == "verified"
    assert list(out["reachables"]) == ["ReachConformant"]
    assert out["invariants_checked"] == []
    assert out["transitions_checked"] == []
