"""実装適合の錨: fslc_session / fslc_monitor モデルと実 CLI 挙動の一致を検査する。"""
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
MONITOR_SPEC = ROOT / "examples/self/fslc_monitor.fsl"
CART_SPEC = ROOT / "specs/cart_v1.fsl"
PY = ROOT / ".venv/bin/python"

USER_ERROR_KINDS = frozenset({"parse", "semantics", "io", "usage", "type"})

# ---------------------------------------------------------------------------
# (b) fslc_session 錨 — check→verify→induction パイプライン
# ---------------------------------------------------------------------------

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


# (b-2) 追加 subcommand 錨。tool_fault (exit 3) は内部エラーを安全に誘発できないため
# モデルには在るが実装錨は未整備 — 下記コーパスにも含めない。
@dataclass(frozen=True)
class SubcommandAnchorCase:
    id: str
    check_path: Path
    subcommand: str
    argv: tuple[str, ...] = ()
    refine_abs: Path | None = None
    refine_mapping: Path | None = None
    replay_spec: Path | None = None
    replay_events: tuple[dict[str, Any], ...] | None = None


SUBCOMMAND_CORPUS: tuple[SubcommandAnchorCase, ...] = (
    SubcommandAnchorCase(
        id="verify_user_error",
        check_path=ROOT / "examples/self/no_actions.fsl",
        subcommand="verify",
        argv=("--depth", "1"),
    ),
    SubcommandAnchorCase(
        id="scenarios_ok",
        check_path=CART_SPEC,
        subcommand="scenarios",
        argv=("--depth", "8"),
    ),
    SubcommandAnchorCase(
        id="explained_ok",
        check_path=CART_SPEC,
        subcommand="explain",
        argv=("--depth", "4"),
    ),
    SubcommandAnchorCase(
        id="mutated_ok",
        check_path=CART_SPEC,
        subcommand="mutate",
        argv=("--depth", "4"),
    ),
    SubcommandAnchorCase(
        id="typestate_ok",
        check_path=ROOT / "specs/order_workflow.fsl",
        subcommand="typestate",
    ),
    SubcommandAnchorCase(
        id="refines_ok",
        check_path=ROOT / "examples/refinement_chain/bot.fsl",
        subcommand="refine",
        argv=("--depth", "6"),
        refine_abs=ROOT / "examples/refinement_chain/mid.fsl",
        refine_mapping=ROOT / "examples/refinement_chain/bot_refines_mid.fsl",
    ),
    SubcommandAnchorCase(
        id="refine_failed",
        check_path=ROOT / "examples/gallery/errors/refinement_failed_impl.fsl",
        subcommand="refine",
        argv=("--depth", "3"),
        refine_abs=ROOT / "examples/gallery/errors/refinement_failed_abs.fsl",
        refine_mapping=ROOT / "examples/gallery/errors/refinement_failed_map.fsl",
    ),
    SubcommandAnchorCase(
        id="replay_conformant",
        check_path=CART_SPEC,
        subcommand="replay",
        replay_spec=CART_SPEC,
        replay_events=(
            {"action": "add_to_cart", "params": {"u": 0, "i": 0}},
            {"action": "checkout", "params": {"u": 0}},
        ),
    ),
    SubcommandAnchorCase(
        id="replay_nonconformant",
        check_path=CART_SPEC,
        subcommand="replay",
        replay_spec=CART_SPEC,
        replay_events=(
            {"action": "add_to_cart", "params": {"u": 0, "i": 0}},
            {"action": "add_to_cart", "params": {"u": 0, "i": 1}},
            {"action": "checkout", "params": {"u": 0}},
        ),
    ),
)


# ---------------------------------------------------------------------------
# (a) fslc_monitor 錨 — 実 replay 実行の観測をモデルへ写像
# ---------------------------------------------------------------------------

# specs/cart_v1.fsl: add_to_cart は cart[u]==none のときのみ enabled。
CART_OK_EVENTS: tuple[dict[str, Any], ...] = (
    {"action": "add_to_cart", "params": {"u": 0, "i": 0}},
    {"action": "checkout", "params": {"u": 0}},
)
CART_REJECT_EVENTS: tuple[dict[str, Any], ...] = (
    {"action": "add_to_cart", "params": {"u": 0, "i": 0}},
    {"action": "add_to_cart", "params": {"u": 0, "i": 1}},
    {"action": "checkout", "params": {"u": 0}},
)
CART_EMPTY_EVENTS: tuple[dict[str, Any], ...] = ()


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


