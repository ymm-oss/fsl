from pathlib import Path

from fslc.cli import run_check, run_scenarios, run_verify


ROOT = Path(__file__).resolve().parents[1]


OLD_RETURN_POLICY_SRC = r'''spec ReturnPolicy {
  type CaseId = 0..2
  enum Stage { Requested, Approved, Rejected, Refunded }
  state { cases: Map<CaseId, Stage>, refunded: Int }
  init { forall c: CaseId { cases[c] = Requested }  refunded = 0 }

  fair action approve(c: CaseId) { requires cases[c] == Requested  cases[c] = Approved }
  fair action reject(c: CaseId)  { requires cases[c] == Requested  cases[c] = Rejected }
  fair action refund(c: CaseId)  {
    requires cases[c] == Approved
    cases[c] = Refunded
    refunded = refunded + 1
  }

  // policy: the refund count always matches the number of Refunded cases (accounting consistency)
  invariant RefundLedgerConsistent {
    refunded == count(c: CaseId where cases[c] == Refunded)
  }
  // policy (responsiveness): every request is eventually decided
  leadsTo EveryRequestDecided {
    forall c: CaseId {
      cases[c] == Requested ~> (cases[c] == Approved or cases[c] == Rejected or cases[c] == Refunded)
    }
  }
  reachable AllSettled { forall c: CaseId { cases[c] == Refunded or cases[c] == Rejected } }
}
'''


REQ_SRC = r'''requirements ReturnSystemReq {
  implements ReturnPolicy from "return_policy.fsl" {
    map cases[c: CaseId] =
      if sys[c].st == New then Requested
      else if sys[c].st == MgrQueue then Requested
      else if sys[c].st == AutoApproved then Approved
      else if sys[c].st == MgrApproved then Approved
      else if sys[c].st == MgrRejected then Rejected
      else Refunded
    map refunded = paid_count
  }

  type CaseId = 0..2
  type Amount = 0..3
  const AUTO_LIMIT = 1
  enum SSt { New, AutoApproved, MgrQueue, MgrApproved, MgrRejected, Paid }
  struct RCase { st: SSt, amount: Amount }
  state { sys: Map<CaseId, RCase>, paid_count: Int }
  init { forall c: CaseId { sys[c] = RCase { st: New, amount: 0 } }  paid_count = 0 }

  requirement REQ-1 "small returns are auto approved" {
    fair action submit(c: CaseId, a: Amount) {
      requires sys[c].st == New
      requires a > 0
      branches {
        when a <= AUTO_LIMIT { sys[c] = RCase { st: AutoApproved, amount: a } } maps approve(c)
        when a > AUTO_LIMIT { sys[c] = RCase { st: MgrQueue, amount: a } } maps stutter
      }
    }
  }

  requirement REQ-2 "payment only follows approval" {
    fair action mgr_approve(c: CaseId) maps approve(c) { requires sys[c].st == MgrQueue  sys[c].st = MgrApproved }
    fair action mgr_reject(c: CaseId) maps reject(c) { requires sys[c].st == MgrQueue  sys[c].st = MgrRejected }
    fair action pay(c: CaseId) maps refund(c) {
      requires sys[c].st == AutoApproved or sys[c].st == MgrApproved
      sys[c].st = Paid
      paid_count = paid_count + 1
    }
    invariant PaidLedger { paid_count == count(c: CaseId where sys[c].st == Paid) }
  }

  reachable AutoPaid { exists c: CaseId { sys[c].st == Paid } }

  acceptance AC-1 "small return can be paid" {
    submit(0, 1)
    pay(0)
    expect sys[0].st == Paid
  }
}
'''


def _write_req(tmp_path, src=REQ_SRC):
    (tmp_path / "return_policy.fsl").write_text(OLD_RETURN_POLICY_SRC, encoding="utf-8")
    req = tmp_path / "return_req.fsl"
    req.write_text(src, encoding="utf-8")
    return req


