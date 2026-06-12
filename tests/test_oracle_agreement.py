from __future__ import annotations

import shlex
from pathlib import Path

import pytest

from fslc.runtime import Monitor
from fslc.cli import run_refine

from oracle import ROOT, UnsupportedOracle, VerifyCase, bfs_oracle, can_monitor, run_verify_case


GALLERY = ROOT / "examples" / "gallery"
SPECS = ROOT / "specs"

REFINEMENT_FALSE_NEGATIVE_CASES = [
    (
        GALLERY / "errors" / "refinement_failed_impl.fsl",
        GALLERY / "errors" / "refinement_failed_abs.fsl",
        GALLERY / "errors" / "refinement_failed_map.fsl",
        3,
        "abs_requires_failed",
    ),
    (
        GALLERY / "adversarial" / "refine_mapping_boundary_impl.fsl",
        GALLERY / "adversarial" / "refine_mapping_boundary_abs.fsl",
        GALLERY / "adversarial" / "refine_mapping_boundary_map.fsl",
        2,
        "abs_state_mismatch",
    ),
]


def _declared(path: Path) -> tuple[str | None, str | None, str | None]:
    command = result = kind = None
    for line in path.read_text(encoding="utf-8").splitlines()[:16]:
        stripped = line.strip()
        if stripped.startswith("// expected-command:"):
            command = stripped.split(":", 1)[1].strip()
        elif stripped.startswith("// expected-result:"):
            result = stripped.split(":", 1)[1].strip()
        elif stripped.startswith("// expected-kind:"):
            kind = stripped.split(":", 1)[1].strip()
    return command, result, kind


def _case_from_command(path: Path, command: str) -> VerifyCase | None:
    parts = shlex.split(command)
    if not parts or parts[0] != "verify":
        return None
    depth = 4
    deadlock = "warn"
    engine = "bmc"
    for i, part in enumerate(parts):
        if part == "--depth":
            depth = int(parts[i + 1])
        elif part == "--deadlock":
            deadlock = parts[i + 1]
        elif part == "--engine":
            engine = parts[i + 1]
    return VerifyCase(path=path, depth=depth, deadlock=deadlock, engine=engine)


def oracle_cases() -> list[VerifyCase]:
    cases: list[VerifyCase] = []
    for path in sorted(SPECS.glob("*.fsl")):
        if "refines" in path.stem or path.stem.endswith("_refines"):
            continue
        cases.append(VerifyCase(path=path, depth=5, deadlock="warn"))

    for folder in (GALLERY / "valid", GALLERY / "errors"):
        for path in sorted(folder.glob("*.fsl")):
            command, result, _ = _declared(path)
            if result not in {"verified", "proved", "violated", "reachable_failed"}:
                continue
            case = _case_from_command(path, command or "")
            if case is not None:
                cases.append(case)
    return cases


@pytest.mark.parametrize("case", oracle_cases(), ids=lambda c: c.id)
def test_verify_verdict_agrees_with_monitor_oracle(case: VerifyCase):
    ok, reason = can_monitor(case.path)
    if not ok:
        pytest.skip(f"Monitor oracle requires deterministic init/spec: {reason}")

    try:
        oracle = bfs_oracle(case.path, case.depth)
    except UnsupportedOracle as exc:
        pytest.skip(str(exc))
    result = run_verify_case(case)

    violation_kinds = {entry["kind"] for entry in oracle.violations.values()}
    if oracle.violations:
        assert result["result"] == "violated", {
            "false_negative": result.get("result") in {"verified", "proved"},
            "case": case.id,
            "oracle_violations": oracle.violations,
            "fslc": result,
        }
        assert result.get("violation_kind") in violation_kinds
        assert result.get("violated_at_step") == min(v["depth"] for v in oracle.violations.values())
        return

    if case.deadlock == "error" and oracle.deadlock is not None:
        assert result["result"] == "violated", result
        assert result.get("violation_kind") == "deadlock"
        return

    mon_reachable_names = set(oracle.reachables)
    spec_reachable_names = {reach["name"] for reach in Monitor(case.path).spec["reachables"]}
    if spec_reachable_names - mon_reachable_names:
        assert result["result"] == "reachable_failed", {
            "case": case.id,
            "missing": sorted(spec_reachable_names - mon_reachable_names),
            "fslc": result,
        }
        return

    if result["result"] == "violated":
        # The concrete oracle does not model leadsTo lasso checks.  A finite
        # leadsTo counterexample from fslc is not an oracle disagreement here.
        assert result.get("violation_kind") == "leadsTo", result
    else:
        assert result["result"] in {"verified", "proved"}, result


@pytest.mark.parametrize(
    "impl,abs_spec,mapping,depth,kind",
    REFINEMENT_FALSE_NEGATIVE_CASES,
    ids=[case[2].relative_to(ROOT).as_posix() for case in REFINEMENT_FALSE_NEGATIVE_CASES],
)
def test_refinement_false_negative_fixtures_do_not_report_refines(impl, abs_spec, mapping, depth, kind):
    result = run_refine(str(impl), str(abs_spec), str(mapping), depth=depth)
    assert result["result"] == "refinement_failed", result
    assert result["kind"] == kind
