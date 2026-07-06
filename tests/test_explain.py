import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import exit_code, run_explain
from fslc.explain import _expr_to_text, explain_file


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
    assert properties["SoldOut"]["body_text"] == "forall i: ItemId: stock[i] == 0"
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
    assert props["POL-1"]["body_text"] == (
        "forall c: Sub: sub_stage[c] == CancelRequested ~> sub_stage[c] == OfferShown"
    )

    actions = _by_name(out["skeleton"]["actions"])
    assert actions["request_cancel"]["actor"] == "Customer"
    assert actions["request_cancel"]["requires_text"] == [
        "requires sub_stage[c] == Active"
    ]

    skeleton = out["skeleton"]
    assert skeleton["kpis"] == [
        {"name": "churned", "entity": "Sub", "stage": "Churned"},
        {"name": "retained", "entity": "Sub", "stage": "Retained"},
    ]
    assert skeleton["domains"] == ["Sub: 3 instances (0..2)"]
    assert skeleton["enums"] == {
        "SubStage": ["Active", "CancelRequested", "OfferShown", "Retained", "Churned"],
    }
    flow = skeleton["stage_flows"][0]
    assert flow["state"] == "sub_stage"
    assert flow["stages"] == ["Active", "CancelRequested", "OfferShown", "Retained", "Churned"]
    assert {
        "action": "request_cancel", "from": "Active", "to": "CancelRequested", "actor": "Customer",
    } in flow["transitions"]

    requirements = [w["requirement"] for w in out["witnesses"] if w.get("requirement")]
    assert props["POL-1"]["requirement"] in requirements
    assert props["CanRetain"]["requirement"] in requirements


def test_compose_spec_source_fallback_does_not_crash():
    out = explain_file(str(SPECS / "bank_system.fsl"), depth=2)
    assert out["result"] == "explained"
    assert out["skeleton"]["actions"]
    assert out["skeleton"]["properties"]
    bank_settle = _by_name(out["skeleton"]["actions"])["bank.settle"]
    # Rendered from the AST rather than a matched source line, so a composed
    # (component-origin) action still gets a real, non-sentinel guard string.
    assert bank_settle["requires_text"] == ["requires bank.pending > 0"]


def test_deadline_invariant_and_generated_tick_are_tagged_generated():
    out = explain_file(str(EXAMPLES / "nfr" / "support_sla.fsl"), depth=2)
    skeleton = out["skeleton"]

    properties = _by_name(skeleton["properties"])
    deadline = next(p for p in properties.values() if p["name"].startswith("_deadline_"))
    assert deadline["generated"] is True
    assert deadline["body_text"] == "forall c: CaseId: resp_age[c] <= SLA_TICKS"

    actions = _by_name(skeleton["actions"])
    assert actions["tick"]["generated"] == {
        "kind": "time_tick", "urgent_actions": ("respond_due",),
    }
    # A hand-written action literally named `tick` (no time block involved) is
    # not generated — only the dialect-synthesized one is tagged.
    design = explain_file(str(EXAMPLES / "nfr" / "sla_worker_design.fsl"), depth=2)
    design_actions = _by_name(design["skeleton"]["actions"])
    assert "generated" not in design_actions["tick"]

    assert skeleton["enums"] == {"St": ["Waiting", "Accepted", "Responded"]}
    assert skeleton["domains"] == ["CaseId: 3 instances (0..2)"]
    assert skeleton["stage_flows"] == [{
        "state": "cases",
        "type": "St",
        "stages": ["Waiting", "Accepted", "Responded"],
        "transitions": [
            {"action": "accept", "from": "Waiting", "to": "Accepted"},
            {"action": "respond", "from": "Accepted", "to": "Responded"},
            {"action": "respond_due", "from": "Accepted", "to": "Responded"},
        ],
    }]


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


def test_explain_readable_requirements_surfaces_generated_review_context():
    out = run_explain(str(EXAMPLES / "e2e" / "2_requirements.fsl"), readable=True)

    assert out["result"] == "explained"
    assert exit_code(out) == 0
    text = out["readable"]
    assert "Spec: ExpenseRequirements (depth 8)" in text
    assert "Claim: 3 instances (0..2)" in text
    assert "Amount: values 0..3" in text
    assert "paid_claims = count of Claim in Paid" in text
    assert "submit(c: Claim, a: Amount) [fair] actor: Employee" in text
    assert "claim_stage[c] ↦ claim_stage[c]" in text
    assert "submit(c, a) ↦ submit(c)" in text


