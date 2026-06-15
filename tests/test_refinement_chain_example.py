"""examples/refinement_chain — refine 連鎖モード(写像合成)。

隣接 (spec, 写像) を並べると end-to-end の忠実性を合成検査できる。
有界 refinement は同一深さで推移的なので、合成検査は全隣接リンクが
成り立つことと等価(DESIGN-refinement §7)。
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
    # 合成済み: audit/start_review は最上位では stutter、finish は finish に対応
    assert r["action_map"] == {
        "start_review": "stutter", "audit": "stutter", "finish": "finish",
    }


def test_chain_pinpoints_first_broken_link(tmp_path):
    # bot_refines_mid を壊す: audit(内部 stutter) を mid.finish に対応させると
    # α(BAudit)=MReview のまま finish が MDone を期待 → bot⊒mid が破れる。
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
