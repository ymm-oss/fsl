"""examples/refinement_liveness — refine propagates safety but not liveness.

Pins down that each command documented in the README actually behaves as stated.
"""
from pathlib import Path

from fslc.cli import run_refine, run_verify


ROOT = Path(__file__).resolve().parents[1]
E = ROOT / "examples/refinement_liveness"


def test_policy_contract_is_sound():
    assert run_verify(str(E / "policy.fsl"), 8, "ignore")["result"] == "verified"
    assert run_verify(
        str(E / "policy.fsl"), 8, "ignore", engine="induction"
    )["result"] == "proved"


def test_liveness_not_propagated_by_refinement():
    # safety holds, so refine passes...
    refine = run_refine(
        str(E / "design_drops_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_drops_liveness_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refines"

    # ...but the upper-level liveness policy is broken at the design layer (lasso).
    verify = run_verify(str(E / "design_drops_liveness.fsl"), 8, "ignore")
    assert verify["result"] == "violated"
    assert verify["violation_kind"] == "leadsTo"


def test_preserve_progress_catches_liveness_drop():
    refine = run_refine(
        str(E / "design_drops_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_drops_liveness_progress_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refinement_failed"
    assert refine["kind"] == "progress_lost"
    assert refine["violation_kind"] == "leadsTo"
    assert refine["invariant"] == "EveryClaimDecided"
    assert refine["progress"] == {
        "leadsTo": "EveryClaimDecided",
        "actions": ["approve", "reject"],
    }


def test_fair_restores_liveness_at_the_lower_layer():
    refine = run_refine(
        str(E / "design_keeps_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_keeps_liveness_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refines"
    assert run_verify(str(E / "design_keeps_liveness.fsl"), 8, "ignore")["result"] == "verified"


def test_preserve_progress_passes_when_lower_layer_keeps_liveness():
    refine = run_refine(
        str(E / "design_keeps_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_keeps_liveness_progress_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refines"
    assert refine["progress"] == {
        "EveryClaimDecided": {
            "checked_to_depth": 8,
            "actions": ["approve", "reject"],
        }
    }


def test_safety_violation_is_propagated_and_caught():
    refine = run_refine(
        str(E / "design_bypasses_control.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_bypasses_control_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refinement_failed"
    assert refine["kind"] == "abs_requires_failed"
    assert refine["impl_action"]["name"] == "fast_pay"
