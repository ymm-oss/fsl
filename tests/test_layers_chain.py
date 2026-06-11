from pathlib import Path

from fslc.cli import run_refine, run_scenarios, run_verify


ROOT = Path(__file__).resolve().parents[1]
LAYERS = ROOT / "examples/layers"


def test_return_layers_chain_verifies_refines_and_emits_acceptance():
    policy_verified = run_verify(str(LAYERS / "return_policy.fsl"), 8, "ignore")
    assert policy_verified["result"] == "verified"

    policy_proved = run_verify(str(LAYERS / "return_policy.fsl"), 8, "ignore", engine="induction")
    assert policy_proved["result"] == "proved"

    system = run_verify(str(LAYERS / "return_system.fsl"), 8, "ignore")
    assert system["result"] == "verified"
    assert system["implements"]["result"] == "refines"

    impl = run_refine(
        str(LAYERS / "return_impl.fsl"),
        str(LAYERS / "return_system.fsl"),
        str(LAYERS / "return_impl_refines.fsl"),
        depth=8,
    )
    assert impl["result"] == "refines"

    scenarios = run_scenarios(str(LAYERS / "return_system.fsl"), 8)
    assert scenarios["result"] == "scenarios"
    assert any(s["name"] == "acceptance_AC-1" for s in scenarios["scenarios"])
