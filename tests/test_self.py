"""FSL dogfooding itself: specs/repair_loop.fsl models fslc's own
write -> verify -> repair workflow. These tests pin the demonstration —
including the CTI -> auxiliary-invariant lesson the spec is about."""
from pathlib import Path

from fslc import parse, build_spec, verify, prove

SPECS = Path(__file__).resolve().parent.parent / "specs"


def test_repair_loop_verifies_and_reaches_proved():
    spec = build_spec(parse((SPECS / "repair_loop.fsl").read_text(encoding="utf-8")))
    r = verify(spec, 10, deadlock_mode="ignore")
    assert r["result"] == "verified"
    assert r["reachables"]["ReachProved"]["witnessed_at_step"] == 3
    # the CTI -> repair -> proved cycle is reachable (matches DOGFOOD experience)
    assert r["reachables"]["RepairedThenProved"]["witnessed_at_step"] >= 1


def test_repair_loop_proves_with_aux_invariant():
    spec = build_spec(parse((SPECS / "repair_loop.fsl").read_text(encoding="utf-8")))
    r = prove(spec, 1, 10, deadlock_mode="ignore")
    assert r["result"] == "proved"
    assert "VerifiedImpliesEver" in r["k_used"]


def test_repair_loop_without_aux_yields_cti():
    """Drop the auxiliary invariant -> ProvedWasVerified is no longer
    1-inductive, and the CTI points at the unreachable ghost state
    {Verified, ever_verified=false}. This is the lesson the spec encodes."""
    src = (SPECS / "repair_loop.fsl").read_text(encoding="utf-8")
    stripped = "\n".join(
        line for line in src.splitlines()
        if "VerifiedImpliesEver" not in line
    )
    r = prove(build_spec(parse(stripped)), 1, 10, deadlock_mode="ignore")
    assert r["result"] == "unknown_cti"
    assert r["invariant"] == "ProvedWasVerified"
    start = r["cti"]["states"][0]["state"]
    assert start["status"] == "Verified"
    assert start["ever_verified"] is False
