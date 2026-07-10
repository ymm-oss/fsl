# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Issue #171: the shared assurance-class classifier (`fslc.assurance`)."""
from pathlib import Path

from fslc.assurance import (
    BOUNDED,
    NOT_RUN,
    PROVED,
    REPLAY_OBSERVED,
    STATISTICAL,
    assurance_label,
    classify_result,
    strongest,
    weakest,
)
from fslc.cli import (
    run_ai_check,
    run_ai_compare,
    run_ai_compat,
    run_ai_drift,
    run_ai_eval,
    run_ai_regress,
    run_ai_replay,
    run_db_observe,
    run_domain_replay,
    run_verify,
)

ROOT = Path(__file__).resolve().parents[1]
AI = ROOT / "examples" / "ai"
DB = ROOT / "examples" / "db"
DOMAIN = ROOT / "examples" / "domain"
NFR = ROOT / "examples" / "nfr"


def _ai(name):
    return str(AI / name)


def _db(name):
    return str(DB / name)


def _domain(name):
    return str(DOMAIN / name)


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src, encoding="utf-8")
    return p


# --------------------------------------------------------------------------
# kernel BMC / k-induction
# --------------------------------------------------------------------------
def test_bmc_verified_is_bounded():
    out = run_verify(str(NFR / "support_sla.fsl"), 8, "ignore")
    assert out["result"] == "verified"
    assert classify_result(out) == BOUNDED


def test_induction_proved_is_proved(tmp_path):
    p = _write(tmp_path, "counter_latch.fsl", """
spec CounterLatch {
  state { x: Int }
  init { x = 0 }
  action inc() { requires x < 5  x = x + 1 }
  invariant XRange { x >= 0 and x <= 5 }
}
""")
    out = run_verify(str(p), 8, "ignore", engine="induction")
    assert out["result"] == "proved"
    assert classify_result(out) == PROVED


def test_induction_unknown_cti_is_bounded(tmp_path):
    p = _write(tmp_path, "sync.fsl", """
spec Sync {
  state { x: Int, y: Int }
  init { x = 0  y = 0 }
  action step() { requires x < 4  x = x + 1  y = y + 1 }
  invariant Sync { y <= 4 }
}
""")
    out = run_verify(str(p), 8, "ignore", engine="induction")
    assert out["result"] == "unknown_cti"
    assert classify_result(out) == BOUNDED


# --------------------------------------------------------------------------
# fsl-ai / fsl-db / fsl-domain "not_run" producers -> the acceptance criterion
# --------------------------------------------------------------------------
def test_ai_check_verified_under_assumptions_is_bounded():
    out = run_ai_check(_ai("refund_agent_tool_safety.fsl"))
    assert out["result"] == "verified_under_assumptions"
    assert classify_result(out) == BOUNDED


def test_ai_replay_is_replay_observed():
    out = run_ai_replay(_ai("refund_agent_tool_safety.fsl"), _ai("runtime_conformant.jsonl"))
    assert out["result"] == "replay_conformant"
    assert out["formal_result"] == "not_run"
    assert classify_result(out) == REPLAY_OBSERVED


def test_ai_eval_statistically_supported_is_statistical():
    out = run_ai_eval(
        _ai("support_answer_quality.fsl"),
        records=_ai("support_eval_v3.jsonl"),
        dataset="SupportEvalV3",
        property_name="LooseQuality",
    )
    assert out["result"] == "statistically_supported"
    assert classify_result(out) == STATISTICAL


def test_ai_eval_gate_failure_is_not_run(tmp_path):
    dup = tmp_path / "dup.jsonl"
    line = (
        '{"case_id":"c1","dataset":"SupportEvalV3","slice":"all",'
        '"metric":"accuracy","outcome":true,'
        '"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}\n'
    )
    dup.write_text(line + line, encoding="utf-8")
    out = run_ai_eval(
        _ai("support_answer_quality.fsl"),
        records=str(dup), dataset="SupportEvalV3", property_name="LooseQuality",
    )
    assert out["result"] == "dataset_invalid"
    assert classify_result(out) == NOT_RUN


