# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-ai stochastic, migration, drift, and compatibility evidence coverage."""

from pathlib import Path

from fslc.cli import (
    run_ai_check,
    run_ai_compare,
    run_ai_compat,
    run_ai_drift,
    run_ai_eval,
    run_ai_replay,
    run_ai_regress,
    run_check,
    run_compat_check,
)


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "ai"
DB_EXAMPLES = ROOT / "examples" / "db"


def _example(name):
    return str(EXAMPLES / name)


def test_ai_project_check_accepts_proposal_level_declarations():
    out = run_ai_check(_example("support_answer_quality.fsl"))

    assert out["result"] == "ai_project_analyzed"
    assert out["formal_result"] == "not_run"
    assert out["components"] == ["SupportAnswerAgent"]
    assert set(out["statistical_properties"]) == {"LooseQuality", "StrictQuality"}
    assert out["observed_properties"] == ["SupportAgentOperationalQuality"]
    assert out["migrations"] == ["PromptV7ToV8"]
    assert {block["kind"] for block in out["raw_blocks"]} == {
        "ai_action",
        "ai_contract",
        "authority",
        "retriever",
        "trust_boundary",
    }

    checked = run_check(_example("support_answer_quality.fsl"))
    assert checked["result"] == "ok"
    assert checked["ai_analysis_result"] == "ai_project_analyzed"


def test_ai_eval_supported_wilson_bound_from_precomputed_jsonl():
    out = run_ai_eval(
        _example("support_answer_quality.fsl"),
        records=_example("support_eval_v3.jsonl"),
        dataset="SupportEvalV3",
        property_name="LooseQuality",
    )

    assert out["result"] == "statistically_supported"
    assert out["formal_result"] == "not_run"
    assert out["interval"]["method"] == "wilson"
    assert out["findings"] == []
    assert {check["slice"] for check in out["checks"]} == {"all", "JapaneseRefundTickets"}


def test_ai_eval_reports_statistical_contract_unsupported_for_strict_slice():
    out = run_ai_eval(
        _example("support_answer_quality.fsl"),
        records=_example("support_eval_v3.jsonl"),
        dataset="SupportEvalV3",
        property_name="StrictQuality",
    )

    assert out["result"] == "statistically_unsupported"
    assert out["findings"][0]["kind"] == "statistical_contract_unsupported"
    assert out["findings"][0]["minimal_conflict_set"] == {
        "property": "StrictQuality",
        "dataset": "SupportEvalV3",
        "slice": "JapaneseRefundTickets",
        "metric": "accuracy",
    }


def test_ai_eval_rejects_duplicate_eval_records(tmp_path):
    records = tmp_path / "dup.jsonl"
    line = (
        '{"case_id":"c1","dataset":"SupportEvalV3","slice":"all",'
        '"metric":"accuracy","outcome":true,'
        '"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}\n'
    )
    records.write_text(line + line, encoding="utf-8")

    out = run_ai_eval(
        _example("support_answer_quality.fsl"),
        records=str(records),
        dataset="SupportEvalV3",
        property_name="LooseQuality",
    )

    assert out["result"] == "dataset_invalid"
    assert out["findings"][0]["violation"] == "dataset_invalid"


def test_ai_regress_detects_no_regression_violation():
    out = run_ai_regress(
        _example("support_answer_quality.fsl"),
        migration="PromptV7ToV8",
        before_records=_example("support_eval_v7.jsonl"),
        after_records=_example("support_eval_v8_regressed.jsonl"),
        dataset="SupportEvalV3",
    )

    assert out["result"] == "statistically_unsupported"
    assert {finding["kind"] for finding in out["findings"]} == {"ai_migration_regression"}
    assert {finding["minimal_conflict_set"]["metric"] for finding in out["findings"]} == {
        "accuracy",
        "hallucination_rate",
    }


def test_ai_compare_returns_metric_deltas_without_threshold_claim():
    out = run_ai_compare(
        _example("support_eval_v7.jsonl"),
        _example("support_eval_v8_regressed.jsonl"),
        dataset="SupportEvalV3",
        from_label="prompt_v7",
        to_label="prompt_v8",
    )

    assert out["result"] == "compared"
    assert out["formal_result"] == "not_run"
    by_metric = {item["metric"]: item for item in out["comparisons"]}
    assert by_metric["accuracy"]["delta"] < 0


def test_ai_drift_reports_runtime_observed_mismatch():
    out = run_ai_drift(
        _example("support_answer_quality.fsl"),
        logs=_example("runtime_drift_current.jsonl"),
        baseline_logs=_example("runtime_drift_baseline.jsonl"),
        property_name="SupportAgentOperationalQuality",
        window="last_7_days",
        baseline="previous_7_days",
    )

    assert out["result"] == "observed_mismatch"
    assert out["formal_result"] == "not_run"
    assert {finding["kind"] for finding in out["findings"]} == {"ai_observed_drift"}


def test_ai_compat_generates_shared_artifact_capability_profile():
    out = run_ai_compat(_example("support_answer_quality.fsl"), environment="prod")

    assert out["result"] == "compat_profile_generated"
    profile = out["profiles"][0]
    assert profile["component"] == "SupportAnswerAgent"
    assert "model.gpt_5_5" in profile["requires"]
    assert "prompt.support_answer_prompt_v8" in profile["requires"]
    assert "retriever.support_docs_index_v14" in profile["requires"]
    assert "tool.SearchDocs" in profile["requires"]
    assert profile["provides"] == ["output.AnswerSchemaV2"]


def test_ai_replay_selects_component_from_project_and_checks_artifact_metadata(tmp_path):
    logs = tmp_path / "support_runtime.jsonl"
    logs.write_text(
        '{"component":"SupportAnswerAgent","model":"gpt_5_5",'
        '"prompt":"support_answer_prompt_v9","retriever":"support_docs_index_v14",'
        '"output_schema":"AnswerSchemaV2"}\n',
        encoding="utf-8",
    )

    out = run_ai_replay(
        _example("support_answer_quality.fsl"),
        str(logs),
        component="SupportAnswerAgent",
    )

    assert out["result"] == "replay_nonconformant"
    assert out["findings"][0]["kind"] == "observed_contract_violation"
    assert out["findings"][0]["violation"] == "prompt_mismatch"


def test_compat_check_include_ai_delegates_to_dbsystem_capability_model():
    out = run_compat_check(str(DB_EXAMPLES / "safe_ai_artifact_compat.fsl"), include_ai=True)

    assert out["result"] == "verified_under_assumptions"
    assert out["compat"] == {
        "include_ai": True,
        "source": "dbsystem artifact capability model",
    }
