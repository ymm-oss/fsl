from pathlib import Path

from fslc.cli import run_check, run_refine, run_verify


ROOT = Path(__file__).resolve().parents[1]


BIZ_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  case Return = 0..2

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  kpi refunded counts Return in Refunded

  policy PAY-1 "refund count is consistent" invariant {
    refunded == count(c: Return where stage(c) == Refunded)
  }
  policy PAY-2 "requests are eventually decided" responds {
    forall c: Return {
      stage(c) == Requested ~> (stage(c) == Approved or stage(c) == Rejected or stage(c) == Refunded)
    }
  }
  goal AllSettled "all returns can settle" {
    forall c: Return { stage(c) == Refunded or stage(c) == Rejected }
  }
}
'''


def _write_biz(tmp_path, src=BIZ_SRC, name="return_biz.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_biz_dialect_check_verify_and_induction(tmp_path):
    biz = _write_biz(tmp_path)

    checked = run_check(str(biz))
    assert checked["result"] == "ok"

    verified = run_verify(str(biz), 8, "warn")
    assert verified["result"] == "verified"
    assert "_kpi_refunded" in verified["invariants_checked"]
    assert "AllSettled" in verified["reachables"]
    assert verified["reachables"]["AllSettled"]["witnessed_at_step"] >= 0

    proved = run_verify(str(biz), 8, "ignore", engine="induction")
    assert proved["result"] == "proved"
    assert "_kpi_refunded" in proved["invariants_checked"]


def test_biz_policy_violation_carries_policy_requirement_meta(tmp_path):
    bad = BIZ_SRC.replace(
        "refunded == count(c: Return where stage(c) == Refunded)",
        "refunded == 0",
    )
    biz = _write_biz(tmp_path, bad)

    result = run_verify(str(biz), 8, "warn")

    assert result["result"] == "violated"
    assert result["violation_kind"] == "invariant"
    assert result["invariant"] == "PAY-1"
    assert result["requirement"] == {
        "id": "PAY-1",
        "text": "refund count is consistent",
    }


def test_biz_kpi_auto_invariant_can_be_violated(tmp_path):
    src = r'''business BadKpi {
  actor Manager
  case Return = 0..1
  process Return {
    stages Refunded
    initial Refunded
    transition noop Refunded -> Refunded by Manager
  }
  kpi refunded counts Return in Refunded
}
'''
    biz = _write_biz(tmp_path, src)

    result = run_verify(str(biz), 3, "warn")

    assert result["result"] == "violated"
    assert result["violation_kind"] == "invariant"
    assert result["invariant"] == "_kpi_refunded"
    assert result["violated_at_step"] == 0


def test_biz_goal_has_reachable_witness(tmp_path):
    biz = _write_biz(tmp_path)

    result = run_verify(str(biz), 8, "warn")

    witness = result["reachables"]["AllSettled"]["witness"]
    assert witness[-1]["state"]["return_stage"] == {
        "0": "Rejected",
        "1": "Rejected",
        "2": "Rejected",
    }


def test_biz_undeclared_actor_is_type_error(tmp_path):
    bad = BIZ_SRC.replace("actor Customer, Manager", "actor Customer")
    biz = _write_biz(tmp_path, bad)

    result = run_check(str(biz))

    assert result["result"] == "error"
    assert result["kind"] == "type"
    assert "undeclared actor 'Manager'" in result["message"]


def test_biz_kpi_decrement_is_type_error(tmp_path):
    bad = BIZ_SRC.replace("kpi refunded counts Return in Refunded", "kpi requested counts Return in Requested")
    biz = _write_biz(tmp_path, bad)

    result = run_check(str(biz))

    assert result["result"] == "error"
    assert result["kind"] == "type"
    assert "decrement KPI is not supported" in result["message"]


def test_biz_stage_call_ambiguous_is_type_error(tmp_path):
    src = r'''business AmbiguousStage {
  actor Manager
  case Return = 0..1
  process Return {
    stages Requested, Approved
    initial Requested
    transition approve Requested -> Approved by Manager
  }
  process Return {
    stages Requested, Rejected
    initial Requested
    transition reject2 Requested -> Rejected by Manager
  }
  policy P-1 "ambiguous stage helper" invariant {
    forall c: Return { stage(c) == Requested }
  }
}
'''
    biz = _write_biz(tmp_path, src)

    result = run_check(str(biz))

    assert result["result"] == "error"
    assert result["kind"] == "type"
    assert "ambiguous" in result["message"]


def test_biz_policy_can_replace_handwritten_return_policy_for_refinement(tmp_path):
    abs_src = BIZ_SRC.replace("business ReturnHandling", "business ReturnPolicy")
    abs_file = _write_biz(tmp_path, abs_src, "return_policy_biz.fsl")
    mapping = tmp_path / "return_refines_biz.fsl"
    mapping.write_text(
        r'''refinement SystemRefinesBizPolicy {
  impl ReturnSystemReq
  abs  ReturnPolicy

  map return_stage[c: Return] =
    if sys[c].st == New then Requested
    else if sys[c].st == MgrQueue then Requested
    else if sys[c].st == AutoApproved then Approved
    else if sys[c].st == MgrApproved then Approved
    else if sys[c].st == MgrRejected then Rejected
    else Refunded
  map refunded = paid_count

  action submit__b1(c, a) -> approve(c)
  action submit__b2(c, a) -> stutter
  action mgr_approve(c)     -> approve(c)
  action mgr_reject(c)      -> reject(c)
  action pay(c)             -> refund(c)
}
''',
        encoding="utf-8",
    )

    result = run_refine(
        str(ROOT / "examples/layers/return_system.fsl"),
        str(abs_file),
        str(mapping),
        depth=6,
    )

    assert result["result"] == "refines", result
    assert result["abs"] == "ReturnPolicy"
