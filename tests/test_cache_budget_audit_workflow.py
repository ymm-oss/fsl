# SPDX-License-Identifier: Apache-2.0
"""Calibration for the parsed cache-budget-audit workflow wiring contract."""
from __future__ import annotations

import copy
import importlib.util
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "cache-budget-audit.yml"
VALIDATOR_PATH = REPO_ROOT / ".github" / "scripts" / "validate-cache-budget-audit-workflow.py"


def _load_validator():
    spec = importlib.util.spec_from_file_location("cache_budget_audit_workflow", VALIDATOR_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


validator = _load_validator()


def _real_workflow() -> dict[str, Any]:
    document = validator.load_workflow(WORKFLOW_PATH)
    assert isinstance(document, dict)
    return document


def _audit_job(document: dict[str, Any]) -> dict[str, Any]:
    return document["jobs"][validator.AUDIT_JOB_ID]


def _live_audit_step(document: dict[str, Any]) -> dict[str, Any]:
    return next(
        step
        for step in _audit_job(document)["steps"]
        if step.get("name") == validator.LIVE_AUDIT_STEP_NAME
    )


def _append_true(document: dict[str, Any]) -> None:
    _live_audit_step(document)["run"] += " || true"


def _step_continue_on_error(document: dict[str, Any]) -> None:
    _live_audit_step(document)["continue-on-error"] = True


def _job_continue_on_error(document: dict[str, Any]) -> None:
    _audit_job(document)["continue-on-error"] = True


def _step_if(document: dict[str, Any]) -> None:
    _live_audit_step(document)["if"] = "always()"


def _remove_permission(permission: str) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        del document["permissions"][permission]

    return mutate


def _remove_trigger(trigger: str) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        del document["on"][trigger]

    return mutate


def _replace_main_branch(document: dict[str, Any]) -> None:
    document["on"]["push"]["branches"] = ["nonexistent-branch"]


def _remove_watched_path(path: str) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        document["on"]["push"]["paths"].remove(path)

    return mutate


def _replace_workflow_with_list(_document: dict[str, Any]) -> list[str]:
    return ["not a workflow mapping"]


def _set_on_to_list(document: dict[str, Any]) -> None:
    document["on"] = []


def _empty_schedule(document: dict[str, Any]) -> None:
    document["on"]["schedule"] = []


def _disable_workflow_dispatch(document: dict[str, Any]) -> None:
    document["on"]["workflow_dispatch"] = False


def _set_workflow_dispatch_to_list(document: dict[str, Any]) -> None:
    document["on"]["workflow_dispatch"] = []


def _set_push_to_list(document: dict[str, Any]) -> None:
    document["on"]["push"] = []


def _set_push_paths_to_mapping(document: dict[str, Any]) -> None:
    document["on"]["push"]["paths"] = {}


def _remove_jobs(document: dict[str, Any]) -> None:
    del document["jobs"]


def _set_audit_job_to_list(document: dict[str, Any]) -> None:
    document["jobs"][validator.AUDIT_JOB_ID] = []


def _remove_steps(document: dict[str, Any]) -> None:
    del _audit_job(document)["steps"]


def _remove_named_step(document: dict[str, Any], name: str) -> None:
    _audit_job(document)["steps"] = [
        step for step in _audit_job(document)["steps"] if step.get("name") != name
    ]


def _remove_calibration_step(document: dict[str, Any]) -> None:
    _remove_named_step(document, validator.CALIBRATION_STEP_NAME)


def _remove_live_audit_step(document: dict[str, Any]) -> None:
    _remove_named_step(document, validator.LIVE_AUDIT_STEP_NAME)


def _move_calibration_after_live_audit(document: dict[str, Any]) -> None:
    steps = _audit_job(document)["steps"]
    calibration_index = next(
        index for index, step in enumerate(steps) if step.get("name") == validator.CALIBRATION_STEP_NAME
    )
    calibration = steps.pop(calibration_index)
    live_index = next(
        index for index, step in enumerate(steps) if step.get("name") == validator.LIVE_AUDIT_STEP_NAME
    )
    steps.insert(live_index + 1, calibration)


def _remove_live_audit_run(document: dict[str, Any]) -> None:
    del _live_audit_step(document)["run"]


Mutation = tuple[str, Callable[[dict[str, Any]], Any], str]
MUTATIONS: list[Mutation] = [
    ("replace workflow with a YAML list", _replace_workflow_with_list, "workflow must be a mapping"),
    ("set on to a list", _set_on_to_list, "workflow 'on' must be a mapping"),
    ("empty schedule", _empty_schedule, "trigger 'schedule' must be a non-empty list"),
    ("disable workflow_dispatch", _disable_workflow_dispatch, "trigger 'workflow_dispatch' must be enabled"),
    ("set workflow_dispatch to a list", _set_workflow_dispatch_to_list, "trigger 'workflow_dispatch' must be an event mapping"),
    ("set push to a list", _set_push_to_list, "trigger 'push' must be a mapping"),
    ("set push paths to a mapping", _set_push_paths_to_mapping, "trigger 'push.paths' must be a list"),
    ("remove jobs", _remove_jobs, "workflow jobs must be a mapping"),
    ("set audit job to a list", _set_audit_job_to_list, "job 'audit' must be a mapping"),
    ("remove steps", _remove_steps, "audit job steps must be a list"),
    ("remove calibration step", _remove_calibration_step, "audit job must contain exactly one named calibration step"),
    ("remove live-audit step", _remove_live_audit_step, "audit job must contain exactly one named live-audit step"),
    ("move calibration after live audit", _move_calibration_after_live_audit, "calibration step must precede the live-audit step"),
    ("remove live-audit run", _remove_live_audit_run, "live-audit command must be exactly 'node .github/scripts/run-cache-budget-audit.mjs'"),
    ("append || true to the live command", _append_true, "live-audit command must be exactly 'node .github/scripts/run-cache-budget-audit.mjs'"),
    ("set live-step continue-on-error", _step_continue_on_error, "live-audit step must not declare 'continue-on-error'"),
    ("set audit-job continue-on-error", _job_continue_on_error, "audit job must not declare 'continue-on-error'"),
    ("add an if to the live step", _step_if, "live-audit step must not declare 'if'"),
    *[
        (
            f"remove permission {permission}",
            _remove_permission(permission),
            "top-level permissions must be exactly actions: read and contents: read",
        )
        for permission in sorted(validator.REQUIRED_PERMISSIONS)
    ],
    *[
        (f"remove trigger {trigger}", _remove_trigger(trigger), f"required trigger '{trigger}' is absent")
        for trigger in validator.REQUIRED_TRIGGERS
    ],
    ("replace push main branch", _replace_main_branch, "trigger 'push.branches' must contain 'main'"),
    *[
        (f"remove watched path {path}", _remove_watched_path(path), f"required watched path '{path}' is absent")
        for path in sorted(validator.REQUIRED_WATCHED_PATHS)
    ],
]


def test_real_cache_budget_audit_workflow_is_accepted():
    assert validator.validate_workflow(_real_workflow()) == []


@pytest.mark.parametrize(("description", "mutate", "expected"), MUTATIONS, ids=[item[0] for item in MUTATIONS])
def test_each_single_wiring_mutation_is_rejected(
    description: str, mutate: Callable[[dict[str, Any]], Any], expected: str
):
    document = copy.deepcopy(_real_workflow())
    mutated = mutate(document)
    errors = validator.validate_workflow(document if mutated is None else mutated)
    assert errors == [expected], f"{description}: expected {expected!r}, got {errors!r}"


def _run_cli(path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(VALIDATOR_PATH), "--workflow", str(path)],
        check=False,
        capture_output=True,
        text=True,
    )


