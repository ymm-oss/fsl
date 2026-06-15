"""examples/self — fslc 自身の設計契約を FSL で検証する。"""
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
    """terminal { } のおかげで意図した終端状態が deadlock 警告にならない。"""
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
    assert out["message"].startswith("no such invariant: NoSuchInv")
