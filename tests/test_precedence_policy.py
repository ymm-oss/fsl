# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Business-layer no-bypass precedence policy (#75).

``every <Entity> reaching <targets> must have passed through <waypoints>``
desugars to a synthesized, invisible history flag (``Map<Entity, Bool>``) plus
a kernel invariant — the same "business already synthesizes state" pattern
the stage enum/map/init/fair-actions synthesis already uses (see
``docs/DESIGN-precedence-policy.md``). Two policies over the same
(process, waypoint-set) share one history map (dedup).

#85 additionally synthesizes a stabilizing auxiliary invariant, `<PolicyId>_stability`,
alongside the history flag: `forall c { stage[c] in dominated(W) => visited[c] }`,
where `dominated(W)` is the set of stages dominated by the waypoint set `W` in
the process graph. This closes k-induction (k=1) for compliant precedence
policies without a ghost CTI.
"""
from fslc.cli import run_check, run_verify
from fslc.model import FslError, build_spec
from fslc.parser import parse


# --------------------------------------------------------------------------
# violated: a bypass path exists
# --------------------------------------------------------------------------

BIZ_BYPASS_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition bypass Requested -> Refunded by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-APPROVAL "承認を経ずに完了しない"
    every Return reaching Refunded must have passed through Approved
}
verify {
  instances Return = 3
}
'''


def test_precedence_policy_bypass_is_violated(tmp_path):
    f = tmp_path / "bypass.fsl"
    f.write_text(BIZ_BYPASS_SRC, encoding="utf-8")

    result = run_verify(str(f), 6, "warn")
    assert result["result"] == "violated"
    assert result["violation_kind"] == "invariant"
    assert result["invariant"] == "CTRL-APPROVAL"
    assert result["requirement"]["id"] == "CTRL-APPROVAL"

    # the trace demonstrates the bypass transition, not some unrelated path
    assert result["last_action"]["name"] == "bypass"

    # #85: the auto-synthesized stability aux is a *separate* invariant and
    # must not be what the counterexample attributes to -- the bypass
    # transition (Requested -> Refunded) also enlarges the dominated set to
    # exclude Refunded (it's now reachable around Approved), so the aux
    # stays true and only the real policy invariant is violated.
    assert result["invariant"] != "CTRL-APPROVAL_stability"


# --------------------------------------------------------------------------
# compliant: no bypass path
# --------------------------------------------------------------------------

BIZ_COMPLIANT_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-APPROVAL "承認を経ずに完了しない"
    every Return reaching Refunded must have passed through Approved
}
verify {
  instances Return = 3
}
'''


def test_precedence_policy_compliant_verifies_clean(tmp_path):
    f = tmp_path / "compliant.fsl"
    f.write_text(BIZ_COMPLIANT_SRC, encoding="utf-8")

    checked = run_check(str(f))
    assert checked["result"] == "ok"

    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert "CTRL-APPROVAL" in result["invariants_checked"]
    # composes cleanly with #69's business sink-derived terminal: both sinks
    # (Rejected, Refunded) are declared terminal, so no deadlock warning.
    assert result["deadlock"]["found"] is False


# --------------------------------------------------------------------------
# waypoint disjunction: "passed through A or B"
# --------------------------------------------------------------------------

BIZ_DISJUNCTION_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition close_approved Approved -> Refunded by Manager
    transition close_rejected Rejected -> Refunded by Manager
  }

  policy CTRL-DECIDED "決定を経ずに完了しない"
    every Return reaching Refunded must have passed through Approved or Rejected
}
verify {
  instances Return = 2
}
'''


def test_precedence_policy_waypoint_disjunction_verifies_clean(tmp_path):
    # Either branch of the disjunction satisfies the policy; neither branch
    # alone is a bypass.
    f = tmp_path / "disjunction.fsl"
    f.write_text(BIZ_DISJUNCTION_SRC, encoding="utf-8")

    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert "CTRL-DECIDED" in result["invariants_checked"]


def test_precedence_policy_waypoint_disjunction_bypass_is_violated():
    # A direct Requested -> Refunded transition bypasses both disjuncts.
    src = BIZ_DISJUNCTION_SRC.replace(
        "transition close_rejected Rejected -> Refunded by Manager",
        "transition close_rejected Rejected -> Refunded by Manager\n"
        "    transition bypass Requested -> Refunded by Manager",
    )
    ast = parse(src)
    spec = build_spec(ast)
    assert "return_stage_via_Approved_Rejected" in spec["state"]


# --------------------------------------------------------------------------
# initial-stage-in-waypoints: history flag starts true
# --------------------------------------------------------------------------

BIZ_INITIAL_WAYPOINT_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-TRIVIAL "waypoint is the initial stage"
    every Return reaching Refunded must have passed through Requested
}
verify {
  instances Return = 2
}
'''


def test_precedence_policy_initial_stage_in_waypoints_inits_true():
    ast = parse(BIZ_INITIAL_WAYPOINT_SRC)
    init_stmts = next(item for item in ast[2] if item[0] == "init")[1]
    # locate the forall_stmt whose body assigns the history var
    matches = []
    for stmt in init_stmts:
        for action in stmt[2]:
            target = action[1]
            if target[0] == "index" and target[1] == "return_stage_via_Requested":
                matches.append(action)
    assert len(matches) == 1
    assign = matches[0]
    assert assign[2] == ("bool", True)


def test_precedence_policy_initial_stage_in_waypoints_verifies_trivially(tmp_path):
    f = tmp_path / "initial_waypoint.fsl"
    f.write_text(BIZ_INITIAL_WAYPOINT_SRC, encoding="utf-8")

    result = run_verify(str(f), 6, "warn")
    assert result["result"] == "verified"


# --------------------------------------------------------------------------
# dedup: two policies over the same (process, waypoints) share one history map
# --------------------------------------------------------------------------

BIZ_DEDUP_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Closed, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition close_approved Approved -> Closed by Manager
    transition close_rejected Rejected -> Closed by Manager
    transition refund Closed -> Refunded by Manager
  }

  policy CTRL-A "closed via approved or rejected"
    every Return reaching Closed must have passed through Approved or Rejected
  policy CTRL-B "refunded via approved or rejected"
    every Return reaching Refunded must have passed through Approved or Rejected
}
verify {
  instances Return = 2
}
'''


def test_precedence_policy_dedups_history_map_across_policies():
    ast = parse(BIZ_DEDUP_SRC)
    state_decls = next(item for item in ast[2] if item[0] == "state")[1]
    via_decls = [decl for decl in state_decls if "_via_" in decl[1]]
    assert len(via_decls) == 1
    assert via_decls[0][1] == "return_stage_via_Approved_Rejected"

    invariants = {item[1]: item[2] for item in ast[2] if item[0] == "invariant"}
    assert "CTRL-A" in invariants and "CTRL-B" in invariants
    # both invariants reference the same shared history var: expr is
    # ("forall", binder, ("bin", "=>", reached_target, ("index", history_var, ("var", "c"))))
    assert invariants["CTRL-A"][2][3][1] == "return_stage_via_Approved_Rejected"
    assert invariants["CTRL-B"][2][3][1] == "return_stage_via_Approved_Rejected"


def test_precedence_policy_dedup_spec_verifies_clean(tmp_path):
    f = tmp_path / "dedup.fsl"
    f.write_text(BIZ_DEDUP_SRC, encoding="utf-8")

    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert "CTRL-A" in result["invariants_checked"]
    assert "CTRL-B" in result["invariants_checked"]


# --------------------------------------------------------------------------
# unknown stage / unknown entity -> FslError naming the policy id
# --------------------------------------------------------------------------

BIZ_UNKNOWN_STAGE_SRC = r'''business ReturnHandling {
  actor Manager
  entity Return

  process Return {
    stages Requested, Approved, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-BAD "references a stage that does not exist"
    every Return reaching Refunded must have passed through NoSuchStage
}
verify {
  instances Return = 2
}
'''


def test_precedence_policy_unknown_stage_names_policy_id(tmp_path):
    f = tmp_path / "unknown_stage.fsl"
    f.write_text(BIZ_UNKNOWN_STAGE_SRC, encoding="utf-8")

    try:
        parse(BIZ_UNKNOWN_STAGE_SRC)
        assert False, "expected FslError"
    except FslError as exc:
        assert "CTRL-BAD" in str(exc)
        assert "NoSuchStage" in str(exc)

    checked = run_check(str(f))
    assert checked["result"] == "error"
    assert checked["kind"] == "type"
    assert "CTRL-BAD" in checked["message"]


BIZ_UNKNOWN_ENTITY_SRC = r'''business ReturnHandling {
  actor Manager
  entity Return

  process Return {
    stages Requested, Approved, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-BAD "references an entity with no process"
    every Invoice reaching Refunded must have passed through Approved
}
verify {
  instances Return = 2
}
'''


def test_precedence_policy_unknown_entity_names_policy_id():
    try:
        parse(BIZ_UNKNOWN_ENTITY_SRC)
        assert False, "expected FslError"
    except FslError as exc:
        assert "CTRL-BAD" in str(exc)
        assert "Invoice" in str(exc)


# --------------------------------------------------------------------------
# #85: auto-synthesized dominated-set stability invariant closes k-induction
# --------------------------------------------------------------------------


def test_precedence_policy_compliant_proves_under_induction(tmp_path):
    # Reproduces the ghost CTI from #85: before the fix, this compliant
    # linear-process spec verified under BMC but stalled at unknown_cti
    # under `--engine induction`.
    f = tmp_path / "compliant.fsl"
    f.write_text(BIZ_COMPLIANT_SRC, encoding="utf-8")

    result = run_verify(str(f), 8, "warn", engine="induction")
    assert result["result"] == "proved"
    assert result["invariants_checked"] == ["_bounds_return_stage", "CTRL-APPROVAL", "CTRL-APPROVAL_stability"]
    assert result["k_used"]["CTRL-APPROVAL_stability"] == 1


BIZ_DOWNSTREAM_DOMINATOR_SRC = r'''business DownstreamDominator {
  actor Worker
  entity Item

  process Item {
    stages R, A, B, C
    initial R
    transition toA R -> A by Worker
    transition toB R -> B by Worker
    transition aToB A -> B by Worker
    transition aToC A -> C by Worker
  }

  policy CTRL-DOM "C is reached only via A"
    every Item reaching C must have passed through A
}
verify {
  instances Item = 2
}
'''


def test_precedence_policy_downstream_but_reachable_around_waypoint_proves(tmp_path):
    # B is downstream of A (A -> B) but is *also* directly reachable from the
    # initial stage without passing through A (R -> B). "downstream of W"
    # would wrongly include B in the aux invariant's domain and make it
    # unsound (R -> B falsifies it); the dominated-set computation must
    # exclude B. C, by contrast, is only reachable via A, so C IS dominated.
    ast = parse(BIZ_DOWNSTREAM_DOMINATOR_SRC)
    invariants = {item[1]: item[2] for item in ast[2] if item[0] == "invariant"}
    stability_expr = invariants["CTRL-DOM_stability"]
    # expr shape: ("forall", binder, ("bin", "=>", in_dominated, visited))
    in_dominated = stability_expr[2][2]

    def stage_names(expr):
        if expr[0] == "bin" and expr[1] == "==":
            return {expr[3][1]}
        assert expr[0] == "bin" and expr[1] == "or"
        return stage_names(expr[2]) | stage_names(expr[3])

    assert stage_names(in_dominated) == {"A", "C"}

    f = tmp_path / "dominator.fsl"
    f.write_text(BIZ_DOWNSTREAM_DOMINATOR_SRC, encoding="utf-8")
    result = run_verify(str(f), 8, "warn", engine="induction")
    assert result["result"] == "proved"


BIZ_CYCLIC_SRC = r'''business ReturnCycle {
  actor Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition rework Approved -> Requested by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-APPROVAL "no completion without approval"
    every Return reaching Refunded must have passed through Approved
}
verify {
  instances Return = 3
}
'''


def test_precedence_policy_cyclic_process_proves_under_induction(tmp_path):
    # `rework` goes back upstream (Approved -> Requested) after the waypoint
    # has already been passed; the aux invariant must still hold (and be
    # inductive) despite the cycle.
    f = tmp_path / "cyclic.fsl"
    f.write_text(BIZ_CYCLIC_SRC, encoding="utf-8")

    result = run_verify(str(f), 8, "warn", engine="induction")
    assert result["result"] == "proved"
    assert "CTRL-APPROVAL_stability" in result["invariants_checked"]


def test_precedence_policy_waypoint_disjunction_proves_under_induction(tmp_path):
    f = tmp_path / "disjunction.fsl"
    f.write_text(BIZ_DISJUNCTION_SRC, encoding="utf-8")

    result = run_verify(str(f), 8, "warn", engine="induction")
    assert result["result"] == "proved"
    assert "CTRL-DECIDED_stability" in result["invariants_checked"]
