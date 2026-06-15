"""The fsl-ui spike (issue #9) artifacts pass verification, plus a regression
test for the zero-arg abstract action mapping bug fix (F-UI-1)."""
from pathlib import Path

from fslc.cli import run_verify
from fslc.parser import parse_refinement, parse_src
from fslc.model import build_spec
from fslc.refine import build_refinement, refine

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "examples" / "ui_spike"


def test_return_ui_screen_flow_proves():
    # plain fsl expresses the screen flow: all screens reachable, no dead ends, no double submit.
    out = run_verify(str(UI / "return_ui.fsl"), 8, "warn")
    assert out["result"] == "verified"
    assert set(out["reachables"]) == {"CanDone", "CanError", "CanMgr"}
    assert "SubmitResolves" in (out.get("leads_to") or {})
    proved = run_verify(str(UI / "return_ui.fsl"), 8, "warn", engine="induction")
    assert proved["result"] == "proved"


def test_ui_flow_refines_into_requirements():
    # the UI flow (impl) refines the requirement essence (abs).
    # Since it includes pay() -> pay(), it is also a F-UI-1 (zero-arg abstract mapping) regression.
    impl_ast, impl_dn = parse_src((UI / "return_ui.fsl").read_text(), str(UI))
    abs_ast, abs_dn = parse_src((UI / "return_req_min.fsl").read_text(), str(UI))
    impl = build_spec(impl_ast, impl_dn)
    abs_spec = build_spec(abs_ast, abs_dn)
    mapping = build_refinement(
        parse_refinement((UI / "ui_refines_req.fsl").read_text()), impl, abs_spec)
    result = refine(impl, abs_spec, mapping, 8)
    assert result["result"] == "refines"
    assert result["action_map"]["pay"] == "pay"        # zero-arg -> zero-arg mapping holds
    assert result["action_map"]["enter_amount"] == "stutter"


def test_zero_arg_abstract_action_mapping_parses():
    # F-UI-1 regression: empty-paren abstract target foo() is interpreted as zero args.
    ref = parse_refinement(
        "refinement R { impl I abs A action step() -> done() }")
    _, _name, items = ref
    action_maps = [it for it in items if it[0] == "action_map"]
    assert action_maps, "action_map item expected"
    _tag, _name, _params, target, _loc = action_maps[0]
    assert target == ("action", "done", [])           # must be [], not [None]


def test_navstack_back_stack_idiom_verifies():
    # represent the back stack as Map<Depth,Screen> + depth (Seq=FIFO is a poor fit).
    out = run_verify(str(UI / "navstack.fsl"), 8, "ignore")
    assert out["result"] == "verified"
    assert out["action_coverage"]["back"] is True