def test_bug18_keyword_prefix_identifiers_check_and_verify(tmp_path):
    src = r'''spec PrefixIdentifiers {
  type K = 0..1
  state { notify: Seq<K,3>, order_total: Int, iffy: Int, android: Int, fairness: Int }
  init { notify = Seq {} order_total = 0 iffy = 0 android = 0 fairness = 0 }
  action noop() {
    requires notify.size() > 0 or order_total == 0 or iffy == 0 or android == 0 or fairness == 0
  }
  invariant NamesStayPut {
    notify.size() == 0 and order_total == 0 and iffy == 0 and android == 0 and fairness == 0
  }
}
'''
    path = tmp_path / "prefix.fsl"
    path.write_text(src, encoding="utf-8")

    assert run_check(str(path))["result"] == "ok"
    assert run_verify(str(path), 3, "warn")["result"] == "verified"


def test_req_dialect_check_verify_and_induction_with_implements(tmp_path):
    req = _write_req(tmp_path)

    checked = run_check(str(req))
    assert checked["result"] == "ok"
    assert checked["implements"] == {"abs": "ReturnPolicy", "result": "refines"}

    verified = run_verify(str(req), 8, "warn")
    assert verified["result"] == "verified"
    assert verified["implements"]["result"] == "refines"

    proved = run_verify(str(req), 8, "warn", engine="induction")
    assert proved["result"] == "proved"
    assert proved["implements"]["result"] == "refines"


def test_req_dialect_branch_display_names_in_coverage(tmp_path):
    req = _write_req(tmp_path)
    proved = run_verify(str(req), 8, "warn", engine="induction")

    assert "submit[a <= AUTO_LIMIT]" in proved["action_coverage"]
    assert "submit[a > AUTO_LIMIT]" in proved["action_coverage"]


def test_req_dialect_violation_carries_requirement_meta(tmp_path):
    bad = REQ_SRC.replace(
        "invariant PaidLedger { paid_count == count(c: CaseId where sys[c].st == Paid) }",
        "invariant PaidLedger { paid_count == 0 }",
    ).replace(
        '''  acceptance AC-1 "small return can be paid" {
    submit(0, 1)
    pay(0)
    expect sys[0].st == Paid
  }
''',
        "",
    )
    req = _write_req(tmp_path, bad)

    result = run_verify(str(req), 8, "warn")

    assert result["result"] == "violated"
    assert result["requirement"] == {
        "id": "REQ-2",
        "text": "payment only follows approval",
    }


def test_req_dialect_acceptance_scenario_is_emitted(tmp_path):
    req = _write_req(tmp_path)
    result = run_scenarios(str(req), 8)

    assert result["result"] == "scenarios"
    acceptance = [s for s in result["scenarios"] if s["kind"] == "acceptance"]
    assert acceptance
    assert acceptance[0]["acceptance"] == "AC-1"
    assert acceptance[0]["requirement"] == {
        "id": "AC-1",
        "text": "small return can be paid",
    }


def test_req_dialect_acceptance_expect_failure_is_check_error(tmp_path):
    bad = REQ_SRC.replace("expect sys[0].st == Paid", "expect sys[0].st == New")
    req = _write_req(tmp_path, bad)

    result = run_check(str(req))

    assert result["result"] == "error"
    assert result["kind"] == "acceptance"
    assert result["id"] == "AC-1"
    assert result["failed_step"] == 2


def test_req_dialect_broken_action_map_reports_implements_failure(tmp_path):
    bad = REQ_SRC.replace(
        "fair action pay(c: CaseId) maps refund(c)",
        "fair action pay(c: CaseId) maps stutter",
    )
    req = _write_req(tmp_path, bad)

    result = run_verify(str(req), 8, "warn")

    assert result["result"] == "verified"
    assert result["implements"]["abs"] == "ReturnPolicy"
    assert result["implements"]["result"] == "refinement_failed"
    assert result["implements"]["violation"]["impl_action"]["requirement"] == {
        "id": "REQ-2",
        "text": "payment only follows approval",
    }
