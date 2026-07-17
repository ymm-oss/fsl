# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-ai hard-contract coverage."""

import json
from pathlib import Path

from fslc.cli import run_ai_check, run_ai_replay, run_check, run_verify


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "ai"
AI_SCHEMAS = ROOT / "schemas" / "fslc" / "ai"


def _example(name):
    return str(EXAMPLES / name)


def _finding_kinds(out):
    return {finding["kind"] for finding in out["findings"]}


def _violations(out):
    return {finding["violation"] for finding in out["findings"]}


def _guarantees(out):
    return {finding["guarantee_kind"] for finding in out["findings"]}


def test_ai_component_check_accepts_mvp_syntax():
    out = run_check(_example("refund_agent_tool_safety.fsl"))

    assert out["result"] == "ok"
    assert out["spec"] == "RefundAgentToolSafety"


def test_ai_check_verifies_hard_contract_without_kernel_semantics_change():
    out = run_ai_check(_example("refund_agent_tool_safety.fsl"))

    assert out["result"] == "verified_under_assumptions"
    assert out["dialect"] == "fsl-ai-hard.v0"
    assert out["finding_schema_version"] == "fsl-ai-finding.v0"
    assert out["findings"] == []
    assert out["formal_result"] == "verified"
    assert out["kernel"]["result"] == "verified"

    verified = run_verify(_example("refund_agent_tool_safety.fsl"), 8, "warn")
    assert verified["result"] == "verified"


def test_ai_replay_conformant_log_is_runtime_evidence_not_proof():
    out = run_ai_replay(
        _example("refund_agent_tool_safety.fsl"),
        _example("runtime_conformant.jsonl"),
    )

    assert out["result"] == "replay_conformant"
    assert out["formal_result"] == "not_run"
    assert out["evidence"] == {"kind": "runtime_replay", "formal_proof": False}
    assert out["findings"] == []


def test_ai_replay_human_approval_bypass_is_hard_contract_violation():
    out = run_ai_replay(
        _example("refund_agent_tool_safety.fsl"),
        _example("runtime_human_approval_bypass.jsonl"),
    )

    assert out["result"] == "replay_nonconformant"
    assert _finding_kinds(out) == {"ai_hard_contract_violation"}
    assert _violations(out) == {"human_approval_required_before_irreversible_tool"}
    assert _guarantees(out) == {"syntactic_hard"}
    finding = out["findings"][0]
    assert finding["tool"] == "RefundPayment"
    assert finding["failed_rule"] == "human_approval_required"
    assert finding["evidence"]["formal_proof"] is False


def test_ai_replay_forbidden_tool_is_hard_contract_violation():
    out = run_ai_replay(
        _example("refund_agent_tool_safety.fsl"),
        _example("runtime_forbidden_tool.jsonl"),
    )

    assert out["result"] == "replay_nonconformant"
    assert _finding_kinds(out) == {"ai_hard_contract_violation"}
    assert _violations(out) == {"forbidden_tool_call"}
    assert out["findings"][0]["tool"] == "DeleteCustomerData"


def test_ai_replay_declared_capability_mismatch_is_observed_contract_violation():
    out = run_ai_replay(
        _example("refund_agent_tool_safety.fsl"),
        _example("runtime_observed_mismatch.jsonl"),
    )

    assert out["result"] == "replay_nonconformant"
    assert _finding_kinds(out) == {"observed_contract_violation"}
    assert _violations(out) == {"undeclared_tool_observed"}
    assert _guarantees(out) == {"runtime_observed"}
    assert out["formal_result"] == "not_run"


def test_ai_statistical_evidence_schemas_are_non_formal():
    result_schema = json.loads((AI_SCHEMAS / "statistical-result.v0.schema.json").read_text(encoding="utf-8"))
    record_schema = json.loads((AI_SCHEMAS / "eval-record.v0.schema.json").read_text(encoding="utf-8"))

    assert result_schema["$id"].endswith("statistical-result.v0.schema.json")
    assert "formal_result" in result_schema["required"]
    assert result_schema["properties"]["formal_result"]["const"] == "not_run"
    assert result_schema["properties"]["interval"]["properties"]["method"]["const"] == "wilson"
    assert {"case_id", "slice", "metric", "outcome", "evaluator"}.issubset(record_schema["required"])

    fixture = json.loads((EXAMPLES / "statistical_result_supported.json").read_text(encoding="utf-8"))
    assert fixture["result"] == "statistically_supported"
    assert fixture["formal_result"] == "not_run"
    assert fixture["interval"]["method"] == "wilson"


