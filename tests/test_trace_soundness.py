from __future__ import annotations

import pytest

from fslc.model import display_label
from fslc.runtime import Monitor
from oracle import expr_holds_in_monitor, replay_trace, run_verify_case
from test_oracle_agreement import oracle_cases


REPLAYABLE_VIOLATIONS = {"invariant", "type_bound", "ensures", "partial_op"}


@pytest.mark.parametrize("case", oracle_cases(), ids=lambda c: c.id)
def test_violated_traces_replay_to_claimed_violation(case):
    result = run_verify_case(case)
    if result.get("result") != "violated":
        pytest.skip("case did not emit a violated trace")

    kind = result.get("violation_kind")
    if kind == "leadsTo":
        pytest.skip("Monitor replay is finite-log safety only, not leadsTo lasso checking")

    mon, replayed = replay_trace(case.path, result["trace"])
    if kind == "deadlock":
        assert replayed and all(step.get("ok") for step in replayed), replayed
        assert mon.enabled() == []
        return

    assert kind in REPLAYABLE_VIOLATIONS
    assert replayed, result
    final = replayed[-1]
    assert final.get("ok") is False, {"result": result, "replayed": replayed}
    assert final.get("kind") == kind
    assert len(replayed) == result["violated_at_step"]


@pytest.mark.parametrize("case", oracle_cases(), ids=lambda c: c.id)
def test_reachable_witnesses_replay_to_final_property(case):
    result = run_verify_case(case)
    if result.get("result") not in {"verified", "proved"} or not result.get("reachables"):
        pytest.skip("case did not emit reachable witnesses")

    spec = Monitor(case.path).spec
    by_name = {display_label(reach["name"], spec): reach for reach in spec["reachables"]}
    for name, data in result["reachables"].items():
        mon, replayed = replay_trace(case.path, data["witness"])
        assert all(step.get("ok") for step in replayed), {"reachable": name, "replayed": replayed}
        assert expr_holds_in_monitor(mon, by_name[name]["expr"]), name


@pytest.mark.parametrize("case", oracle_cases(), ids=lambda c: c.id)
def test_proved_specs_have_no_bmc_violation_four_steps_deeper(case):
    result = run_verify_case(case)
    if result.get("result") != "proved":
        pytest.skip("case was not proved")

    deeper = run_verify_case(type(case)(
        path=case.path,
        depth=case.depth + 4,
        deadlock=case.deadlock,
        engine="bmc",
    ))
    assert deeper.get("result") != "violated", deeper