def run_subcommand_anchor(case: SubcommandAnchorCase) -> PipelineRun:
    """check ok 後に単一 subcommand を実行し、結果を記録する。"""
    run = PipelineRun(case_id=case.id, spec_path=case.check_path)
    spec = str(case.check_path)

    check_argv = ["check", spec]
    proc = _run_fslc(*check_argv)
    check_out = _parse_stdout(proc)
    run.steps.append(PipelineStep("check", check_argv, check_out, proc.returncode))
    if check_out["result"] != "ok":
        return run

    trace_path: str | None = None
    try:
        if case.subcommand == "refine":
            assert case.refine_abs is not None and case.refine_mapping is not None
            sub_argv = [
                "refine",
                spec,
                str(case.refine_abs),
                str(case.refine_mapping),
                *case.argv,
            ]
        elif case.subcommand == "replay":
            assert case.replay_spec is not None and case.replay_events is not None
            with tempfile.NamedTemporaryFile(
                "w", suffix=".json", delete=False, encoding="utf-8"
            ) as fh:
                json.dump(list(case.replay_events), fh)
                trace_path = fh.name
            sub_argv = ["replay", str(case.replay_spec), "--trace", trace_path, *case.argv]
        else:
            sub_argv = [case.subcommand, spec, *case.argv]

        proc = _run_fslc(*sub_argv)
        sub_out = _parse_stdout(proc)
        run.steps.append(PipelineStep(case.subcommand, sub_argv, sub_out, proc.returncode))
    finally:
        if trace_path:
            Path(trace_path).unlink(missing_ok=True)
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
        if result == "error" and kind in USER_ERROR_KINDS:
            return "verify_user_error"
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

    if step.subcommand == "scenarios":
        if result == "scenarios":
            return "scenarios_ok"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "explain":
        if result == "explained":
            return "explained_ok"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "mutate":
        if result == "mutated":
            return "mutated_ok"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "typestate":
        if result == "typestate":
            return "typestate_ok"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "refine":
        if result == "refines":
            return "refines_ok"
        if result == "refinement_failed":
            return "refine_failed"
        if result == "error" and kind == "internal":
            return "tool_fault"
        raise AssertionError((step.subcommand, result, kind))

    if step.subcommand == "replay":
        if result == "conformant":
            return "replay_conformant"
        if result == "nonconformant":
            return "replay_nonconformant"
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


def run_spec_replay(spec: Path, events: list[dict[str, Any]]) -> tuple[dict[str, Any], int]:
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as fh:
        json.dump(events, fh)
        trace_path = fh.name
    try:
        proc = _run_fslc("replay", str(spec), "--trace", trace_path)
        out = _parse_stdout(proc)
        return out, proc.returncode
    finally:
        Path(trace_path).unlink(missing_ok=True)


def replay_out_to_monitor_actions(
    replay_out: dict[str, Any],
    events: list[dict[str, Any]],
) -> list[dict[str, str]]:
    """実 replay 結果を fslc_monitor の action 列へ写像する。"""
    actions: list[dict[str, str]] = []
    if replay_out["result"] == "conformant":
        for _ in events:
            actions.append({"action": "step_ok"})
        actions.append({"action": "finish"})
        return actions

    if replay_out["result"] == "nonconformant":
        failed_at = replay_out["failed_at_event"]
        for i in range(failed_at):
            actions.append({"action": "step_ok"})
        actions.append({"action": "step_reject"})
        return actions

    raise AssertionError(replay_out)


def run_monitor_replay(trace_events: list[dict[str, str]]) -> tuple[dict[str, Any], int]:
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as fh:
        json.dump(trace_events, fh)
        trace_path = fh.name
    try:
        proc = _run_fslc("replay", str(MONITOR_SPEC), "--trace", trace_path)
        out = _parse_stdout(proc)
        return out, proc.returncode
    finally:
        Path(trace_path).unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# (b) fslc_session 既存コーパス
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# (b-1) verify_user_error 追加後の self-spec 健全性
# ---------------------------------------------------------------------------

def test_fslc_session_self_spec_still_verifies():
    for args in (
        ["check", str(SESSION_SPEC)],
        ["verify", str(SESSION_SPEC), "--depth", "8", "--deadlock", "warn"],
        [
            "verify",
            str(SESSION_SPEC),
            "--depth",
            "8",
            "--deadlock",
            "warn",
            "--engine",
            "induction",
        ],
    ):
        proc = _run_fslc(*args)
        out = _parse_stdout(proc)
        if args[0] == "check":
            assert out["result"] == "ok", out
        elif args[-1] == "induction":
            assert out["result"] == "proved", out
        else:
            assert out["result"] == "verified", out
        assert proc.returncode == 0


