# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed relation layer coverage (#108)."""

from fslc.cli import run_verify
from fslc.runtime import Monitor


RELATION_SRC = r'''spec RelationDemo {
  type User = 0..1
  enum Role { Manager, Staff }

  state {
    delegates: relation User -> User,
    roles: relation User -> Role
  }

  init {
    delegates = Set {}
    roles = Set {}
  }

  action delegate(a: User, b: User) {
    requires a != b
    requires not reachable(delegates, b, a)
    delegates = delegates.add(a, b)
  }

  action revoke(a: User, b: User) {
    delegates = delegates.remove(a, b)
  }

  action grant(u: User, r: Role) {
    roles = roles.add(u, r)
  }

  invariant DelegatesAcyclic { acyclic(delegates) }
  invariant DelegatesFunctional { functional(delegates) }
  invariant RoleRangeBounded { range(roles).size() <= 2 }
  invariant DelegateDomainBounded { domain(delegates).size() <= 2 }
  reachable CanDelegate { delegates.contains(0, 1) }
}
'''


BAD_SELF_RELATION_SRC = r'''spec BadSelfRelation {
  type User = 0..1
  enum Role { Manager, Staff }
  state { assignments: relation User -> Role }
  init { assignments = Set {} }
  action grant(u: User, r: Role) { assignments = assignments.add(u, r) }
  invariant BadAcyclic { acyclic(assignments) }
}
'''


CYCLIC_SRC = r'''spec CyclicRelation {
  type User = 0..1
  state { delegates: relation User -> User }
  init { delegates = Set {} }
  action delegate(a: User, b: User) {
    requires a != b
    delegates = delegates.add(a, b)
  }
  invariant DelegatesAcyclic { acyclic(delegates) }
}
'''


def _write(tmp_path, src, name):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_relation_helpers_verify_and_reachability(tmp_path):
    spec = _write(tmp_path, RELATION_SRC, "relation_demo.fsl")
    out = run_verify(str(spec), 2, "warn")
    assert out["result"] == "verified"
    assert out["reachables"]["CanDelegate"]["witnessed_at_step"] == 1


def test_relation_runtime_add_remove_contains_and_enum_display(tmp_path):
    spec = _write(tmp_path, RELATION_SRC, "relation_demo.fsl")
    mon = Monitor(str(spec))
    assert mon.reset()["delegates"] == []

    first = mon.step("delegate", {"a": 0, "b": 1})
    assert first["ok"] is True
    assert first["state"]["delegates"] == [[0, 1]]

    blocked_cycle = mon.step("delegate", {"a": 1, "b": 0})
    assert blocked_cycle["ok"] is False
    assert blocked_cycle["kind"] == "requires_failed"

    grant = mon.step("grant", {"u": 0, "r": "Manager"})
    assert grant["ok"] is True
    assert grant["state"]["roles"] == [[0, "Manager"]]

    revoked = mon.step("revoke", {"a": 0, "b": 1})
    assert revoked["ok"] is True
    assert revoked["state"]["delegates"] == []


def test_relation_self_helpers_reject_non_self_relation(tmp_path):
    spec = _write(tmp_path, BAD_SELF_RELATION_SRC, "bad_relation.fsl")
    out = run_verify(str(spec), 1, "warn")
    assert out["result"] == "error"
    assert out["kind"] == "type"
    assert "self-relation" in out["message"]
    assert ".contains(a, b)" in out["hint"]


def test_relation_trace_displays_counterexample_pairs(tmp_path):
    spec = _write(tmp_path, CYCLIC_SRC, "cyclic_relation.fsl")
    out = run_verify(str(spec), 2, "warn")
    assert out["result"] == "violated"
    assert out["invariant"] == "DelegatesAcyclic"
    states = out["trace"]
    assert states[-1]["state"]["delegates"] == [[0, 1], [1, 0]]
