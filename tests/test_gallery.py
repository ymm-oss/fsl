from __future__ import annotations

import json
import shlex
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
GALLERY = ROOT / "examples" / "gallery"


@dataclass(frozen=True)
class GalleryCase:
    path: str
    command: str
    expected_result: str
    expected_kind: str | None = None


GALLERY_CASES = [
    GalleryCase("valid/tiny_turnstile.fsl", "verify --depth 4 --engine induction --deadlock ignore", "proved"),
    GalleryCase("valid/tiny_traffic_light.fsl", "verify --depth 5 --engine induction --deadlock ignore", "proved"),
    GalleryCase("valid/tiny_bounded_counter.fsl", "verify --depth 4 --engine induction --deadlock ignore", "proved"),
    GalleryCase("valid/small_vending_machine.fsl", "verify --depth 6 --deadlock ignore", "verified"),
    GalleryCase("valid/small_elevator.fsl", "verify --depth 7 --engine induction --deadlock ignore", "proved"),
    GalleryCase("valid/small_tcp_handshake.fsl", "verify --depth 6 --deadlock ignore", "verified"),
    GalleryCase("valid/medium_dining_philosophers_deadlock_demo.fsl", "verify --depth 6 --deadlock warn", "verified"),
    GalleryCase("valid/medium_two_phase_commit.fsl", "verify --depth 8 --engine induction --deadlock ignore", "proved"),
    GalleryCase("valid/large_order_workflow.fsl", "verify --depth 8 --deadlock ignore", "verified"),
    GalleryCase("errors/parse_missing_expression.fsl", "check", "error", "parse"),
    GalleryCase("errors/type_option_some_equality.fsl", "verify --depth 2", "error", "type"),
    GalleryCase("errors/type_undeclared_type.fsl", "check", "error", "type"),
    GalleryCase("errors/type_struct_set_field.fsl", "check", "error", "type"),
    GalleryCase("errors/semantics_duplicate_assignment.fsl", "verify --depth 2", "error", "semantics"),
    GalleryCase("errors/vacuous_contradictory_init.fsl", "verify --depth 2", "error", "vacuous"),
    GalleryCase("errors/violated_invariant_counter.fsl", "verify --depth 2", "violated", "invariant"),
    GalleryCase("errors/violated_type_bound_missing_guard.fsl", "verify --depth 2", "violated", "type_bound"),
    GalleryCase("errors/violated_ensures_wrong_postcondition.fsl", "verify --depth 2", "violated", "ensures"),
    GalleryCase("errors/violated_partial_op_unchecked_pop.fsl", "verify --depth 2", "violated", "partial_op"),
    GalleryCase("errors/violated_leads_to_starvation.fsl", "verify --depth 4 --deadlock ignore", "violated", "leadsTo"),
    GalleryCase("errors/violated_deadlock_terminal.fsl", "verify --depth 3 --deadlock error", "violated", "deadlock"),
    pytest.param(
        GalleryCase(
            "errors/refinement_failed_map.fsl",
            "refine refinement_failed_impl.fsl refinement_failed_abs.fsl refinement_failed_map.fsl --depth 3",
            "refinement_failed",
            "abs_requires_failed",
        ),
        marks=pytest.mark.xfail(
            reason="DOGFOOD-6 BUG-001: fslc reports refines for an approval shortcut",
            strict=True,
        ),
    ),
    GalleryCase("errors/error_acceptance_false_expect.fsl", "check", "error", "acceptance"),
    GalleryCase("adversarial/deep_nested_if_invariant.fsl", "verify --depth 4", "violated", "invariant"),
    GalleryCase("adversarial/seq_full_push_boundary.fsl", "verify --depth 2", "violated", "type_bound"),
    GalleryCase("adversarial/seq_empty_head_boundary.fsl", "verify --depth 2", "violated", "partial_op"),
    GalleryCase("adversarial/option_struct_set_seq_combo.fsl", "verify --depth 5 --engine induction --deadlock ignore", "proved"),
    GalleryCase("adversarial/quantifier_boundary_break.fsl", "verify --depth 3", "violated", "invariant"),
    pytest.param(
        GalleryCase(
            "adversarial/refine_mapping_boundary_map.fsl",
            "refine refine_mapping_boundary_impl.fsl refine_mapping_boundary_abs.fsl refine_mapping_boundary_map.fsl --depth 2",
            "refinement_failed",
            "map_out_of_bounds",
        ),
        marks=pytest.mark.xfail(
            reason="DOGFOOD-6 BUG-002: fslc reports refines when the map leaves the abstract bound",
            strict=True,
        ),
    ),
    GalleryCase("adversarial/clever_double_assignment_placement.fsl", "verify --depth 3", "error", "semantics"),
    GalleryCase("adversarial/simultaneous_leads_to_satisfaction.fsl", "verify --depth 4 --deadlock ignore", "verified"),
]


def _declared(path: Path) -> tuple[str, str, str | None]:
    command = result = kind = None
    for line in path.read_text(encoding="utf-8").splitlines()[:16]:
        stripped = line.strip()
        if stripped.startswith("// expected-command:"):
            command = stripped.split(":", 1)[1].strip()
        elif stripped.startswith("// expected-result:"):
            result = stripped.split(":", 1)[1].strip()
        elif stripped.startswith("// expected-kind:"):
            kind = stripped.split(":", 1)[1].strip()
    assert command is not None, f"{path} does not declare expected-command"
    assert result is not None, f"{path} does not declare expected-result"
    return command, result, kind


def _argv(case: GalleryCase) -> list[str]:
    path = GALLERY / case.path
    parts = shlex.split(case.command)
    if parts[0] == "check":
        return [str(ROOT / ".venv" / "bin" / "python"), "-m", "fslc", "check", str(path), *parts[1:]]
    if parts[0] == "verify":
        return [str(ROOT / ".venv" / "bin" / "python"), "-m", "fslc", "verify", str(path), *parts[1:]]
    if parts[0] == "refine":
        return [
            str(ROOT / ".venv" / "bin" / "python"),
            "-m",
            "fslc",
            "refine",
            str(path.parent / parts[1]),
            str(path.parent / parts[2]),
            str(path.parent / parts[3]),
            *parts[4:],
        ]
    raise AssertionError(f"unknown gallery command: {case.command}")


def _actual_kind(out: dict) -> str | None:
    return out.get("kind") or out.get("violation_kind")


@pytest.mark.parametrize("case", GALLERY_CASES, ids=lambda c: c.path)
def test_gallery_declared_expectation_matches_fslc(case: GalleryCase):
    path = GALLERY / case.path
    declared_command, declared_result, declared_kind = _declared(path)
    assert (declared_command, declared_result, declared_kind) == (
        case.command,
        case.expected_result,
        case.expected_kind,
    )

    proc = subprocess.run(
        _argv(case),
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        out = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise AssertionError(
            f"fslc did not return JSON for {case.path}; exit={proc.returncode}; stderr={proc.stderr}"
        ) from exc

    assert out.get("result") == case.expected_result, out
    if case.expected_kind is not None:
        assert _actual_kind(out) == case.expected_kind, out
