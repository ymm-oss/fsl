# SPDX-License-Identifier: Apache-2.0

import json
import subprocess
from pathlib import Path

import pytest

from oracle import BoundedPropertyOracle, bounded_liveness_oracle


ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust" / "target" / "debug" / "fslc"
SPEC = ROOT / "examples" / "nfr" / "bounded_response.fsl"


@pytest.fixture(scope="module", autouse=True)
def build_native_cli():
    subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "fslc-rust"],
        cwd=ROOT / "rust",
        check=True,
    )


@pytest.mark.parametrize(
    "fixture",
    ["bounded_response.within.v1.json", "bounded_response.overdue.v1.json"],
)
def test_native_bounded_liveness_matches_the_z3_free_oracle(fixture: str):
    trace_path = ROOT / "examples" / "nfr" / fixture
    trace = json.loads(trace_path.read_text(encoding="utf-8"))
    states = [trace["initial"], *(event["state"] for event in trace["events"])]
    oracle = bounded_liveness_oracle(
        states,
        [
            BoundedPropertyOracle(
                name="RespondsInTwo",
                before=lambda state: state["stage"] == 1,
                after=lambda state: state["stage"] == 3,
                within=2,
            )
        ],
    )
    completed = subprocess.run(
        [str(RUST), "replay", str(SPEC), "--trace", str(trace_path)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    native = json.loads(completed.stdout)

    if oracle["status"] == "violated":
        assert completed.returncode == 1
        assert native["violation"]["check"] == "bounded_liveness"
        for key in ["property", "pending_since", "deadline", "within", "tick"]:
            assert native["violation"][key] == oracle[key]
    else:
        assert completed.returncode == 0
        assert native["checks"]["bounded_liveness"]["status"] == oracle["status"]
