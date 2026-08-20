# SPDX-License-Identifier: Apache-2.0
"""Calibration for the parsed cache-budget-audit workflow wiring contract."""
from __future__ import annotations

import copy
import importlib.util
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

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


Mutation = tuple[str, Callable[[dict[str, Any]], None]]
MUTATIONS: list[Mutation] = [
    ("append || true to the live command", _append_true),
    ("set live-step continue-on-error", _step_continue_on_error),
    ("set audit-job continue-on-error", _job_continue_on_error),
    ("add an if to the live step", _step_if),
    *[
        (f"remove permission {permission}", _remove_permission(permission))
        for permission in sorted(validator.REQUIRED_PERMISSIONS)
    ],
    *[
        (f"remove trigger {trigger}", _remove_trigger(trigger))
        for trigger in validator.REQUIRED_TRIGGERS
    ],
    ("replace push main branch", _replace_main_branch),
    *[
        (f"remove watched path {path}", _remove_watched_path(path))
        for path in sorted(validator.REQUIRED_WATCHED_PATHS)
    ],
]


def test_real_cache_budget_audit_workflow_is_accepted():
    assert validator.validate_workflow(_real_workflow()) == []


@pytest.mark.parametrize(("description", "mutate"), MUTATIONS, ids=[item[0] for item in MUTATIONS])
def test_each_single_wiring_mutation_is_rejected(
    description: str, mutate: Callable[[dict[str, Any]], None]
):
    document = copy.deepcopy(_real_workflow())
    mutate(document)
    errors = validator.validate_workflow(document)
    assert errors, f"{description}: validator accepted the mutated workflow"
