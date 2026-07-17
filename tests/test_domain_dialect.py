# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""fsl-domain / fsl-effect v0 coverage."""

import json
from pathlib import Path

import pytest

from fslc.cli import (
    main,
    run_check,
    run_domain_analyze,
    run_domain_check,
    run_domain_expand,
    run_domain_generate,
    run_domain_replay,
    run_domain_testgen,
    run_verify,
)


ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples" / "domain"
DOMAIN_SCHEMAS = ROOT / "schemas" / "fslc" / "domain"


def _example(name):
    return str(EXAMPLES / name)


def _finding_kinds(out):
    return {finding["kind"] for finding in out["findings"]}


def test_domain_functional_core_checks_and_verifies():
    path = _example("order_functional_ddd.fsl")

    checked = run_check(path)
    assert checked["result"] == "ok"
    assert checked["spec"] == "OrderFunctionalDdd"

    verified = run_verify(path, 6, "warn")
    assert verified["result"] == "verified"

    domain = run_domain_check(path, depth=6)
    assert domain["result"] == "verified_under_assumptions"
    assert domain["dialect"] == "fsl-domain-effect.v0"
    assert domain["finding_schema_version"] == "fsl-domain-finding.v0"
    assert domain["formal_result"] == "verified"
    assert domain["findings"] == []


def test_domain_async_effect_lowers_to_kernel_and_checks_cleanly():
    path = _example("order_async_effect.fsl")

    out = run_domain_check(path, depth=6)

    assert out["result"] == "verified_under_assumptions"
    assert out["formal_result"] == "verified"
    assert out["findings"] == []
    assert "DOMAIN-ASSUME-FINITE-DOMAIN-MODEL" in {a["id"] for a in out["assumptions"]}
    assert {
        "order_request_payment_capture",
        "capture_payment_complete_payment_captured",
        "capture_payment_retry",
    }.issubset(set(out["generated_actions"]))


def test_domain_static_findings_block_formal_run_for_hard_errors():
    out = run_domain_check(_example("unsafe_irreversible_effect_without_idempotency.fsl"))

    assert out["result"] == "violated"
    assert out["formal_result"] == "not_run"
    assert "irreversible_effect_without_idempotency_key" in _finding_kinds(out)
    assert "pending_effect_without_timeout_or_fallback" in _finding_kinds(out)


def test_domain_analyze_reports_aggregate_and_effect_summary():
    out = run_domain_analyze(_example("order_async_effect.fsl"))

    assert out["result"] == "analyzed"
    assert out["aggregates"][0]["name"] == "Order"
    assert out["effects"][0]["name"] == "CapturePayment"
    assert out["effects"][0]["correlation_id"] == "PaymentCaptureRequested.payment_request_id"


def test_domain_saga_analyze_and_expand_process_manager_actions():
    out = run_domain_analyze(_example("order_fulfillment_saga.fsl"))

    assert out["result"] == "analyzed"
    assert out["sagas"][0]["name"] == "OrderFulfillment"
    assert out["sagas"][0]["starts_on"] == "OrderApproved"
    assert out["sagas"][0]["outboxes"] == ["OrderOutbox"]

    expanded = run_domain_expand(_example("order_fulfillment_saga.fsl"))
    source = expanded["kernel_source"]
    assert "action saga_order_fulfillment_reserve_inventory()" in source
    assert "action saga_order_fulfillment_capture_payment_timeout()" in source
    assert "action saga_order_fulfillment_compensate_payment_failed_after_inventory_reserved()" in source


def test_domain_expand_emits_kernel_source_with_namespaced_enum_members():
    out = run_domain_expand(_example("order_async_effect.fsl"))

    assert out["result"] == "expanded"
    source = out["kernel_source"]
    assert "spec OrderAsyncEffect" in source
    assert "OrderStatus_Pending" in source
    assert "PaymentStatus_PaymentPending" in source
    assert "capture_payment_status: Map<PaymentRequestId, CapturePaymentEffectStatus>" in source


