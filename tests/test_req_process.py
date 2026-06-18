from fslc.cli import run_check, run_verify


BIZ_SRC = r'''business ClaimBiz {
  actor Employee, System, Manager, Finance
  entity Claim

  process Claim {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit Draft -> Submitted by Employee
    transition auto_approve Submitted -> Approved by System
    transition mgr_approve Submitted -> Approved by Manager
    transition reject Submitted -> Rejected by Manager
    transition pay Approved -> Paid by Finance
  }
}
verify {
  instances Claim = 3
}
'''


REQ_SRC = r'''requirements ClaimReq {
  implements ClaimBiz from "biz.fsl" {
  }

  entity Claim
  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit Draft -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-1 "employee submits an amount"
    transition auto_approve Submitted -> Approved by System when amount <= AUTO_LIMIT
    transition mgr_approve Submitted -> Approved by Manager when amount > AUTO_LIMIT
    transition reject Submitted -> Rejected by Manager when amount > AUTO_LIMIT
    transition pay Approved -> Paid by Finance
  }

  acceptance AC-1 "low amount is auto-approved and paid" {
    submit(0, 1)
    auto_approve(0)
    pay(0)
    expect Claim 0 in Paid
  }

  forbidden FB-1 "large amount cannot be auto-approved" {
    submit(1, 2)
    auto_approve(1)
    expect rejected
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
'''


def _write_pair(tmp_path, req_src=REQ_SRC):
    (tmp_path / "biz.fsl").write_text(BIZ_SRC, encoding="utf-8")
    req = tmp_path / "req.fsl"
    req.write_text(req_src, encoding="utf-8")
    return req


def test_requirements_process_data_profile_check_verify_and_refine(tmp_path):
    req = _write_pair(tmp_path)

    checked = run_check(str(req))
    assert checked["result"] == "ok", checked
    assert checked["implements"] == {"abs": "ClaimBiz", "result": "refines"}

    verified = run_verify(str(req), 6, "ignore")
    assert verified["result"] == "verified", verified
    assert verified["implements"]["result"] == "refines"


def test_requirements_process_guard_bites_via_forbidden(tmp_path):
    bad = REQ_SRC.replace(
        "transition auto_approve Submitted -> Approved by System when amount <= AUTO_LIMIT",
        "transition auto_approve Submitted -> Approved by System",
    )
    req = _write_pair(tmp_path, bad)

    result = run_check(str(req))

    assert result["result"] == "error", result
    assert result["kind"] == "forbidden"
    assert result["id"] == "FB-1"


def test_requirements_process_stage_expect_failure_is_acceptance_error(tmp_path):
    bad = REQ_SRC.replace("expect Claim 0 in Paid", "expect Claim 0 in Rejected")
    req = _write_pair(tmp_path, bad)

    result = run_check(str(req))

    assert result["result"] == "error", result
    assert result["kind"] == "acceptance"
    assert result["id"] == "AC-1"


def test_business_process_rejects_requirements_data_extensions(tmp_path):
    path = tmp_path / "bad_biz.fsl"
    path.write_text(
        r'''business BadBiz {
  actor Employee
  entity Claim
  process Claim with amount: Claim {
    stages Draft, Submitted
    initial Draft
    transition submit Draft -> Submitted by Employee with a: Claim when a == 0 set amount = a
  }
}
verify {
  instances Claim = 1
}
''',
        encoding="utf-8",
    )

    result = run_check(str(path))

    assert result["result"] == "error", result
    assert result["kind"] == "type"
    assert "requirements-layer feature" in result["message"]