def _assert_cli_failure(path: Path, expected: str) -> None:
    result = _run_cli(path)
    output = result.stdout + result.stderr
    assert result.returncode == 1, output
    assert result.stdout == f"cache-budget-audit workflow wiring: FAIL -- {expected}\n"
    assert "Traceback" not in output


def test_cli_reports_missing_workflow_file(tmp_path: Path):
    path = tmp_path / "missing.yml"
    _assert_cli_failure(path, f"workflow file '{path}' does not exist")


def test_cli_reports_unparseable_yaml(tmp_path: Path):
    path = tmp_path / "invalid.yml"
    path.write_text("on: [\n", encoding="utf-8")
    result = _run_cli(path)
    output = result.stdout + result.stderr
    assert result.returncode == 1, output
    assert result.stdout.startswith("cache-budget-audit workflow wiring: FAIL -- workflow YAML is invalid:")
    assert "Traceback" not in output


@pytest.mark.parametrize(
    ("description", "mutate", "expected"),
    [
        ("list document", _replace_workflow_with_list, "workflow must be a mapping"),
        ("mapping without jobs", _remove_jobs, "workflow jobs must be a mapping"),
        ("audit job without steps", _remove_steps, "audit job steps must be a list"),
        ("live audit without run", _remove_live_audit_run, "live-audit command must be exactly 'node .github/scripts/run-cache-budget-audit.mjs'"),
    ],
)
def test_cli_reports_malformed_workflow_shape(
    tmp_path: Path, description: str, mutate: Callable[[dict[str, Any]], Any], expected: str
):
    document = copy.deepcopy(_real_workflow())
    mutated = mutate(document)
    path = tmp_path / f"{description.replace(' ', '_')}.yml"
    path.write_text(yaml.safe_dump(document if mutated is None else mutated), encoding="utf-8")
    _assert_cli_failure(path, expected)
