import json
import os
import sys
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "fixtures" / "chain"


def _run_chain(manifest, *extra):
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT / "src") + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [sys.executable, "-m", "fslc", "chain", str(manifest), *extra],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def test_chain_manifest_all_passes_reports_table_and_json():
    result = _run_chain(FIXTURE / "fsl-project.toml")

    assert result.returncode == 0
    assert "Layer" in result.stderr
    assert "business" in result.stderr
    assert "design->requirements" in result.stderr
    assert "impl" in result.stderr

    out = json.loads(result.stdout)
    assert out["result"] == "verified"
    assert [layer["layer"] for layer in out["layers"]] == [
        "business",
        "requirements",
        "design",
        "design->requirements",
        "impl",
    ]
    assert [layer["status"] for layer in out["layers"]] == [
        "passed",
        "passed",
        "passed",
        "passed",
        "passed",
    ]
    assert [layer["result"] for layer in out["layers"]] == [
        "verified",
        "ok",
        "verified",
        "refines",
        "passed",
    ]


def test_chain_short_circuits_after_first_failure():
    result = _run_chain(FIXTURE / "fsl-project-broken.toml")

    assert result.returncode == 1
    out = json.loads(result.stdout)
    assert out["result"] == "violated"
    assert out["failed"] == ["business"]
    assert [layer["status"] for layer in out["layers"]] == [
        "failed",
        "skipped",
        "skipped",
        "skipped",
        "skipped",
    ]
    assert "skipped" in result.stderr


def test_chain_treats_nested_implements_failure_as_layer_failure():
    result = _run_chain(FIXTURE / "fsl-project-broken-implements.toml")

    assert result.returncode == 1
    out = json.loads(result.stdout)
    assert out["result"] == "violated"
    assert out["failed"] == ["requirements"]
    assert out["layers"][0]["status"] == "passed"
    assert out["layers"][1]["status"] == "failed"
    assert out["layers"][1]["detail"]["result"] == "ok"
    assert out["layers"][1]["detail"]["implements"]["result"] == "refinement_failed"
    assert [layer["status"] for layer in out["layers"][2:]] == [
        "skipped",
        "skipped",
        "skipped",
    ]


def test_chain_keep_going_runs_later_layers_after_failure():
    result = _run_chain(FIXTURE / "fsl-project-broken.toml", "--keep-going")

    assert result.returncode == 1
    out = json.loads(result.stdout)
    assert out["result"] == "violated"
    assert out["keep_going"] is True
    assert out["failed"] == ["business"]
    assert [layer["status"] for layer in out["layers"]] == [
        "failed",
        "passed",
        "passed",
        "passed",
        "passed",
    ]
