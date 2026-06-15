import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import exit_code, run_explain
from fslc.explain import explain_file


ROOT = Path(__file__).resolve().parents[1]
SPECS = ROOT / "specs"
EXAMPLES = ROOT / "examples"


def _by_name(items):
    return {item["name"]: item for item in items}


def _counterfactual(out, invariant):
    for item in out["counterfactuals"]:
        if item["invariant"] == invariant:
            return item
    raise AssertionError(f"missing counterfactual for {invariant}")


def test_cart_v1_skeleton_lists_actions_properties_auto_checks_and_tags():
    out = explain_file(str(SPECS / "cart_v1.fsl"), depth=4)
    assert out["result"] == "explained"

    skeleton = out["skeleton"]
    assert set(skeleton["state"]) == {"stock", "cart"}

    actions = _by_name(skeleton["actions"])
    assert set(actions) == {"add_to_cart", "remove_from_cart", "checkout"}
    assert actions["add_to_cart"]["writes"] == ["cart"]
    assert actions["add_to_cart"]["requires_text"] == ["requires cart[u] == none"]
    assert actions["checkout"]["writes"] == ["cart", "stock"]
    assert actions["checkout"]["requires_text"] == [
        "requires cart[u] is some(i)",
        "requires stock[i] > 0",
    ]
    assert actions["checkout"]["ensures_text"] == [
        "ensures stock[i] == old(stock[i]) - 1",
    ]
    assert all("requirement" in action for action in skeleton["actions"])

    properties = _by_name(skeleton["properties"])
    assert properties["SoldOut"]["kind"] == "reachable"
    assert properties["SoldOut"]["body_text"] == "reachable SoldOut {"
    assert properties["SoldOut"]["requirement"] is None

    checks = {(check["kind"], check["target"]) for check in skeleton["auto_checks"]}
    assert ("type_bound", "stock") in checks
    assert ("type_bound", "cart") in checks


def test_order_workflow_shipped_was_paid_counterfactual_is_ship_assignment_removal():
    out = explain_file(str(SPECS / "order_workflow.fsl"), depth=6)
    cf = _counterfactual(out, "ShippedWasPaid")
    assert cf["weakening"]["op"] == "assignment-removal"
    assert cf["weakening"]["target"] == "ship assignment"
    assert cf["weakening"]["source_text"] == "orders[o].status = Shipped"
    assert cf["trace"]
    assert cf["violation"]["last_action"]["name"] == "ship"


def test_order_workflow_non_negative_revenue_has_graceful_no_counterfactual():
    out = explain_file(str(SPECS / "order_workflow.fsl"), depth=6)
    cf = _counterfactual(out, "NonNegativeRevenue")
    assert cf["weakening"] is None
    assert cf["trace"] is None
    assert cf["note"] == "no counterfactual within depth 6"


def test_cancel_flow_dialect_carries_requirement_text_in_skeleton_and_witnesses():
    out = explain_file(str(EXAMPLES / "pm" / "cancel_flow.fsl"), depth=4)
    props = _by_name(out["skeleton"]["properties"])
    assert props["POL-1"]["requirement"] == {
        "id": "POL-1",
        "text": "A cancellation request must always be met with a retention offer",
    }
    assert "policy POL-1" in props["POL-1"]["body_text"]

    actions = _by_name(out["skeleton"]["actions"])
    assert actions["request_cancel"]["actor"] == "Customer"
    assert actions["request_cancel"]["requires_text"] == [
        "transition request_cancel Active          -> CancelRequested by Customer"
    ]

    requirements = [w["requirement"] for w in out["witnesses"] if w.get("requirement")]
    assert props["POL-1"]["requirement"] in requirements
    assert props["CanRetain"]["requirement"] in requirements


def test_compose_spec_source_fallback_does_not_crash():
    out = explain_file(str(SPECS / "bank_system.fsl"), depth=2)
    assert out["result"] == "explained"
    assert out["skeleton"]["actions"]
    assert out["skeleton"]["properties"]
    bank_settle = _by_name(out["skeleton"]["actions"])["bank.settle"]
    assert bank_settle["requires_text"] == [
        "source unavailable; using name/structure (component-origin or generated element)"
    ]


def test_explain_cli_exit_zero_for_valid_specs():
    proc = subprocess.run(
        [sys.executable, "-m", "fslc", "explain", str(SPECS / "cart_v1.fsl"), "--depth", "4"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.returncode == 0, proc.stderr
    assert json.loads(proc.stdout)["result"] == "explained"

    for path in [SPECS / "order_workflow.fsl", EXAMPLES / "pm" / "cancel_flow.fsl"]:
        out = run_explain(str(path), depth=4)
        assert out["result"] == "explained"
        assert exit_code(out) == 0


def test_explain_json_has_no_internal_double_underscore_names():
    out = explain_file(str(SPECS / "bank_system.fsl"), depth=2)
    blob = json.dumps(out, ensure_ascii=False)
    assert "__" not in blob


def test_explain_output_is_json_serializable():
    for path in [
        SPECS / "cart_v1.fsl",
        SPECS / "order_workflow.fsl",
        EXAMPLES / "pm" / "cancel_flow.fsl",
        SPECS / "bank_system.fsl",
    ]:
        out = explain_file(str(path), depth=2)
        json.dumps(out, ensure_ascii=False)