def test_domain_generate_typescript_scaffold():
    out = run_domain_generate(_example("order_functional_ddd.fsl"))

    assert out["result"] == "generated"
    files = {item["path"]: item["content"] for item in out["files"]}
    assert "types.ts" in files
    assert "order/decide.ts" in files
    assert "export function decideOrder" in files["order/decide.ts"]
    assert 'type: "CancelOrder"' in files["types.ts"]


def test_domain_generate_saga_process_manager_and_python_target():
    ts = run_domain_generate(_example("order_fulfillment_saga.fsl"))
    files = {item["path"]: item["content"] for item in ts["files"]}

    assert "process-manager.ts" in files
    assert "onOrderFulfillment" in files["process-manager.ts"]

    py = run_domain_generate(_example("order_functional_ddd.fsl"), target="python")
    py_files = {item["path"]: item["content"] for item in py["files"]}
    assert "domain_scaffold.py" in py_files
    assert "def decide_order" in py_files["domain_scaffold.py"]


def test_domain_replay_accepts_command_event_effect_jsonl():
    out = run_domain_replay(
        _example("order_async_effect.fsl"),
        str(EXAMPLES / "order_async_effect_replay.jsonl"),
    )

    assert out["result"] == "conformance_checked"
    assert out["guarantee_kind"] == "runtime_observed"
    assert out["findings"] == []
    assert "PaymentCaptured" in out["events_observed"]


def test_domain_replay_reports_uncorrelated_completion(tmp_path):
    logs = tmp_path / "completion_without_request.jsonl"
    logs.write_text(
        '{"event":"effect_completion","effect":"CapturePayment","name":"PaymentCaptured","correlation_id":"p9","params":{"payment_request_id":"p9"}}\n',
        encoding="utf-8",
    )

    out = run_domain_replay(_example("order_async_effect.fsl"), str(logs))

    assert out["result"] == "nonconformant"
    assert "uncorrelated_async_completion" in _finding_kinds(out)


def test_domain_testgen_wraps_vitest_conformance_scaffold():
    out = run_domain_testgen(_example("order_functional_ddd.fsl"), depth=4)

    assert out["result"] == "generated"
    assert out["target"] == "vitest"
    assert "Auto-generated fsl-domain conformance scaffold" in out["content"]
    assert "const scenario = wired ? test : test.skip" in out["content"]


def test_domain_finding_schema_fixture_is_versioned():
    schema = json.loads((DOMAIN_SCHEMAS / "finding.v0.schema.json").read_text(encoding="utf-8"))

    assert schema["$id"].endswith("finding.v0.schema.json")
    assert schema["properties"]["schema_version"]["const"] == "fsl-domain-finding.v0"
    assert "irreversible_effect_without_idempotency_key" in schema["properties"]["kind"]["enum"]
    assert "process_wait_cycle" in schema["properties"]["kind"]["enum"]


def test_domain_reports_reliable_effect_without_outbox(tmp_path):
    path = tmp_path / "reliable_no_outbox.fsl"
    path.write_text(
        """domain ReliableNoOutbox {
  implementation_profile functional_ddd
  type St = Pending | Done
  aggregate Job {
    state { st: St = Pending; }
    command Start { input request_id: RequestId }
    event Started { request_id: RequestId }
    event Done { request_id: RequestId }
    decide Start { emits Started }
    evolve Started { st = Pending }
    evolve Done { st = Done }
  }
  effect ReliableWorker {
    async
    reliable
    correlation_id Started.request_id
    handles Started
    emits one_of [Done]
    retry { max_attempts 2 }
  }
}
""",
        encoding="utf-8",
    )

    out = run_domain_check(str(path))

    assert out["result"] == "verified_under_assumptions"
    assert "reliable_effect_without_outbox_boundary" in _finding_kinds(out)


