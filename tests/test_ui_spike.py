"""fsl-ui スパイク(issue #9)の資産が検証を通ること、および
0引数 abstract アクション写像のバグ修正(F-UI-1)の回帰テスト。"""
from pathlib import Path

from fslc.cli import run_verify
from fslc.parser import parse_refinement, parse_src
from fslc.model import build_spec
from fslc.refine import build_refinement, refine

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "examples" / "ui_spike"


def test_return_ui_screen_flow_proves():
    # 素の fsl が画面フローを表現する: 全画面到達・袋小路なし・二重送信防止。
    out = run_verify(str(UI / "return_ui.fsl"), 8, "warn")
    assert out["result"] == "verified"
    assert set(out["reachables"]) == {"CanDone", "CanError", "CanMgr"}
    assert "SubmitResolves" in (out.get("leads_to") or {})
    proved = run_verify(str(UI / "return_ui.fsl"), 8, "warn", engine="induction")
    assert proved["result"] == "proved"


def test_ui_flow_refines_into_requirements():
    # UI フロー(impl)が要件エッセンス(abs)を refine する。
    # pay() -> pay() を含むため F-UI-1(0引数 abstract 写像)の回帰でもある。
    impl_ast, impl_dn = parse_src((UI / "return_ui.fsl").read_text(), str(UI))
    abs_ast, abs_dn = parse_src((UI / "return_req_min.fsl").read_text(), str(UI))
    impl = build_spec(impl_ast, impl_dn)
    abs_spec = build_spec(abs_ast, abs_dn)
    mapping = build_refinement(
        parse_refinement((UI / "ui_refines_req.fsl").read_text()), impl, abs_spec)
    result = refine(impl, abs_spec, mapping, 8)
    assert result["result"] == "refines"
    assert result["action_map"]["pay"] == "pay"        # 0引数→0引数 写像が成立
    assert result["action_map"]["enter_amount"] == "stutter"


def test_zero_arg_abstract_action_mapping_parses():
    # F-UI-1 回帰: 空括弧 abstract ターゲット foo() が引数0個として解釈される。
    ref = parse_refinement(
        "refinement R { impl I abs A action step() -> done() }")
    _, _name, items = ref
    action_maps = [it for it in items if it[0] == "action_map"]
    assert action_maps, "action_map item expected"
    _tag, _name, _params, target, _loc = action_maps[0]
    assert target == ("action", "done", [])           # [None] でなく [] であること


def test_navstack_back_stack_idiom_verifies():
    # back stack を Map<Depth,Screen> + depth で表現(Seq=FIFO は不向き)。
    out = run_verify(str(UI / "navstack.fsl"), 8, "ignore")
    assert out["result"] == "verified"
    assert out["action_coverage"]["back"] is True