def test_explain_readable_cli_prints_plain_text():
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "explain",
            str(SPECS / "cart_v1.fsl"),
            "--depth",
            "4",
            "--readable",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.startswith("Spec: ShoppingCart")
    assert not proc.stdout.lstrip().startswith("{")


def test_explain_readable_branch_lowering_lists_split_actions(tmp_path):
    src = r'''requirements BranchReq {
  type CaseId = 0..2
  type Amount = 0..3
  const AUTO_LIMIT = 1
  enum S { New, Small, Big }
  state { st: Map<CaseId, S> }
  init { forall c: CaseId { st[c] = New } }

  fair action submit(c: CaseId, a: Amount) {
    requires st[c] == New
    branches {
      when a <= AUTO_LIMIT { st[c] = Small } maps stutter
      when a > AUTO_LIMIT { st[c] = Big } maps stutter
    }
  }

  reachable SmallReach { exists c: CaseId { st[c] == Small } }
}
'''
    path = tmp_path / "branch_req.fsl"
    path.write_text(src, encoding="utf-8")

    out = run_explain(str(path), depth=2, readable=True)

    assert out["result"] == "explained"
    assert "Branch lowering:" in out["readable"]
    assert "submit → submit[a <= AUTO_LIMIT], submit[a > AUTO_LIMIT]" in out["readable"]


def test_explain_default_json_shape_does_not_include_readable_text():
    out = run_explain(str(SPECS / "cart_v1.fsl"), depth=4)

    assert out["result"] == "explained"
    assert "skeleton" in out
    assert "counterfactuals" in out
    assert "readable" not in out


def test_explain_json_has_no_internal_double_underscore_names():
    out = explain_file(str(SPECS / "bank_system.fsl"), depth=2)
    blob = json.dumps(out, ensure_ascii=False)
    assert "__" not in blob
    violations = [
        item["violation"] for item in out["counterfactuals"]
        if item.get("violation") is not None
    ]
    assert violations
    assert all("internal_invariant" not in violation for violation in violations)
    assert any(
        violation.get("invariant") in {
            "bank.ClearedNonNegative",
            "audit.BalanceNonNegative",
        }
        for violation in violations
    )


def test_explain_output_is_json_serializable():
    for path in [
        SPECS / "cart_v1.fsl",
        SPECS / "order_workflow.fsl",
        EXAMPLES / "pm" / "cancel_flow.fsl",
        SPECS / "bank_system.fsl",
    ]:
        out = explain_file(str(path), depth=2)
        json.dumps(out, ensure_ascii=False)


def _var(name):
    return ("var", name)


def _bin(op, a, b):
    return ("bin", op, a, b)


def test_expr_to_text_parenthesizes_where_semantics_require_it():
    # not (A and B) must not render as "not A and B", which re-parses as
    # (not A) and B -- a different (inverted) formula.
    assert (
        _expr_to_text(("not", _bin("and", _var("pending"), _var("served"))))
        == "not (pending and served)"
    )
    # (A or B) and C: the "or" binds looser than "and", so the grouping must
    # survive.
    assert (
        _expr_to_text(_bin("and", _bin("or", _var("A"), _var("B")), _var("C")))
        == "(A or B) and C"
    )
    # a * (b + c): "+" binds looser than "*".
    assert (
        _expr_to_text(_bin("*", _var("a"), _bin("+", _var("b"), _var("c"))))
        == "a * (b + c)"
    )
    # a - (b - c) vs a - b - c: "-" is left-associative but not associative,
    # so an explicitly-grouped right operand must keep its parens.
    assert (
        _expr_to_text(_bin("-", _var("a"), _bin("-", _var("b"), _var("c"))))
        == "a - (b - c)"
    )
    assert (
        _expr_to_text(_bin("-", _bin("-", _var("a"), _var("b")), _var("c")))
        == "a - b - c"
    )


def test_expr_to_text_omits_parens_not_required_by_precedence():
    # "and" binds tighter than "or", so a left-nested `(A and B) or C` needs
    # no parens around the "and" -- this is the natural, unparenthesized parse.
    assert (
        _expr_to_text(_bin("or", _bin("and", _var("A"), _var("B")), _var("C")))
        == "A and B or C"
    )
