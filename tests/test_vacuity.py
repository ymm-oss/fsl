from __future__ import annotations
import sys

import json
import subprocess
import textwrap
from pathlib import Path

from fslc import build_spec, parse, prove, verify
from fslc.cli import run_verify


ROOT = Path(__file__).resolve().parents[1]
PY = sys.executable


def _spec(src: str):
    return build_spec(parse(textwrap.dedent(src)))


def _kinds(out):
    return [w.get("kind") for w in out.get("warnings", []) if isinstance(w, dict)]


def test_vacuous_implication_warning_has_name_loc_and_requirement():
    src = """
    spec VacuousInvariant {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant NeverOne "REQ-I: x never reaches one" { x == 1 => false }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    assert out["result"] == "verified"
    warning = next(w for w in out["warnings"] if w.get("kind") == "vacuous_implication")
    assert warning["name"] == "NeverOne"
    assert warning["loc"]["line"] > 0
    assert warning["requirement"]["id"] == "REQ-I"
    assert "within depth 2" in warning["message"]


def test_vacuous_implication_finding_names_insufficient_depth_when_structurally_possible():
    # issue #170: the antecedent `x == 1` is not blocked by any *other*
    # invariant (nothing stops x from reaching 1) -- it is simply never
    # reached by the spec's one no-op action within depth 2. The invariant
    # being diagnosed must be excluded from its own blocking-core check, or
    # `x == 1 => false` trivially "blocks itself" and misreports
    # over_constrained for every vacuous_implication (a bug caught while
    # implementing this).
    src = """
    spec VacuousInvariant {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant NeverOne "REQ-I: x never reaches one" { x == 1 => false }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    warning = next(w for w in out["warnings"] if w.get("kind") == "vacuous_implication")
    assert warning["classification"] == "insufficient_depth"
    assert warning["blocking"] == []


def test_vacuous_implication_finding_names_blocking_invariant():
    src = """
    spec OverConstrainedInvariant {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant XIsZero { x == 0 }
      invariant NeverOne "REQ-I: x never reaches one" { x == 1 => false }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    warning = next(w for w in out["warnings"] if w.get("kind") == "vacuous_implication")
    assert warning["classification"] == "over_constrained"
    blocking_names = {b.get("name") for b in warning["blocking"]}
    assert "XIsZero" in blocking_names
    assert "NeverOne" not in blocking_names


def test_forall_wrapped_implication_antecedent_is_checked():
    src = """
    spec ForallVacuousInvariant {
      type K = 0..1
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant Guarded "REQ-F: guarded invariant" {
        forall k: K { x == 1 => k >= 0 }
      }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    assert out["result"] == "verified"
    assert "vacuous_implication" in _kinds(out)


def test_vacuous_leadsto_trigger_warning():
    src = """
    spec VacuousLeadsto {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant Stable { x == 0 }
      leadsTo EventuallyTwo "REQ-L: x one responds" { x == 1 ~> x == 2 }
    }
    """
    out = verify(_spec(src), 3, deadlock_mode="ignore")
    assert out["result"] == "verified"
    warning = next(w for w in out["warnings"] if w.get("kind") == "vacuous_leadsto")
    assert warning["name"] == "EventuallyTwo"
    assert warning["requirement"]["id"] == "REQ-L"


def test_conditioned_redundant_requires_warning():
    src = """
    spec ConditionedRequires {
      enum St { Paid, Cancelled }
      state { st: St }
      init { st = Paid }
      action cancel() { st = Cancelled }
      action pay() "REQ-A: paid action" {
        requires st == Paid
        requires st != Cancelled
        st = Paid
      }
      invariant TypeOk { st == Paid or st == Cancelled }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    assert out["result"] == "verified"
    warnings = [w for w in out["warnings"] if w.get("kind") == "always_true_requires"]
    assert len(warnings) == 1
    assert warnings[0]["name"] == "pay"
    assert warnings[0]["requirement"]["id"] == "REQ-A"


def test_frozen_ghost_tautology_warning():
    src = """
    spec FrozenGhostTautology {
      state { ghost: Bool, x: Bool }
      init {
        ghost = true
        x = false
      }
      action flip() { x = not x }
      invariant FrozenGhost { ghost }
    }
    """
    out = verify(_spec(src), 1, deadlock_mode="ignore")
    assert out["result"] == "verified"
    warning = next(w for w in out["warnings"] if w.get("kind") == "tautology_over_frozen")
    assert warning["name"] == "FrozenGhost"
    assert "ghost" in warning["message"]
    assert warning["hint"] == "make such variables 'const', or add the action that should modify them"
    assert warning["faithfulness_class"] == "frozen_only_invariant"
    assert warning["recommended_action"] == "run mutate to check kill-rate"


def test_urgency_freeze_warning_for_always_enabled_urgent_deadline():
    src = """
    requirements UrgencyFreezeTrap {
      state { pending: Bool }
      init { pending = true }
      action spin() { pending = pending }
      time {
        urgent spin
        age age while pending
      }
      requirement NFR-1 "deadline" { deadline age <= 0 }
    }
    """
    out = verify(_spec(src), 3, deadlock_mode="ignore")
    assert out["result"] == "verified"
    warning = next(w for w in out["warnings"] if w.get("kind") == "urgency_freeze")
    assert warning["name"] == "tick"
    assert "'spin'" in warning["message"]
    assert "generated action 'tick' is never enabled" in warning["message"]
    assert "deadline invariant(s)" in warning["message"]
    assert "deadline-urgency pattern" in warning["hint"]
    assert warning["requirement"]["id"] == "NFR-1"


def test_deadline_urgency_pattern_not_flagged_as_freeze():
    src = """
    requirements DeadlineUrgencyPattern {
      const SLA = 1
      state { pending: Bool }
      init { pending = true }
      action respond_due() {
        requires pending
        requires age >= SLA
        pending = false
      }
      time {
        urgent respond_due
        age age while pending
      }
      requirement NFR-1 "deadline" { deadline age <= SLA }
    }
    """
    out = verify(_spec(src), 4, deadlock_mode="ignore")
    assert out["result"] == "verified"
    assert out["action_coverage"]["tick"] is True
    assert "urgency_freeze" not in _kinds(out)


def test_frozen_plus_dynamic_nontrivial_invariant_not_flagged():
    src = """
    spec FrozenPlusDynamic {
      state { ghost: Bool, x: Bool }
      init {
        ghost = true
        x = false
      }
      action keep_x_dynamic() { x = false }
      invariant DependsOnDynamic { ghost => not x }
    }
    """
    out = verify(_spec(src), 1, deadlock_mode="ignore")
    assert out["result"] == "verified"
    assert "tautology_over_frozen" not in _kinds(out)


def test_literal_true_invariant_not_frozen_tautology():
    src = """
    spec LiteralTrueInvariant {
      state { x: Bool }
      init { x = false }
      action flip() { x = not x }
      invariant Inv { true }
    }
    """
    out = verify(_spec(src), 1, deadlock_mode="ignore")
    assert out["result"] == "verified"
    assert "tautology_over_frozen" not in _kinds(out)


def test_coverage_false_actions_suppress_always_true_requires():
    src = """
    spec CoverageFalseSuppressesRequires {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      action impossible() {
        requires x == 1
        requires x == 1
        x = x
      }
      invariant Stable { x == 0 }
    }
    """
    out = verify(_spec(src), 2, deadlock_mode="ignore")
    assert out["result"] == "verified"
    assert "always_true_requires" not in _kinds(out)
    assert out["action_coverage"]["impossible"] is not True


def test_vacuity_warnings_not_surfaced_on_violated_path():
    src = """
    spec ViolatedBeatsVacuity {
      state { x: Int }
      init { x = 0 }
      action inc() { x = 1 }
      invariant Bad { x == 0 }
      invariant Vacuous { x == 2 => false }
    }
    """
    out = verify(_spec(src), 1, deadlock_mode="ignore")
    assert out["result"] == "violated"
    assert "warnings" not in out


def test_induction_transparently_carries_vacuity_warning():
    src = """
    spec InductionVacuity {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant NeverOne { x == 1 => false }
    }
    """
    out = prove(_spec(src), k_ind=1, base_depth=2, deadlock_mode="ignore")
    assert out["result"] == "proved"
    assert "vacuous_implication" in _kinds(out)


def test_vacuity_error_and_ignore_modes(tmp_path):
    src = """
    spec VacuityModes {
      state { x: Int }
      init { x = 0 }
      action noop() { x = x }
      invariant NeverOne { x == 1 => false }
    }
    """
    path = tmp_path / "vacuity_modes.fsl"
    path.write_text(textwrap.dedent(src), encoding="utf-8")

    proc = subprocess.run(
        [str(PY), "-m", "fslc", "verify", str(path), "--depth", "2",
         "--deadlock", "ignore", "--vacuity", "error"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    error_out = json.loads(proc.stdout)
    assert proc.returncode == 2
    assert error_out["result"] == "error"
    assert error_out["kind"] == "vacuous_implication"
    assert error_out["findings"][0]["kind"] == "vacuous_implication"

    ignored = run_verify(
        str(path), 2, "ignore", vacuity_mode="ignore")
    assert ignored["result"] == "verified"
    assert "vacuous_implication" not in _kinds(ignored)


def test_compose_sync_duplicate_component_guards_not_flagged(tmp_path):
    # A synchronized action inherits its requires from multiple components.
    # An identical guard shared across components (a by-design duplicate where
    # each component defends its own contract) must not be misflagged as
    # "tautological given a preceding clause". Same shape as deposit_audited in
    # specs/bank_system.fsl.
    (tmp_path / "a.fsl").write_text(textwrap.dedent("""
    spec CompA {
      type Amount = 0..2
      state { x: Int }
      init { x = 0 }
      action step(a: Amount) {
        requires a > 0
        x = x + a
      }
      invariant NonNeg { x >= 0 }
    }
    """), encoding="utf-8")
    (tmp_path / "b.fsl").write_text(textwrap.dedent("""
    spec CompB {
      type Amount = 0..2
      state { y: Int }
      init { y = 0 }
      action step(a: Amount) {
        requires a > 0
        y = y + a
      }
      invariant NonNeg { y >= 0 }
    }
    """), encoding="utf-8")
    compose_path = tmp_path / "system.fsl"
    compose_path.write_text(textwrap.dedent("""
    compose DupGuardSystem {
      use CompA as a from "a.fsl"
      use CompB as b from "b.fsl"

      action both(v: a.Amount) = a.step(v) || b.step(v) {
      }

      internal a.step
      internal b.step

      invariant Together { a.x == b.y }
    }
    """), encoding="utf-8")

    out = run_verify(str(compose_path), 6, "ignore")
    assert out["result"] == "verified"
    assert out["action_coverage"]["both"] is True
    assert "always_true_requires" not in _kinds(out)


def test_verified_sample_corpus_has_no_vacuity_false_positives():
    roots = [ROOT / "specs", ROOT / "examples", ROOT / "examples" / "gallery" / "valid"]
    paths = sorted({p for root in roots for p in root.rglob("*.fsl")})
    checked = 0
    for path in paths:
        # gallery/errors, adversarial, and injected hold intentionally-flawed specs.
        # They are out of scope for the "zero false positives on the correct corpus"
        # sweep (injected is handled by the dedicated tests/test_injection_bench.py).
        posix = path.as_posix()
        if ("gallery/errors" in posix or "gallery/adversarial" in posix
                or "gallery/injected" in posix):
            continue
        baseline = run_verify(str(path), 8, "ignore", vacuity_mode="ignore")
        if baseline["result"] not in {"verified", "proved"}:
            continue
        checked += 1
        out = run_verify(str(path), 8, "ignore", vacuity_mode="warn")
        vacuity = [w for w in out.get("warnings", []) if w.get("kind") in {
            "vacuous_implication",
            "vacuous_leadsto",
            "always_true_requires",
            "tautology_over_frozen",
            "urgency_freeze",
        }]
        assert vacuity == [], (path, vacuity)
    assert checked > 0
