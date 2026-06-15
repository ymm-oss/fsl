"""examples/refinement_liveness — refine は安全性を伝播し活性を伝播しない。

README に書いた各コマンドの結果が実際にその通りであることを固定する。
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
    # 安全性は満たすので refine は通る…
    refine = run_refine(
        str(E / "design_drops_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_drops_liveness_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refines"

    # …が、上位の活性 policy は設計層で壊れている(ラッソ)。
    verify = run_verify(str(E / "design_drops_liveness.fsl"), 8, "ignore")
    assert verify["result"] == "violated"
    assert verify["violation_kind"] == "leadsTo"


def test_fair_restores_liveness_at_the_lower_layer():
    refine = run_refine(
        str(E / "design_keeps_liveness.fsl"),
        str(E / "policy.fsl"),
        str(E / "design_keeps_liveness_refines.fsl"),
        depth=8,
    )
    assert refine["result"] == "refines"
    assert run_verify(str(E / "design_keeps_liveness.fsl"), 8, "ignore")["result"] == "verified"


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