def test_domain_reports_process_wait_cycle(tmp_path):
    path = tmp_path / "saga_cycle.fsl"
    path.write_text(
        """domain SagaCycle {
  implementation_profile functional_ddd
  type St = A | B
  aggregate Flow {
    state { st: St = A; }
    command Start {}
    event Started {}
    event AEvent {}
    event BEvent {}
    decide Start { emits Started }
    evolve Started { st = A }
    evolve AEvent { st = A }
    evolve BEvent { st = B }
  }
  saga CyclicFlow {
    starts_on Started
    step AStep {
      emits AEvent
      awaits one_of [BEvent]
    }
    step BStep {
      emits BEvent
      awaits one_of [AEvent]
    }
  }
}
""",
        encoding="utf-8",
    )

    out = run_domain_check(str(path))

    assert out["result"] == "violated"
    assert "process_wait_cycle" in _finding_kinds(out)


def test_domain_duplicate_enum_members_are_namespaced_before_kernel(tmp_path):
    path = tmp_path / "duplicate_members.fsl"
    path.write_text(
        """domain DuplicateMembers {
  implementation_profile functional_ddd
  type OrderStatus = Pending | Approved
  type PaymentStatus = Pending | Captured
  aggregate Order {
    state {
      status: OrderStatus = Pending;
      payment_status: PaymentStatus = Pending;
    }
    command Approve {}
    event ApprovedEvent {}
    decide Approve {
      requires status == Pending
      emits ApprovedEvent
    }
    evolve ApprovedEvent {
      status = Approved
      payment_status = Captured
    }
  }
}
""",
        encoding="utf-8",
    )

    out = run_check(str(path))

    assert out["result"] == "ok"


def test_domain_canonical_enum_parser_matches_legacy_union(tmp_path):
    canonical = tmp_path / "canonical.fsl"
    legacy = tmp_path / "legacy.fsl"
    body = """
  aggregate Order {
    state { status: Status = Pending; }
    command Approve {}
    event ApprovedEvent {}
    decide Approve { emits ApprovedEvent }
    evolve ApprovedEvent { status = Approved }
  }
}
"""
    canonical.write_text(
        "domain Orders {\n  enum Status { Pending, Approved }\n" + body,
        encoding="utf-8",
    )
    legacy.write_text(
        "domain Orders {\n  type Status = Pending | Approved\n" + body,
        encoding="utf-8",
    )
    canonical_result = run_check(str(canonical))
    legacy_result = run_check(str(legacy))
    assert canonical_result["result"] == legacy_result["result"] == "ok"


def test_python_compatibility_cli_enforces_domain_edition(tmp_path, capsys):
    legacy = tmp_path / "legacy.fsl"
    legacy.write_text(
        """domain Orders {
  type Status = Pending | Approved
  aggregate Order { state { status: Status = Pending; } }
}
""",
        encoding="utf-8",
    )

    with pytest.raises(SystemExit) as stopped:
        main(["check", str(legacy)])
    assert stopped.value.code == 0
    output = json.loads(capsys.readouterr().out)
    assert any(
        warning.get("code") == "deprecated_domain_enum_union"
        for warning in output["warnings"]
    )

    for arguments in (
        ["check", str(legacy), "--edition", "next"],
        ["verify", str(legacy), "--edition", "next", "--no-cache"],
        ["domain", "check", str(legacy), "--edition", "next"],
    ):
        with pytest.raises(SystemExit) as stopped:
            main(arguments)
        assert stopped.value.code == 2
        output = json.loads(capsys.readouterr().out)
        assert output["kind"] == "deprecated_domain_enum_union"
        assert output["findings"][0]["severity"] == "error"

    canonical = tmp_path / "canonical.fsl"
    canonical.write_text(
        legacy.read_text(encoding="utf-8").replace(
            "type Status = Pending | Approved",
            "enum Status { Pending, Approved }",
        ),
        encoding="utf-8",
    )
    with pytest.raises(SystemExit) as stopped:
        main(["check", str(canonical), "--edition", "next"])
    assert stopped.value.code == 0
    output = json.loads(capsys.readouterr().out)
    assert output["result"] == "ok"
    assert output["edition"] == "next"
