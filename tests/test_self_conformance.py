"""実装適合の錨: fslc_session モデルと実 CLI 挙動の一致を検査する。"""
from __future__ import annotations

import json
import os
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import pytest

from fslc.cli import exit_code

ROOT = Path(__file__).resolve().parents[1]
SESSION_SPEC = ROOT / "examples/self/fslc_session.fsl"
PY = ROOT / ".venv/bin/python"

USER_ERROR_KINDS = frozenset({"parse", "semantics", "io", "usage", "type"})


@dataclass(frozen=True)
class CorpusCase:
    id: str
    path: Path
    verify_args: tuple[str, ...] = ()
    induction_args: tuple[str, ...] = ("--depth", "8", "--engine", "induction")
    run_verify: bool = True
    run_induction: bool = False


CORPUS: tuple[CorpusCase, ...] = (
    CorpusCase(
        id="happy_pipeline",
        path=ROOT / "examples/self/fslc_session.fsl",
        verify_args=("--depth", "8", "--deadlock", "warn"),
        induction_args=("--depth", "8", "--deadlock", "warn", "--engine", "induction"),
        run_induction=True,
    ),
    CorpusCase(
        id="violated",
        path=ROOT / "examples/gallery/errors/violated_invariant_counter.fsl",
        verify_args=("--depth", "2"),
    ),
    CorpusCase(
        id="reachable_failed",
        path=ROOT / "examples/gallery/injected/bank__over_strengthened_guard.fsl",
        verify_args=("--depth", "8"),
    ),
    CorpusCase(
        id="check_parse_error",
        path=ROOT / "examples/gallery/errors/parse_missing_expression.fsl",
        run_verify=False,
    ),
    CorpusCase(
        id="check_type_error",
        path=ROOT / "examples/gallery/errors/type_undeclared_type.fsl",
        run_verify=False,
    ),
)


@dataclass
class PipelineStep:
    subcommand: str
    argv: list[str]
    out: dict[str, Any]
    returncode: int


@dataclass
class PipelineRun:
    case_id: str
    spec_path: Path
    steps: list[PipelineStep] = field(default_factory=list)


def _run_fslc(*args: str) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT) + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [str(PY), "-m", "fslc", *args],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def _parse_stdout(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.stdout.strip(), proc.stderr
    return json.loads(proc.stdout)


def run_model_pipeline(case: CorpusCase) -> PipelineRun:
    """fslc_session のパイプラインに忠実に CLI を実行する。"""
    run = PipelineRun(case_id=case.id, spec_path=case.path)
    spec = str(case.path)

    check_argv = ["check", spec]
    proc = _run_fslc(*check_argv)
    check_out = _parse_stdout(proc)
    run.steps.append(
        PipelineStep("check", check_argv, check_out, proc.returncode)
    )

    if check_out["result"] != "ok":
        return run

    if not case.run_verify:
        return run

    verify_argv = ["verify", spec, *case.verify_args]
    proc = _run_fslc(*verify_argv)
    verify_out = _parse_stdout(proc)
    run.steps.append(
        PipelineStep("verify", verify_argv, verify_out, proc.returncode)
    )

    if verify_out["result"] != "verified" or not case.run_induction:
        return run

    induction_argv = ["verify", spec, *case.induction_args]
    proc = _run_fslc(*induction_argv)
    induction_out = _parse_stdout(proc)
    run.steps.append(
        PipelineStep("induction", induction_argv, induction_out, proc.returncode)
    )
    return run


def assert_exit_code_matches_severity(step: PipelineStep) -> None:
    expected = exit_code(step.out)
    assert step.returncode == expected, (
        step.subcommand,
        step.out.get("result"),
        step.out.get("kind"),
        step.returncode,
        expected,
    )


def assert_pipeline_contracts(run: PipelineRun) -> None:
    check_step = next((s for s in run.steps if s.subcommand == "check"), None)
    verify_step = next((s for s in run.steps if s.subcommand == "verify"), None)
    induction_step = next((s for s in run.steps if s.subcommand == "induction"), None)

    assert check_step is not None
    if verify_step is not None:
        assert check_step.out["result"] == "ok", run.case_id
    else:
        assert check_step.out["result"] != "ok", run.case_id

    if induction_step is not None:
        assert verify_step is not None, run.case_id
        assert verify_step.out["result"] == "verified", run.case_id
        assert induction_step.out["result"] == "proved", run.case_id


def cli_result_to_session_action(step: PipelineStep) -> str:
    result = step.out["result"]
    kind = step.out.get("kind")

    if step.subcommand == "check":
        if result == "ok":
            return "check_ok"
        if result == "error" and kind in USER_ERROR_KINDS:
            return "check_err"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "verify":
        if result == "verified":
            return "verify_ok"
        if result == "violated":
            return "verify_violated"
        if result == "reachable_failed":
            return "verify_reachable_failed"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "induction":
        if result == "proved":
            return "induction_proved"
        if result == "unknown_cti":
            return "induction_cti"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    raise AssertionError(step.subcommand)


def pipeline_to_trace_events(run: PipelineRun) -> list[dict[str, str]]:
    return [{"action": cli_result_to_session_action(step)} for step in run.steps]


def run_session_replay(trace_events: list[dict[str, str]]) -> tuple[dict[str, Any], int]:
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as fh:
        json.dump(trace_events, fh)
        trace_path = fh.name
    try:
        proc = _run_fslc("replay", str(SESSION_SPEC), "--trace", trace_path)
        out = _parse_stdout(proc)
        return out, proc.returncode
    finally:
        Path(trace_path).unlink(missing_ok=True)


@pytest.mark.parametrize("case", CORPUS, ids=[c.id for c in CORPUS])
def test_corpus_exit_codes_match_severity(case: CorpusCase):
    run = run_model_pipeline(case)
    for step in run.steps:
        assert_exit_code_matches_severity(step)


@pytest.mark.parametrize("case", CORPUS, ids=[c.id for c in CORPUS])
def test_corpus_contract_invariants(case: CorpusCase):
    run = run_model_pipeline(case)
    for step in run.steps:
        assert_exit_code_matches_severity(step)
    assert_pipeline_contracts(run)


@pytest.mark.parametrize("case", CORPUS, ids=[c.id for c in CORPUS])
def test_corpus_replay_conformant(case: CorpusCase):
    run = run_model_pipeline(case)
    trace_events = pipeline_to_trace_events(run)
    out, rc = run_session_replay(trace_events)
    assert out["result"] == "conformant", (case.id, trace_events, out)
    assert rc == 0


NEGATIVE_TRACES = [
    pytest.param(
        [{"action": "verify_ok"}],
        id="verify_ok_without_check_ok",
    ),
    pytest.param(
        [{"action": "check_ok"}, {"action": "induction_proved"}],
        id="induction_proved_without_verify_ok",
    ),
]


@pytest.mark.parametrize("trace_events", NEGATIVE_TRACES)
def test_negative_traces_are_nonconformant(trace_events: list[dict[str, str]]):
    out, rc = run_session_replay(trace_events)
    assert out["result"] == "nonconformant", trace_events
    assert rc == 1
