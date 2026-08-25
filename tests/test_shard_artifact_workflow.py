# SPDX-License-Identifier: Apache-2.0

"""Calibrated controls for the parser-backed shard artifact workflow audit."""

from __future__ import annotations

import copy
import importlib.util
import sys
from pathlib import Path

import pytest

MODULE_PATH = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "validate-shard-artifact-workflow.py"
SPEC = importlib.util.spec_from_file_location("validate_shard_artifact_workflow", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)


@pytest.fixture()
def workflow() -> dict:
    return copy.deepcopy(validator.load_workflow(validator.WORKFLOW))


def step(document: dict, job: str, name: str) -> dict:
    return next(item for item in document["jobs"][job]["steps"] if item.get("name") == name)


def assert_rejected(document: dict, diagnostic: str) -> None:
    errors = validator.audit_workflow(document)
    assert errors, "the isolated mutation must not pass silently"
    assert any(diagnostic in error for error in errors), f"produced={errors!r}; expected={diagnostic!r}"


def test_live_workflow_is_accepted() -> None:
    errors = validator.audit_workflow(validator.load_workflow(validator.WORKFLOW))
    assert errors == []


@pytest.mark.parametrize(
    ("job", "name", "diagnostic"),
    [
        ("rust-tests", "Preserve test-shard inventory", "rust-tests upload name"),
        ("semantic-mutation-operators", "Preserve operator shard evidence", "semantic-mutation-operators upload name"),
    ],
)
def test_one_sided_run_attempt_reintroduction_is_rejected(
    workflow: dict, job: str, name: str, diagnostic: str
) -> None:
    upload = step(workflow, job, name)
    upload["with"]["name"] += "-${{ github.run_attempt }}"
    assert_rejected(workflow, diagnostic)


def test_download_pattern_run_attempt_reintroduction_is_rejected(workflow: dict) -> None:
    download = step(workflow, "rust-workspace", "Download rust test-shard inventories")
    download["with"]["pattern"] += "-${{ github.run_attempt }}"
    assert_rejected(workflow, "rust-workspace download pattern")


def test_overwrite_removal_is_rejected(workflow: dict) -> None:
    upload = step(workflow, "semantic-mutation-operators", "Preserve operator shard evidence")
    upload["with"]["overwrite"] = False
    assert_rejected(workflow, "upload overwrite: expected True, actual False")


def test_provenance_step_removal_is_rejected(workflow: dict) -> None:
    steps = workflow["jobs"]["rust-tests"]["steps"]
    steps.remove(step(workflow, "rust-tests", "Record test-shard provenance"))
    assert_rejected(workflow, "expected exactly one provenance step")


def test_common_helper_bypass_is_rejected(workflow: dict) -> None:
    verify = step(workflow, "semantic-mutation", "Verify operator shard completeness")
    verify["run"] = "./tools/check-shard-union.sh full.txt shard-*.txt"
    assert_rejected(workflow, "semantic-mutation verifier")


def test_aggregator_always_guard_removal_is_rejected(workflow: dict) -> None:
    workflow["jobs"]["rust-workspace"]["if"] = "success()"
    assert_rejected(workflow, "jobs.rust-workspace.if")


def test_dependency_result_guard_removal_is_rejected(workflow: dict) -> None:
    guard = step(workflow, "semantic-mutation", "Require complete operator-shard and mutants-lane evidence")
    del guard["env"]["SEMANTIC_MUTATION_OPERATORS"]
    assert_rejected(workflow, "dependency guard env: expected 'SEMANTIC_MUTATION_OPERATORS'")


@pytest.mark.parametrize(
    ("job", "step_name", "mutated_name", "diagnostic"),
    [
        (
            "semantic-mutation-mutants",
            "Preserve mutation evidence",
            "semantic-mutation-mutants-${{ github.run_id }}",
            "semantic-mutation-mutants upload name",
        ),
        (
            "fsl-logic",
            "Preserve FSL Logic evidence",
            "fsl-logic-${{ github.run_id }}",
            "fsl-logic upload name",
        ),
    ],
)
def test_unsharded_artifact_attempt_scope_is_preserved(
    workflow: dict, job: str, step_name: str, mutated_name: str, diagnostic: str
) -> None:
    upload = step(workflow, job, step_name)
    upload["with"]["name"] = mutated_name
    assert_rejected(workflow, diagnostic)
