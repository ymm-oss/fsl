"""examples/self — fslc 自身の設計契約を FSL で検証する。"""
from pathlib import Path

import pytest

from fslc.cli import run_check, run_verify


ROOT = Path(__file__).resolve().parents[1]
E = ROOT / "examples/self"


CASES = [
    ("fslc_session.fsl", "ignore"),
    ("fslc_monitor.fsl", "ignore"),
    ("refinement_algebra.fsl", "warn"),
]


@pytest.mark.parametrize(("filename", "deadlock_mode"), CASES)
def test_self_example_check_verify_and_induction(filename, deadlock_mode):
    path = str(E / filename)

    assert run_check(path)["result"] == "ok"
    assert run_verify(path, depth=8, deadlock_mode=deadlock_mode)["result"] == "verified"
    assert run_verify(
        path, depth=8, deadlock_mode=deadlock_mode, engine="induction"
    )["result"] == "proved"
