from fslc import build_spec, parse
from fslc.cli import run_check, run_verify


CONTROLLED_BUSINESS = r'''business RefundFlow {
  actor Support, Finance
  entity Refund

  process Refund {
    stages Requested, Approved, Paid
    initial Requested
    transition approve Requested -> Approved by Support
    transition pay Approved -> Paid by Finance
  }

  control CTRL-NO-UNREVIEWED-PAYMENT
    "Refund payment must preserve review control"
    owner Finance
    severity high
    applies_to Refund

  policy POL-APPROVAL-TO-PAY
    "Approved refunds must eventually be paid"
    satisfies CTRL-NO-UNREVIEWED-PAYMENT
    every Refund in Approved must eventually be Paid

  goal G-CAN-PAY
    "A valid refund can complete"
    some Refund can reach Paid
}
verify {
  instances Refund = 2
}
'''


def test_business_controls_are_metadata_and_satisfaction_is_checked(tmp_path):
    path = tmp_path / "refund.fsl"
    path.write_text(CONTROLLED_BUSINESS, encoding="utf-8")

    checked = run_check(str(path))
    assert checked["result"] == "ok", checked
    assert checked["warnings"] == []

    spec = build_spec(parse(CONTROLLED_BUSINESS))
    assert spec["controls"]["controls"][0]["id"] == "CTRL-NO-UNREVIEWED-PAYMENT"
    assert spec["controls"]["controls"][0]["owner"] == "Finance"
    assert spec["controls"]["satisfies"] == [
        {
            "element": "policy",
            "id": "POL-APPROVAL-TO-PAY",
            "control": "CTRL-NO-UNREVIEWED-PAYMENT",
            "loc": {"line": 18, "column": 3},
        }
    ]


def test_business_policy_violation_carries_satisfied_control(tmp_path):
    bad = CONTROLLED_BUSINESS.replace(
        '''policy POL-APPROVAL-TO-PAY
    "Approved refunds must eventually be paid"
    satisfies CTRL-NO-UNREVIEWED-PAYMENT
    every Refund in Approved must eventually be Paid''',
        '''policy POL-APPROVAL-TO-PAY
    "Approved refunds must never be paid"
    satisfies CTRL-NO-UNREVIEWED-PAYMENT
    invariant { forall r: Refund { stage(r) != Paid } }''',
    )
    path = tmp_path / "refund_bad.fsl"
    path.write_text(bad, encoding="utf-8")

    result = run_verify(str(path), 3, "ignore")

    assert result["result"] == "violated", result
    assert result["requirement"] == {
        "id": "POL-APPROVAL-TO-PAY",
        "text": "Approved refunds must never be paid",
        "controls": [
            {
                "id": "CTRL-NO-UNREVIEWED-PAYMENT",
                "text": "Refund payment must preserve review control",
            }
        ],
    }


def test_business_satisfies_unknown_control_is_type_error(tmp_path):
    src = CONTROLLED_BUSINESS.replace(
        "satisfies CTRL-NO-UNREVIEWED-PAYMENT",
        "satisfies CTRL-MISSING",
    )
    path = tmp_path / "refund_unknown_control.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "error", result
    assert result["kind"] == "type"
    assert "satisfies unknown control 'CTRL-MISSING'" in result["message"]


def test_governance_delegate_uses_business_satisfies_metadata(tmp_path):
    (tmp_path / "refund.fsl").write_text(CONTROLLED_BUSINESS, encoding="utf-8")
    gov = tmp_path / "governance.fsl"
    gov.write_text(
        r'''governance EnterpriseRefundControls {
  authority Finance owns CTRL-NO-UNREVIEWED-PAYMENT
  control CTRL-NO-UNREVIEWED-PAYMENT
    "Refund payment must preserve review control"
    owner Finance
    severity high
    applies_to Refund

  delegates RefundFlow from "refund.fsl" {
    require CTRL-NO-UNREVIEWED-PAYMENT
  }
}
''',
        encoding="utf-8",
    )

    result = run_check(str(gov))

    assert result["result"] == "ok", result
    delegate = result["governance"]["delegates"][0]
    assert delegate["business"] == "RefundFlow"
    assert delegate["required"] == ["CTRL-NO-UNREVIEWED-PAYMENT"]
    assert delegate["satisfied"] == {
        "CTRL-NO-UNREVIEWED-PAYMENT": [
            {"kind": "policy", "id": "POL-APPROVAL-TO-PAY"}
        ]
    }


def test_governance_delegate_requires_satisfied_control(tmp_path):
    business = CONTROLLED_BUSINESS.replace(
        "satisfies CTRL-NO-UNREVIEWED-PAYMENT\n    ",
        "",
    )
    (tmp_path / "refund.fsl").write_text(business, encoding="utf-8")
    gov = tmp_path / "governance.fsl"
    gov.write_text(
        r'''governance EnterpriseRefundControls {
  control CTRL-NO-UNREVIEWED-PAYMENT "Refund payment must preserve review control"

  delegates RefundFlow from "refund.fsl" {
    require CTRL-NO-UNREVIEWED-PAYMENT
  }
}
''',
        encoding="utf-8",
    )

    result = run_check(str(gov))

    assert result["result"] == "error", result
    assert result["kind"] == "type"
    assert "requires unsatisfied control(s): CTRL-NO-UNREVIEWED-PAYMENT" in result["message"]


def test_governance_preservation_runs_refinement(tmp_path):
    asis = r'''business AsIsRefund {
  actor Clerk
  entity Refund
  process Refund {
    stages Requested, Done
    initial Requested
    transition complete Requested -> Done by Clerk
  }
  control CTRL-COMPLETE "Requests must be completed"
  policy POL-COMPLETE "Every request completes" satisfies CTRL-COMPLETE
    every Refund in Requested must eventually be Done
}
verify { instances Refund = 2 }
'''
    tobe = asis.replace("AsIsRefund", "ToBeRefund")
    mapping = r'''refinement ToBePreservesAsIs {
  impl ToBeRefund
  abs AsIsRefund
  maps auto
}
'''
    (tmp_path / "asis.fsl").write_text(asis, encoding="utf-8")
    (tmp_path / "tobe.fsl").write_text(tobe, encoding="utf-8")
    (tmp_path / "preserves.fsl").write_text(mapping, encoding="utf-8")
    gov = tmp_path / "governance.fsl"
    gov.write_text(
        r'''governance RefundTransformationControls {
  control CTRL-COMPLETE "Requests must be completed"

  preservation SameFlow {
    before AsIsRefund from "asis.fsl"
    after ToBeRefund from "tobe.fsl"
    preserve CTRL-COMPLETE
    checked_by refinement "preserves.fsl"
  }
}
''',
        encoding="utf-8",
    )

    result = run_check(str(gov))

    assert result["result"] == "ok", result
    assert result["governance"]["preservations"] == [
        {
            "name": "SameFlow",
            "before": "AsIsRefund",
            "after": "ToBeRefund",
            "preserve": ["CTRL-COMPLETE"],
            "result": "refines",
        }
    ]
