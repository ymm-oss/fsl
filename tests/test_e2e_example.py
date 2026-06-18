import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from fslc.cli import run_refine, run_scenarios, run_verify


ROOT = Path(__file__).resolve().parents[1]
E2E = ROOT / "examples" / "e2e"


def _run(args, cwd=ROOT, check=True):
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT) + os.pathsep + env.get("PYTHONPATH", "")
    result = subprocess.run(
        args,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if check and result.returncode != 0:
        raise AssertionError(
            f"command failed: {' '.join(map(str, args))}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def test_e2e_chain_verifies_refines_and_implementation_passes():
    business = run_verify(str(E2E / "1_business.fsl"), 8, "ignore", engine="induction")
    assert business["result"] == "proved"
    assert "CTRL-1" in business["leads_to"]
    assert "CTRL-2" in business["leads_to"]

    requirements = run_verify(str(E2E / "2_requirements.fsl"), 8, "ignore")
    assert requirements["result"] == "verified"
    assert requirements["implements"]["result"] == "refines"

    requirements_proved = run_verify(
        str(E2E / "2_requirements.fsl"), 8, "ignore", engine="induction"
    )
    assert requirements_proved["result"] == "proved"
    assert requirements_proved["implements"]["result"] == "refines"

    scenarios = run_scenarios(str(E2E / "2_requirements.fsl"), 8)
    acceptance_names = {
        scenario["name"]
        for scenario in scenarios["scenarios"]
        if scenario["kind"] == "acceptance"
    }
    assert {"acceptance_AC-1", "acceptance_AC-2"} <= acceptance_names

    design = run_verify(str(E2E / "3_design.fsl"), 8, "ignore", engine="induction")
    assert design["result"] == "proved"
    assert "CountsMatchSubmittedOrPaid" in design["invariants_checked"]

    design_refines = run_refine(
        str(E2E / "3_design.fsl"),
        str(E2E / "2_requirements.fsl"),
        str(E2E / "3_refines_2.fsl"),
        depth=8,
    )
    assert design_refines["result"] == "refines"

    impl = _run([sys.executable, "-m", "pytest", "-q"], cwd=E2E / "impl")
    assert "10 passed" in impl.stdout


def test_e2e_readme_commands_and_break_demo_are_current():
    readme = (E2E / "README.md").read_text(encoding="utf-8")
    documented_commands = [
        "./.venv/bin/python -m fslc verify examples/e2e/1_business.fsl --engine induction --deadlock ignore",
        "./.venv/bin/python -m fslc verify examples/e2e/2_requirements.fsl --deadlock ignore",
        "./.venv/bin/python -m fslc verify examples/e2e/2_requirements.fsl --engine induction --deadlock ignore",
        "./.venv/bin/python -m fslc scenarios examples/e2e/2_requirements.fsl --deadlock ignore",
        "./.venv/bin/python -m fslc verify examples/e2e/3_design.fsl --engine induction --deadlock ignore",
        "./.venv/bin/python -m fslc refine examples/e2e/3_design.fsl examples/e2e/2_requirements.fsl examples/e2e/3_refines_2.fsl --depth 8",
        "./.venv/bin/python -m fslc testgen examples/e2e/3_design.fsl -o examples/e2e/impl/test_conformance.py",
        "(cd examples/e2e/impl && ../../../.venv/bin/python -m pytest -q)",
        "./.venv/bin/python -m pytest tests/ -q",
    ]
    for command in documented_commands:
        assert command in readme

    command_results = [
        _run([sys.executable, "-m", "fslc", "verify", str(E2E / "1_business.fsl"), "--engine", "induction", "--deadlock", "ignore"]),
        _run([sys.executable, "-m", "fslc", "verify", str(E2E / "2_requirements.fsl"), "--deadlock", "ignore"]),
        _run([sys.executable, "-m", "fslc", "verify", str(E2E / "2_requirements.fsl"), "--engine", "induction", "--deadlock", "ignore"]),
        _run([sys.executable, "-m", "fslc", "scenarios", str(E2E / "2_requirements.fsl"), "--deadlock", "ignore"]),
        _run([sys.executable, "-m", "fslc", "verify", str(E2E / "3_design.fsl"), "--engine", "induction", "--deadlock", "ignore"]),
        _run([
            sys.executable,
            "-m",
            "fslc",
            "refine",
            str(E2E / "3_design.fsl"),
            str(E2E / "2_requirements.fsl"),
            str(E2E / "3_refines_2.fsl"),
            "--depth",
            "8",
        ]),
        _run([sys.executable, "-m", "pytest", "-q"], cwd=E2E / "impl"),
    ]
    parsed = [json.loads(result.stdout) for result in command_results[:-1]]
    assert [item["result"] for item in parsed] == [
        "proved",
        "verified",
        "proved",
        "scenarios",
        "proved",
        "refines",
    ]
    assert "10 passed" in command_results[-1].stdout

    _tmp = Path(tempfile.gettempdir())
    broken_design = _tmp / "3_design_shortcut_test.fsl"
    broken_mapping = _tmp / "3_refines_shortcut_test.fsl"
    broken_design.write_text((E2E / "3_design.fsl").read_text(encoding="utf-8"), encoding="utf-8")
    broken_mapping.write_text((E2E / "3_refines_2.fsl").read_text(encoding="utf-8"), encoding="utf-8")
    design_src = broken_design.read_text(encoding="utf-8")
    broken_design.write_text(
        design_src.replace(
            "\n  fair action pay_submit(c: Claim) {",
            "\n  fair action pay_without_approval(c: Claim) {\n"
            "    requires design[c].st == DesignDraft\n"
            "    requires outbox.size() < OUTBOX_CAP\n"
            "    design[c].st = DesignPaymentSubmitted\n"
            "    paid_count = paid_count + 1\n"
            "    outbox = outbox.push(c)\n"
            "  }\n\n"
            "  fair action pay_submit(c: Claim) {",
        ),
        encoding="utf-8",
    )
    mapping_src = broken_mapping.read_text(encoding="utf-8")
    broken_mapping.write_text(
        mapping_src.replace(
            "\n  action pay_submit(c)      -> pay(c)",
            "\n  action pay_without_approval(c) -> pay(c)"
            "\n  action pay_submit(c)      -> pay(c)",
        ),
        encoding="utf-8",
    )
    broken = _run(
        [
            sys.executable,
            "-m",
            "fslc",
            "refine",
            str(broken_design),
            str(E2E / "2_requirements.fsl"),
            str(broken_mapping),
            "--depth",
            "4",
        ],
        check=False,
    )
    assert broken.returncode == 1
    broken_json = json.loads(broken.stdout)
    assert broken_json["result"] == "refinement_failed"
    assert broken_json["kind"] == "abs_requires_failed"
    assert broken_json["impl_action"]["name"] == "pay_without_approval"
    assert '"kind": "abs_requires_failed"' in readme
