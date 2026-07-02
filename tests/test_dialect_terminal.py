# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""terminal { } across the requirements and business dialects (#69).

Requirements: `terminal_def` is now a `requirements_item` alternative
(grammar.py) and passes through the existing dialects.py fallback
(`_expand_item`'s ``return [item], []``) unchanged into the kernel AST — no
dialects.py case was added for it. The kernel (model.py) still only accepts
one `terminal { }` block per spec.

Business: no new syntax. `_generate_business_items` derives one kernel
`terminal { }` from each process's *sink stages* (stages with no outgoing
transition): if every process has >=1 sink, the generated predicate is the
conjunction over processes of `forall c: <Entity> { stage(c) in {Sink...} }`.
If any process is cyclic (no sink), no terminal is generated at all and
deadlock checking is unchanged from before this feature.
"""
from fslc.cli import run_check, run_verify
from fslc.model import FslError, build_spec
from fslc.parser import parse


# --------------------------------------------------------------------------
# requirements layer: explicit terminal { } (Part 1 — pass-through)
# --------------------------------------------------------------------------

# Uses `process Claim { ... }`, so the terminal predicate is written against
# the synthesized stage map: the lowercased process/entity name + "_stage"
# (`_process_state_var` in dialects.py), i.e. `claim_stage` here.
REQ_PROCESS_TERMINAL_SRC = r'''requirements ClaimTerminal {
  entity Claim
  process Claim {
    stages Draft, Approved, Rejected
    initial Draft
    transition approve Draft -> Approved by Manager
    transition reject Draft -> Rejected by Manager
  }
  terminal { forall c: Claim { claim_stage[c] == Approved or claim_stage[c] == Rejected } }
}
verify {
  instances Claim = 2
}
'''


def test_requirements_terminal_parses_and_flows_to_kernel_spec():
    spec = build_spec(parse(REQ_PROCESS_TERMINAL_SRC))
    assert spec["terminal"] is not None
    # the synthesized stage map name is <process-name-lowercased>_stage
    assert spec["state"]["claim_stage"]


def test_requirements_terminal_verifies_clean_at_declared_stop(tmp_path):
    f = tmp_path / "claim_terminal.fsl"
    f.write_text(REQ_PROCESS_TERMINAL_SRC, encoding="utf-8")

    checked = run_check(str(f))
    assert checked["result"] == "ok"

    # default --deadlock mode ("warn"): both sinks are declared terminal, so
    # no deadlock warning is produced without needing --deadlock ignore.
    result = run_verify(str(f), 6, "warn")
    assert result["result"] == "verified"
    assert result["deadlock"]["found"] is False


# Raw kernel-style requirements (no `process`) so a transition can exist that
# is NOT covered by the declared terminal predicate — demonstrates that
# `terminal` only excludes the states it names; a different, un-declared
# stuck state is still reported.
REQ_PARTIAL_TERMINAL_SRC = r'''requirements TerminalReq {
  type Id = 0..1
  enum St { Working, Done, Stuck }
  state { x: Map<Id, St> }
  init { forall i: Id { x[i] = Working } }

  requirement REQ-1 "finish moves working items to done" {
    action finish(i: Id) { requires x[i] == Working  x[i] = Done }
  }
  requirement REQ-2 "a bug: items can get stuck without finishing" {
    action drop(i: Id) { requires x[i] == Working  x[i] = Stuck }
  }

  terminal { forall i: Id { x[i] == Done } }
}
'''


def test_requirements_terminal_still_reports_unintended_deadlock(tmp_path):
    f = tmp_path / "terminal_partial.fsl"
    f.write_text(REQ_PARTIAL_TERMINAL_SRC, encoding="utf-8")

    result = run_verify(str(f), 5, "error")
    assert result["result"] == "violated"
    assert result["violation_kind"] == "deadlock"


REQ_DUP_TERMINAL_SRC = r'''requirements DupTerminal {
  type Id = 0..1
  enum St { Working, Done }
  state { x: Map<Id, St> }
  init { forall i: Id { x[i] = Working } }

  requirement REQ-1 "finish" {
    action finish(i: Id) { requires x[i] == Working  x[i] = Done }
  }

  terminal { forall i: Id { x[i] == Done } }
  terminal { forall i: Id { x[i] == Done } }
}
'''


def test_requirements_duplicate_terminal_block_errors(tmp_path):
    # Mirrors the kernel's own duplicate-terminal-block rule (model.py) —
    # requirements gets no special-case handling of its own.
    f = tmp_path / "dup_terminal.fsl"
    f.write_text(REQ_DUP_TERMINAL_SRC, encoding="utf-8")

    try:
        build_spec(parse(REQ_DUP_TERMINAL_SRC))
        assert False, "expected FslError"
    except FslError as exc:
        assert "duplicate terminal block" in str(exc)

    checked = run_check(str(f))
    assert checked["result"] == "error"
    assert checked["kind"] == "semantics"


# --------------------------------------------------------------------------
# business layer: sink-derived terminal (Part 2 — no new syntax)
# --------------------------------------------------------------------------

BIZ_LINEAR_SRC = r'''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }
}
verify {
  instances Return = 3
}
'''


def test_business_linear_process_derives_terminal_from_sinks():
    spec = build_spec(parse(BIZ_LINEAR_SRC))
    assert spec["terminal"] is not None


def test_business_linear_process_verifies_clean_without_deadlock_ignore(tmp_path):
    f = tmp_path / "return_handling.fsl"
    f.write_text(BIZ_LINEAR_SRC, encoding="utf-8")

    checked = run_check(str(f))
    assert checked["result"] == "ok"

    # default --deadlock mode ("warn"): Rejected/Refunded (the sinks) are the
    # only reachable stuck states, so they no longer need --deadlock ignore.
    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert result["deadlock"]["found"] is False


BIZ_CYCLIC_SRC = r'''business PingPong {
  actor System
  entity Token

  process Token {
    stages A, B
    initial A
    transition to_b A -> B by System
    transition to_a B -> A by System
  }
}
verify {
  instances Token = 2
}
'''


def test_business_cyclic_process_generates_no_terminal(tmp_path):
    # Every stage has an outgoing transition (a genuine cycle) -> no sink on
    # this process -> no terminal item is generated at all, matching
    # pre-#69 behavior exactly (deadlock checking is unaffected).
    ast = parse(BIZ_CYCLIC_SRC)
    assert not any(item[0] == "terminal" for item in ast[2])

    spec = build_spec(ast)
    assert spec["terminal"] is None

    f = tmp_path / "ping_pong.fsl"
    f.write_text(BIZ_CYCLIC_SRC, encoding="utf-8")
    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert result["deadlock"]["found"] is False


BIZ_MULTI_PROCESS_SRC = r'''business TwoProcs {
  actor Manager, Finance
  entity Order
  entity Invoice

  process Order {
    stages Placed, Shipped
    initial Placed
    transition ship Placed -> Shipped by Manager
  }

  process Invoice {
    stages Issued, Paid
    initial Issued
    transition pay Issued -> Paid by Finance
  }
}
verify {
  instances Order = 2
  instances Invoice = 2
}
'''


def test_business_multi_process_terminal_is_conjunction_of_sinks(tmp_path):
    # Deadlock is only "intended" once BOTH entities are simultaneously at
    # their own sink -- the conjunction, not a per-process disjunction.
    spec = build_spec(parse(BIZ_MULTI_PROCESS_SRC))
    term = spec["terminal"]
    assert term[0] == "bin" and term[1] == "and"
    branches = {term[2][0], term[3][0]}
    assert branches == {"forall"}

    f = tmp_path / "two_procs.fsl"
    f.write_text(BIZ_MULTI_PROCESS_SRC, encoding="utf-8")
    checked = run_check(str(f))
    assert checked["result"] == "ok"
    result = run_verify(str(f), 8, "warn")
    assert result["result"] == "verified"
    assert result["deadlock"]["found"] is False