def test_fslc_session_mutate_kill_rate_not_degraded():
    """verify_user_error 追加後も mutate kill-rate が大きく落ちないこと。"""
    proc = _run_fslc("mutate", str(SESSION_SPEC), "--depth", "8")
    out = _parse_stdout(proc)
    assert out["result"] == "mutated", out
    killed = sum(1 for m in out["mutants"] if m["status"] == "killed")
    rate = killed / len(out["mutants"])
    assert rate >= 0.65, (killed, len(out["mutants"]), rate)


# ---------------------------------------------------------------------------
# (b-2) 拡張 subcommand 錨
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("case", SUBCOMMAND_CORPUS, ids=[c.id for c in SUBCOMMAND_CORPUS])
def test_subcommand_anchor_exit_codes(case: SubcommandAnchorCase):
    run = run_subcommand_anchor(case)
    for step in run.steps:
        assert_exit_code_matches_severity(step)


@pytest.mark.parametrize("case", SUBCOMMAND_CORPUS, ids=[c.id for c in SUBCOMMAND_CORPUS])
def test_subcommand_anchor_session_replay_conformant(case: SubcommandAnchorCase):
    run = run_subcommand_anchor(case)
    trace_events = pipeline_to_trace_events(run)
    out, rc = run_session_replay(trace_events)
    assert out["result"] == "conformant", (case.id, trace_events, out)
    assert rc == 0


# ---------------------------------------------------------------------------
# (a) fslc_monitor 錨
# ---------------------------------------------------------------------------

@pytest.mark.parametrize(
    ("events", "expect_result", "expect_steps"),
    [
        pytest.param(list(CART_OK_EVENTS), "conformant", 2, id="all_accepted"),
        pytest.param(list(CART_EMPTY_EVENTS), "conformant", 0, id="empty_log"),
    ],
)
def test_monitor_anchor_conformant_replay(
    events: list[dict[str, Any]],
    expect_result: str,
    expect_steps: int,
):
    replay_out, rc = run_spec_replay(CART_SPEC, events)
    assert replay_out["result"] == expect_result, replay_out
    assert replay_out["steps_checked"] == expect_steps, replay_out
    assert rc == (0 if expect_result == "conformant" else 1)

    monitor_trace = replay_out_to_monitor_actions(replay_out, events)
    mon_out, mon_rc = run_monitor_replay(monitor_trace)
    assert mon_out["result"] == "conformant", (monitor_trace, mon_out)
    assert mon_rc == 0


def test_monitor_anchor_stops_at_first_reject():
    """最初の拒否で停止し、以降のイベントが処理されない (NoStepAfterReject)。"""
    events = list(CART_REJECT_EVENTS)
    replay_out, rc = run_spec_replay(CART_SPEC, events)
    assert replay_out["result"] == "nonconformant", replay_out
    assert replay_out["failed_at_event"] == 1, replay_out
    assert rc == 1

    # reject 後の checkout (index 2) は処理されていない
    failed_at = replay_out["failed_at_event"]
    assert failed_at < len(events) - 1

    monitor_trace = replay_out_to_monitor_actions(replay_out, events)
    assert monitor_trace == [
        {"action": "step_ok"},
        {"action": "step_reject"},
    ]
    assert len(monitor_trace) == failed_at + 1

    mon_out, mon_rc = run_monitor_replay(monitor_trace)
    assert mon_out["result"] == "conformant", (monitor_trace, mon_out)
    assert mon_rc == 0
    assert mon_out["final_state"]["processed"] == failed_at
    assert mon_out["final_state"]["status"] == "Nonconformant"


MONITOR_NEGATIVE_TRACES = [
    pytest.param(
        [{"action": "step_ok"}, {"action": "step_reject"}, {"action": "step_ok"}],
        id="step_ok_after_reject",
    ),
    pytest.param(
        [{"action": "finish"}, {"action": "step_ok"}],
        id="step_ok_after_finish",
    ),
]


@pytest.mark.parametrize("trace_events", MONITOR_NEGATIVE_TRACES)
def test_monitor_negative_traces_are_nonconformant(trace_events: list[dict[str, str]]):
    out, rc = run_monitor_replay(trace_events)
    assert out["result"] == "nonconformant", trace_events
    assert rc == 1
