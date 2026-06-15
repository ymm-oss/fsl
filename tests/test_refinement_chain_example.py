"""examples/refinement_chain — refine chain mode (mapping composition).

Listing adjacent (spec, mapping) pairs lets you check end-to-end fidelity as a
composition. Bounded refinement is transitive at the same depth, so the
composed check is equivalent to every adjacent link holding
(DESIGN-refinement §7).
"""
from pathlib import Path

from fslc.cli import run_refine


ROOT = Path(__file__).resolve().parents[1]
E = ROOT / "examples/refinement_chain"


def test_adjacent_links_each_refine():
    assert run_refine(
        str(E / "bot.fsl"), str(E / "mid.fsl"), str(E / "bot_refines_mid.fsl"), depth=6
    )["result"] == "refines"
    assert run_refine(
        str(E / "mid.fsl"), str(E / "top.fsl"), str(E / "mid_refines_top.fsl"), depth=6
    )["result"] == "refines"


def test_chain_refines_end_to_end_with_composed_action_map():
    r = run_refine(
        str(E / "bot.fsl"), str(E / "mid.fsl"), str(E / "bot_refines_mid.fsl"),
        depth=6,
        rest=[str(E / "top.fsl"), str(E / "mid_refines_top.fsl")],
    )
    assert r["result"] == "refines"
    assert r["impl"] == "ChainBot"
    assert r["abs"] == "ChainTop"
    assert r["chain"] == ["ChainBot", "ChainMid", "ChainTop"]
    # composed: audit/start_review are stutter at the top level, finish maps to finish
    assert r["action_map"] == {
        "start_review": "stutter", "audit": "stutter", "finish": "finish",
    }


def test_chain_pinpoints_first_broken_link(tmp_path):
    # Break bot_refines_mid: mapping audit (an internal stutter) to mid.finish
    # leaves α(BAudit)=MReview while finish expects MDone → bot⊒mid breaks.
    broken = (E / "bot_refines_mid.fsl").read_text(encoding="utf-8").replace(
        "action audit(c)        -> stutter", "action audit(c)        -> finish(c)")
    bad = tmp_path / "bot_refines_mid_bad.fsl"
    bad.write_text(broken, encoding="utf-8")
    r = run_refine(
        str(E / "bot.fsl"), str(E / "mid.fsl"), str(bad),
        depth=6,
        rest=[str(E / "top.fsl"), str(E / "mid_refines_top.fsl")],
    )
    assert r["result"] == "refinement_failed"
    assert r["failed_link"] == {
        "from": "ChainBot", "to": "ChainMid", "kind": r["failed_link"]["kind"]}
    assert r["failed_link"]["kind"] in ("abs_state_mismatch", "abs_requires_failed")
