# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""`fslc verify --instances`/`--values` bounds overrides (#86) redefine the
verifiable world. Acceptance/forbidden scenarios that hardcode ids/numbers
from the *original* world can then fall outside the shrunken one — e.g. a
liveness run with `--instances Case=1` replaying an acceptance scenario that
calls `accept(2)`. Before #89, `run_verify` replayed those scenarios as a
hard gate regardless of overrides, so `--instances Case=1 --property <L>`
errored out instead of returning the (small-model) liveness verdict.

#89's fix (design "option B" from the issue): only when `--instances`/
`--values` overrides are active, a scenario whose replay fails purely
because it references a value outside the overridden bounds is downgraded
per-scenario from hard-error to skip, surfaced via `warnings`. Without
overrides, or for any other failure reason (a false `expect`, a `requires`
that never holds), behavior is unchanged: hard error.
"""
from fslc.cli import exit_code, run_verify

REPRO_SRC = r'''requirements Repro {
  entity Case
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  requirement REQ-1 "accept" {
    fair action accept(c: Case) { requires cases[c] == Waiting  cases[c] = Accepted }
  }
  requirement REQ-2 "respond" {
    fair action respond(c: Case) { requires cases[c] == Accepted  cases[c] = Responded }
  }
  leadsTo EveryAcceptedGetsResponse "every accepted case eventually gets a response" {
    forall c: Case { cases[c] == Accepted ~> cases[c] == Responded }
  }
  acceptance AC-1 "two cases in parallel" {
    accept(1)
    accept(2)
    respond(1)
    respond(2)
    expect cases[1] == Responded and cases[2] == Responded
  }
}
verify { instances Case = 3 }
'''

# Same shape as REPRO_SRC, but AC-1's `expect` (not its steps) is the only
# thing that references an out-of-range id: the steps run entirely on id 0.
EXPECT_OUT_OF_RANGE_SRC = r'''requirements ReproExpect {
  entity Case
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  requirement REQ-1 "accept" {
    fair action accept(c: Case) { requires cases[c] == Waiting  cases[c] = Accepted }
  }
  acceptance AC-1 "expect indexes out of range" {
    accept(0)
    expect cases[1] == Accepted
  }
}
verify { instances Case = 3 }
'''

# In-range acceptance (only id 0) whose `expect` is genuinely false: must
# still hard-error even with overrides active (overrides only excuse
# out-of-range references, not wrong specs).
GENUINE_FAILURE_SRC = r'''requirements GenuineFail {
  entity Case
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  requirement REQ-1 "accept" {
    fair action accept(c: Case) { requires cases[c] == Waiting  cases[c] = Accepted }
  }
  acceptance AC-2 "genuinely wrong expect" {
    accept(0)
    expect cases[0] == Waiting
  }
}
verify { instances Case = 3 }
'''

# No CLI overrides here: the verify block itself already declares the small
# world (Case = 1), and AC-1 hardcodes an out-of-range id. This must still
# hard-error today's way (regression guard: skip only applies when the CLI
# --instances/--values flags are the ones doing the shrinking).
NO_OVERRIDE_OUT_OF_RANGE_SRC = r'''requirements NoOverrideRepro {
  entity Case
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  requirement REQ-1 "accept" {
    fair action accept(c: Case) { requires cases[c] == Waiting  cases[c] = Accepted }
  }
  acceptance AC-1 "uses out-of-range id" {
    accept(1)
    expect cases[1] == Accepted
  }
}
verify { instances Case = 1 }
'''

# number-arg out-of-range under a --values shrink (rather than --instances).
NUMBER_OVERRIDE_SRC = r'''requirements NumOverrideDemo {
  number Amount
  entity Claim
  process Claim with amount: Amount {
    stages Draft, Submitted
    initial Draft
    transition submit Draft -> Submitted by Employee with a: Amount when a >= 0 set amount = a
      covers REQ-1 "submit"
  }
  acceptance AC-1 "submit with an amount outside the shrunken range" {
    submit(0, 2)
    expect Claim 0 in Submitted
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
'''

# forbidden scenario whose setup step hardcodes an out-of-range id.
FORBIDDEN_SRC = r'''requirements ForbiddenRepro {
  entity Case
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  requirement REQ-1 "accept" {
    action accept(c: Case) { requires cases[c] == Waiting  cases[c] = Accepted }
  }
  requirement REQ-2 "respond" {
    action respond(c: Case) { requires cases[c] == Accepted  cases[c] = Responded }
  }
  forbidden FB-1 "cannot respond twice" {
    accept(1)
    respond(1)
    respond(1)
    expect rejected
  }
}
verify { instances Case = 3 }
'''


def _write(tmp_path, src, name="repro.fsl"):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return path


def test_liveness_with_instances_override_skips_out_of_fit_acceptance(tmp_path):
    spec = _write(tmp_path, REPRO_SRC)
    out = run_verify(
        str(spec), 6, "warn",
        instances=["Case=1"], property_name="EveryAcceptedGetsResponse",
    )
    assert out["result"] not in ("error",)
    assert out["result"] == "verified"
    skip_warnings = [w for w in out["warnings"] if w.get("kind") == "acceptance_skipped"]
    assert len(skip_warnings) == 1
    assert skip_warnings[0]["id"] == "AC-1"
    assert "AC-1" in skip_warnings[0]["message"]
    assert "skipped" in skip_warnings[0]["message"]


def test_in_range_acceptance_with_false_expect_still_hard_errors_under_overrides(tmp_path):
    spec = _write(tmp_path, GENUINE_FAILURE_SRC, name="genuine_fail.fsl")
    out = run_verify(str(spec), 6, "warn", instances=["Case=1"])
    assert out["result"] == "error"
    assert out["kind"] == "acceptance"
    assert out["id"] == "AC-2"
    assert exit_code(out) == 2


def test_expect_only_out_of_range_is_skipped_under_overrides(tmp_path):
    spec = _write(tmp_path, EXPECT_OUT_OF_RANGE_SRC, name="expect_oor.fsl")
    out = run_verify(str(spec), 6, "warn", instances=["Case=1"])
    assert out["result"] != "error"
    skip_warnings = [w for w in out["warnings"] if w.get("kind") == "acceptance_skipped"]
    assert len(skip_warnings) == 1
    assert skip_warnings[0]["id"] == "AC-1"


def test_values_override_out_of_range_number_arg_is_skipped(tmp_path):
    spec = _write(tmp_path, NUMBER_OVERRIDE_SRC, name="num_override.fsl")
    out = run_verify(str(spec), 4, "warn", values=["Amount=0..1"])
    assert out["result"] != "error"
    skip_warnings = [w for w in out["warnings"] if w.get("kind") == "acceptance_skipped"]
    assert len(skip_warnings) == 1
    assert skip_warnings[0]["id"] == "AC-1"
    assert "Amount=0..1" in skip_warnings[0]["message"]


def test_no_overrides_out_of_range_acceptance_still_hard_errors(tmp_path):
    spec = _write(tmp_path, NO_OVERRIDE_OUT_OF_RANGE_SRC, name="no_override.fsl")
    out = run_verify(str(spec), 4, "warn")
    assert out["result"] == "error"
    assert out["kind"] == "acceptance"
    assert out["id"] == "AC-1"
    assert exit_code(out) == 2
    assert "bounds_overrides" not in out


def test_forbidden_setup_out_of_range_is_skipped_under_overrides(tmp_path):
    spec = _write(tmp_path, FORBIDDEN_SRC, name="forbidden.fsl")

    # No overrides: FB-1's setup ids (1) fit the declared Case=3 world, so it
    # replays normally and verify succeeds without any skip warning.
    no_override_out = run_verify(str(spec), 4, "warn")
    assert no_override_out["result"] != "error"
    assert not [
        w for w in no_override_out.get("warnings", []) if w.get("kind") == "forbidden_skipped"
    ]

    # With --instances Case=1, FB-1's setup step accept(1) is out of range:
    # skipped, not a hard error.
    out = run_verify(str(spec), 4, "warn", instances=["Case=1"])
    assert out["result"] != "error"
    skip_warnings = [w for w in out["warnings"] if w.get("kind") == "forbidden_skipped"]
    assert len(skip_warnings) == 1
    assert skip_warnings[0]["id"] == "FB-1"
    assert "FB-1" in skip_warnings[0]["message"]
