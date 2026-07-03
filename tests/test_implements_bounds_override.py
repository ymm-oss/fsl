# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""`fslc verify --instances`/`--values` overrides (#86) must propagate into an
inline-implements *abstract* spec (#94).

Refinement is a same-world-size forward simulation: `refine(impl, abs, ...)`
maps impl entity `c` to abstract entity `c`. A bounds override redefines the
impl's world size, but the abstract is parsed independently during the impl's
desugaring and, before this fix, kept its own (larger) size — so the identity
mapping referenced abstract indices the shrunken impl never produced and the
refinement failed with `map_out_of_bounds`. The overall `result` stayed
`verified` (the failure was confined to the `implements` sub-field), which is
exactly why it was easy to miss.

The fix propagates the override into the abstract, restricted to the
entity/number names the abstract itself declares (an impl-only carried number
like `Amount` has no abstract counterpart and must not reach it).
"""
from fslc.cli import run_verify

ABS_SRC = r'''business ClaimFlow {
  actor Employee, Manager
  entity Claim
  process Claim {
    stages Draft, Submitted, Approved
    initial Draft
    transition submit  Draft     -> Submitted by Employee
    transition approve Submitted -> Approved  by Manager
  }
}
verify {
  instances Claim = 3
}
'''

IMPL_SRC = r'''requirements ClaimReq {
  implements ClaimFlow from "abs.fsl" { }

  process Claim {
    stages Draft, Submitted, Approved
    initial Draft
    transition submit  Draft     -> Submitted by Employee
      covers REQ-1 "employee submits"
    transition approve Submitted -> Approved  by Manager
      covers REQ-2 "manager approves"
  }
}
verify {
  instances Claim = 3
}
'''

# Impl carries an amount the business abstract does not model — the override
# for it must apply to the impl only and must not error against the abstract.
IMPL_WITH_AMOUNT_SRC = r'''requirements ClaimReqAmount {
  implements ClaimFlow from "abs.fsl" { }

  number Amount

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved
    initial Draft
    transition submit  Draft     -> Submitted by Employee
      with a: Amount
      when a >= 0
      set amount = a
      covers REQ-1 "employee submits"
    transition approve Submitted -> Approved  by Manager
      covers REQ-2 "manager approves"
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
'''


def _pair(tmp_path, impl_src):
    (tmp_path / "abs.fsl").write_text(ABS_SRC, encoding="utf-8")
    impl = tmp_path / "impl.fsl"
    impl.write_text(impl_src, encoding="utf-8")
    return impl


def test_instances_override_propagates_to_abstract(tmp_path):
    # Before #94 this returned implements.result == "refinement_failed"
    # (kind "map_out_of_bounds"); the abstract must shrink to match.
    impl = _pair(tmp_path, IMPL_SRC)
    out = run_verify(str(impl), 6, "warn", instances=["Claim=1"])
    assert out["result"] == "verified"
    assert out["implements"]["result"] == "refines"
    assert out["bounds_overrides"] == {"instances": {"Claim": 1}, "values": {}}


def test_no_override_refinement_unchanged(tmp_path):
    impl = _pair(tmp_path, IMPL_SRC)
    out = run_verify(str(impl), 6, "warn")
    assert out["result"] == "verified"
    assert out["implements"]["result"] == "refines"
    assert "bounds_overrides" not in out


def test_impl_only_number_override_does_not_reach_abstract(tmp_path):
    # `Amount` exists only in the impl; overriding it must not error against
    # the abstract (which declares no `Amount`) and the refinement still holds.
    impl = _pair(tmp_path, IMPL_WITH_AMOUNT_SRC)
    out = run_verify(str(impl), 6, "warn", instances=["Claim=1"], values=["Amount=0..1"])
    assert out["result"] == "verified"
    assert out["implements"]["result"] == "refines"