def test_ai_regress_statistically_unsupported_is_statistical():
    out = run_ai_regress(
        _ai("support_answer_quality.fsl"),
        migration="PromptV7ToV8",
        before_records=_ai("support_eval_v7.jsonl"),
        after_records=_ai("support_eval_v8_regressed.jsonl"),
        dataset="SupportEvalV3",
    )
    assert out["result"] == "statistically_unsupported"
    assert classify_result(out) == STATISTICAL


def test_ai_compare_is_not_run():
    out = run_ai_compare(
        _ai("support_eval_v7.jsonl"), _ai("support_eval_v8_regressed.jsonl"),
        dataset="SupportEvalV3", from_label="prompt_v7", to_label="prompt_v8",
    )
    assert out["result"] == "compared"
    assert classify_result(out) == NOT_RUN


def test_ai_drift_is_replay_observed():
    out = run_ai_drift(
        _ai("support_answer_quality.fsl"),
        logs=_ai("runtime_drift_current.jsonl"),
        baseline_logs=_ai("runtime_drift_baseline.jsonl"),
        property_name="SupportAgentOperationalQuality",
        window="last_7_days", baseline="previous_7_days",
    )
    assert out["result"] == "observed_mismatch"
    assert classify_result(out) == REPLAY_OBSERVED


def test_ai_compat_profile_is_not_run():
    out = run_ai_compat(_ai("support_answer_quality.fsl"), environment="prod")
    assert out["result"] == "compat_profile_generated"
    assert classify_result(out) == NOT_RUN


def test_ai_recursive_agent_analysis_is_not_run():
    out = run_ai_check(_ai("recursive_support_agent.fsl"))
    assert out["result"] == "agent_analyzed"
    assert classify_result(out) == NOT_RUN


def test_ai_project_analysis_is_not_run():
    out = run_ai_check(_ai("support_answer_quality.fsl"))
    assert out["result"] == "ai_project_analyzed"
    assert classify_result(out) == NOT_RUN


def test_db_observe_is_replay_observed():
    out = run_db_observe(_db("runtime_observation_target.fsl"), _db("runtime_observation_mismatch.json"))
    assert out["result"] == "observed_mismatch"
    assert out["formal_result"] == "not_run"
    assert classify_result(out) == REPLAY_OBSERVED


def test_domain_replay_is_replay_observed():
    out = run_domain_replay(_domain("order_async_effect.fsl"),
                             str(DOMAIN / "order_async_effect_replay.jsonl"))
    assert out["result"] == "conformance_checked"
    assert out["guarantee_kind"] == "runtime_observed"
    assert "formal_result" not in out  # domain replay has no formal_result field at all
    assert classify_result(out) == REPLAY_OBSERVED


# --------------------------------------------------------------------------
# ordering / labels
# --------------------------------------------------------------------------
def test_strongest_and_weakest_follow_issue_order():
    tokens = [STATISTICAL, PROVED, NOT_RUN, BOUNDED, REPLAY_OBSERVED]
    assert strongest(tokens) == PROVED
    assert weakest(tokens) == NOT_RUN
    assert strongest([]) == NOT_RUN
    assert weakest([]) == NOT_RUN


def test_assurance_labels_match_issue_vocabulary():
    assert assurance_label(PROVED) == "proved(induction)"
    assert assurance_label(BOUNDED, depth=8) == "bounded(BMC depth 8)"
    assert assurance_label(REPLAY_OBSERVED) == "replay-observed"
    assert assurance_label(STATISTICAL, confidence=0.95) == "statistical(Wilson 95%)"
    assert assurance_label(NOT_RUN) == "not_run"
    assert assurance_label(BOUNDED, depth=8, under_assumptions=True) == "bounded(BMC depth 8)※前提付き"