def test_ai_static_check_flags_irreversible_tool_without_approval(tmp_path):
    path = tmp_path / "unsafe_irreversible.fsl"
    path.write_text(
        """ai_component UnsafeRefundAgent {
  tool RefundPayment irreversible {
    schema RefundPaymentV1;
  }
  authority {
    may_execute RefundPayment;
  }
}
""",
        encoding="utf-8",
    )

    out = run_ai_check(str(path))

    assert out["result"] == "violated"
    assert _finding_kinds(out) == {"ai_hard_contract_violation"}
    assert _violations(out) == {"irreversible_tool_without_human_approval_guard"}
    assert out["formal_result"] == "not_run"


def test_ai_static_check_rejects_duplicate_fallback_reason(tmp_path):
    # Two fallback entries sharing a reason would both lower to the same
    # generated action name (fallback_<reason>), silently colliding.
    path = tmp_path / "dup_fallback.fsl"
    path.write_text(
        """ai_component DupFallbackAgent {
  tool SearchOrder {
    schema SearchOrderV1;
  }
  authority {
    may_execute SearchOrder;
  }
  fallback {
    when low_confidence require human_review;
    when low_confidence require refuse_with_reason;
  }
}
""",
        encoding="utf-8",
    )

    out = run_ai_check(str(path))

    assert out["result"] == "error"
    assert "duplicate fallback reason" in out["message"]


def test_ai_replay_distinguishes_schema_and_business_precondition_mismatch(tmp_path):
    log = tmp_path / "bad_tool_call.jsonl"
    log.write_text(
        '{"event":"human_approval","component":"RefundAgentToolSafety","tool":"RefundPayment"}\n'
        '{"event":"tool_call","component":"RefundAgentToolSafety","tool":"RefundPayment",'
        '"mode":"execute","tool_schema":"RefundPaymentV2","schema_valid":false,'
        '"preconditions":{"order_paid":true,"amount_refundable":false},'
        '"args":{"order_id":"redacted","amount":"redacted"}}\n',
        encoding="utf-8",
    )

    out = run_ai_replay(_example("refund_agent_tool_safety.fsl"), str(log))

    assert out["result"] == "replay_nonconformant"
    assert _finding_kinds(out) == {
        "ai_hard_contract_violation",
        "observed_contract_violation",
    }
    assert _violations(out) == {
        "tool_schema_invalid",
        "tool_schema_mismatch",
        "business_precondition_mismatch",
    }
    by_violation = {finding["violation"]: finding for finding in out["findings"]}
    assert by_violation["tool_schema_invalid"]["failed_rule"] == "tool_schema_declared"
    assert by_violation["tool_schema_mismatch"]["failed_rule"] == "runtime_observation"
    assert by_violation["business_precondition_mismatch"]["failed_rule"] == "tool_precondition_declared"


def test_ai_replay_accepts_completion_event_tool_calls_array(tmp_path):
    log = tmp_path / "completion_tool_calls.jsonl"
    log.write_text(
        '{"component":"RefundAgentToolSafety","model":"refund_model_v1",'
        '"prompt":"refund_prompt_v1","output_schema":"RefundDecisionV1",'
        '"tool_calls":[{"name":"SearchOrder","mode":"execute",'
        '"tool_schema":"SearchOrderV1","schema_valid":true,'
        '"preconditions":{"order_exists":true},'
        '"args":{"order_id":"redacted"}}]}\n',
        encoding="utf-8",
    )

    out = run_ai_replay(_example("refund_agent_tool_safety.fsl"), str(log))

    assert out["result"] == "replay_conformant"
    assert out["findings"] == []
