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
REPORTER_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "cache-budget-audit-reporter.yml"
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


def _audit_checkout_step(document: dict[str, Any]) -> dict[str, Any]:
    return _audit_job(document)["steps"][0]


def _audit_setup_node_step(document: dict[str, Any]) -> dict[str, Any]:
    return _audit_job(document)["steps"][1]


def _audit_calibration_step(document: dict[str, Any]) -> dict[str, Any]:
    return _audit_job(document)["steps"][2]


def _real_reporter_workflow() -> dict[str, Any]:
    document = validator.load_workflow(REPORTER_WORKFLOW_PATH)
    assert isinstance(document, dict)
    return document


def _reporter_job(document: dict[str, Any]) -> dict[str, Any]:
    return document["jobs"][validator.REPORTER_JOB_ID]


def _reporter_checkout_step(document: dict[str, Any]) -> dict[str, Any]:
    return next(
        step
        for step in _reporter_job(document)["steps"]
        if step.get("uses") == validator.REPORTER_CHECKOUT_ACTION
    )


def _reporter_runner_step(document: dict[str, Any]) -> dict[str, Any]:
    return next(
        step
        for step in _reporter_job(document)["steps"]
        if step.get("name") == validator.REPORTER_RUNNER_STEP_NAME
    )


def _reporter_setup_node_step(document: dict[str, Any]) -> dict[str, Any]:
    return _reporter_job(document)["steps"][1]


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


def _add_audit_issue_write_permission(document: dict[str, Any]) -> None:
    document["permissions"]["issues"] = "write"


def _add_audit_workflow_key(document: dict[str, Any]) -> None:
    document["defaults"] = {}


def _rename_audit_workflow(document: dict[str, Any]) -> None:
    document["name"] = "renamed cache audit"


def _add_audit_job(document: dict[str, Any]) -> None:
    document["jobs"]["attacker"] = {
        "permissions": {"issues": "write"},
        "runs-on": "ubuntu-latest",
        "steps": [{"run": "printf compromised"}],
    }


def _audit_job_permissions_override(document: dict[str, Any]) -> None:
    _audit_job(document)["permissions"] = {"issues": "write"}


def _add_audit_job_key(document: dict[str, Any]) -> None:
    _audit_job(document)["defaults"] = {"run": {"shell": "bash"}}


def _add_audit_trigger(document: dict[str, Any]) -> None:
    document["on"]["pull_request"] = {}


def _add_audit_push_key(document: dict[str, Any]) -> None:
    document["on"]["push"]["tags"] = ["v*"]


def _set_audit_concurrency_to_list(document: dict[str, Any]) -> None:
    document["concurrency"] = []


def _add_audit_concurrency_key(document: dict[str, Any]) -> None:
    document["concurrency"]["extra"] = True


def _replace_audit_concurrency_group(document: dict[str, Any]) -> None:
    document["concurrency"]["group"] = "wrong-group"


def _enable_audit_cancel_in_progress(document: dict[str, Any]) -> None:
    document["concurrency"]["cancel-in-progress"] = True


def _replace_audit_job_name(document: dict[str, Any]) -> None:
    _audit_job(document)["name"] = "wrong name"


def _replace_audit_runs_on(document: dict[str, Any]) -> None:
    _audit_job(document)["runs-on"] = "macos-latest"


def _replace_audit_timeout(document: dict[str, Any]) -> None:
    _audit_job(document)["timeout-minutes"] = 6


def _insert_audit_shell_step(document: dict[str, Any]) -> None:
    _audit_job(document)["steps"].insert(3, {"run": "printf compromised"})


def _add_audit_checkout_config(key: str, value: Any) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        _audit_checkout_step(document)["with"][key] = value

    return mutate


def _add_audit_checkout_step_key(key: str, value: Any) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        _audit_checkout_step(document)[key] = value

    return mutate


def _checkout_audit_mutable_action_ref(document: dict[str, Any]) -> None:
    _audit_checkout_step(document)["uses"] = "actions/checkout@main"


def _checkout_audit_persists_credentials(document: dict[str, Any]) -> None:
    _audit_checkout_step(document)["with"]["persist-credentials"] = True


def _add_audit_setup_node_step_key(document: dict[str, Any]) -> None:
    _audit_setup_node_step(document)["id"] = "setup-node"


def _setup_node_audit_mutable_action_ref(document: dict[str, Any]) -> None:
    _audit_setup_node_step(document)["uses"] = "actions/setup-node@main"


def _setup_node_audit_unexpected_config(document: dict[str, Any]) -> None:
    _audit_setup_node_step(document)["with"]["cache"] = "npm"


def _add_calibration_step_key(document: dict[str, Any]) -> None:
    _audit_calibration_step(document)["shell"] = "bash"


def _append_true_to_calibration_command(document: dict[str, Any]) -> None:
    _audit_calibration_step(document)["run"] += " || true"


def _replace_audit_live_token(document: dict[str, Any]) -> None:
    _live_audit_step(document)["env"]["GITHUB_TOKEN"] = "${{ github.token }}"


def _add_audit_live_step_key(document: dict[str, Any]) -> None:
    _live_audit_step(document)["shell"] = "bash"


def _replace_reporter_with_list(_document: dict[str, Any]) -> list[str]:
    return ["not a reporter workflow mapping"]


def _add_reporter_workflow_key(document: dict[str, Any]) -> None:
    document["defaults"] = {}


def _set_reporter_on_to_list(document: dict[str, Any]) -> None:
    document["on"] = []


def _set_reporter_workflow_run_to_list(document: dict[str, Any]) -> None:
    document["on"]["workflow_run"] = []


def _add_reporter_trigger(document: dict[str, Any]) -> None:
    document["on"]["push"] = {"branches": ["main"]}


def _add_reporter_workflow_run_key(document: dict[str, Any]) -> None:
    document["on"]["workflow_run"]["branches"] = ["main"]


def _replace_reporter_source(document: dict[str, Any]) -> None:
    document["on"]["workflow_run"]["workflows"] = ["wrong audit"]


def _rename_reporter_subscription(document: dict[str, Any]) -> None:
    document["on"]["workflow_run"]["workflows"] = ["renamed cache audit"]


def _replace_reporter_types(document: dict[str, Any]) -> None:
    document["on"]["workflow_run"]["types"] = ["requested"]


def _remove_reporter_permission(permission: str) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        del document["permissions"][permission]

    return mutate


def _add_reporter_permission(document: dict[str, Any]) -> None:
    document["permissions"]["pull-requests"] = "read"


def _set_reporter_concurrency_to_list(document: dict[str, Any]) -> None:
    document["concurrency"] = []


def _add_reporter_concurrency_key(document: dict[str, Any]) -> None:
    document["concurrency"]["extra"] = True


def _replace_reporter_concurrency_group(document: dict[str, Any]) -> None:
    document["concurrency"]["group"] = "wrong-group"


def _enable_reporter_cancel_in_progress(document: dict[str, Any]) -> None:
    document["concurrency"]["cancel-in-progress"] = True


def _remove_reporter_jobs(document: dict[str, Any]) -> None:
    del document["jobs"]


def _add_reporter_job(document: dict[str, Any]) -> None:
    document["jobs"]["attacker"] = {"runs-on": "ubuntu-latest", "steps": []}


def _set_reporter_job_to_list(document: dict[str, Any]) -> None:
    document["jobs"][validator.REPORTER_JOB_ID] = []


def _replace_reporter_condition(document: dict[str, Any]) -> None:
    _reporter_job(document)["if"] = "github.event.workflow_run.event == 'push'"


def _replace_reporter_job_name(document: dict[str, Any]) -> None:
    _reporter_job(document)["name"] = "wrong name"


def _replace_reporter_runs_on(document: dict[str, Any]) -> None:
    _reporter_job(document)["runs-on"] = "macos-latest"


def _replace_reporter_timeout(document: dict[str, Any]) -> None:
    _reporter_job(document)["timeout-minutes"] = 6


def _reporter_job_continue_on_error(document: dict[str, Any]) -> None:
    _reporter_job(document)["continue-on-error"] = True


def _reporter_job_permissions_override(document: dict[str, Any]) -> None:
    _reporter_job(document)["permissions"] = validator.REPORTER_PERMISSIONS.copy()


def _add_reporter_job_key(document: dict[str, Any]) -> None:
    _reporter_job(document)["defaults"] = {"run": {"shell": "bash"}}


def _remove_reporter_steps(document: dict[str, Any]) -> None:
    del _reporter_job(document)["steps"]


def _insert_reporter_shell_step(document: dict[str, Any]) -> None:
    _reporter_job(document)["steps"].insert(2, {"run": "printf compromised"})


def _add_checkout_key(key: str, value: Any) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        _reporter_checkout_step(document)["with"][key] = value

    return mutate


def _add_checkout_step_key(key: str, value: Any) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        _reporter_checkout_step(document)[key] = value

    return mutate


def _remove_reporter_checkout(document: dict[str, Any]) -> None:
    _reporter_job(document)["steps"] = [
        step
        for step in _reporter_job(document)["steps"]
        if step.get("uses") != validator.REPORTER_CHECKOUT_ACTION
    ]


def _checkout_triggering_sha(document: dict[str, Any]) -> None:
    _reporter_checkout_step(document)["with"]["ref"] = "${{ github.event.workflow_run.head_sha }}"


def _checkout_mutable_action_ref(document: dict[str, Any]) -> None:
    _reporter_checkout_step(document)["uses"] = "actions/checkout@main"


def _checkout_persists_credentials(document: dict[str, Any]) -> None:
    _reporter_checkout_step(document)["with"]["persist-credentials"] = True


def _setup_node_mutable_action_ref(document: dict[str, Any]) -> None:
    _reporter_setup_node_step(document)["uses"] = "actions/setup-node@main"


def _setup_node_unexpected_config(document: dict[str, Any]) -> None:
    _reporter_setup_node_step(document)["with"]["cache"] = "npm"


def _add_reporter_setup_node_step_key(document: dict[str, Any]) -> None:
    _reporter_setup_node_step(document)["id"] = "setup-node"


def _remove_reporter_runner(document: dict[str, Any]) -> None:
    _reporter_job(document)["steps"] = [
        step
        for step in _reporter_job(document)["steps"]
        if step.get("name") != validator.REPORTER_RUNNER_STEP_NAME
    ]


def _reporter_runner_if(document: dict[str, Any]) -> None:
    _reporter_runner_step(document)["if"] = "always()"


def _reporter_runner_continue_on_error(document: dict[str, Any]) -> None:
    _reporter_runner_step(document)["continue-on-error"] = True


def _append_true_to_reporter_runner(document: dict[str, Any]) -> None:
    _reporter_runner_step(document)["run"] += " || true"


def _replace_reporter_runner_token(document: dict[str, Any]) -> None:
    _reporter_runner_step(document)["env"]["GITHUB_TOKEN"] = "${{ github.token }}"


def _add_runner_key(key: str, value: Any) -> Callable[[dict[str, Any]], None]:
    def mutate(document: dict[str, Any]) -> None:
        _reporter_runner_step(document)[key] = value

    return mutate


Mutation = tuple[str, Callable[[dict[str, Any]], Any], str]
MUTATIONS: list[Mutation] = [
    ("replace workflow with a YAML list", _replace_workflow_with_list, "workflow must be a mapping"),
    ("add audit workflow key", _add_audit_workflow_key, "workflow must not declare unapproved keys: defaults"),
    ("rename audit workflow", _rename_audit_workflow, "audit workflow name must be exactly 'cache budget audit'"),
    ("set on to a list", _set_on_to_list, "workflow 'on' must be a mapping"),
    ("empty schedule", _empty_schedule, "trigger 'schedule' must be a non-empty list"),
    ("disable workflow_dispatch", _disable_workflow_dispatch, "trigger 'workflow_dispatch' must be enabled"),
    ("set workflow_dispatch to a list", _set_workflow_dispatch_to_list, "trigger 'workflow_dispatch' must be an event mapping"),
    ("set push to a list", _set_push_to_list, "trigger 'push' must be a mapping"),
    ("add audit trigger", _add_audit_trigger, "workflow 'on' must not declare unapproved keys: pull_request"),
    ("add audit push key", _add_audit_push_key, "trigger 'push' must not declare unapproved keys: tags"),
    ("set push paths to a mapping", _set_push_paths_to_mapping, "trigger 'push.paths' must be a list"),
    ("remove jobs", _remove_jobs, "workflow jobs must be a mapping"),
    ("add audit job", _add_audit_job, "workflow jobs must not declare unapproved keys: attacker"),
    ("set audit job to a list", _set_audit_job_to_list, "job 'audit' must be a mapping"),
    ("set audit job permissions", _audit_job_permissions_override, "audit job must not declare a permissions override"),
    ("add audit job key", _add_audit_job_key, "audit job must not declare unapproved keys: defaults"),
    ("set audit concurrency to a list", _set_audit_concurrency_to_list, "audit concurrency must be a mapping"),
    ("add audit concurrency key", _add_audit_concurrency_key, "audit concurrency must not declare unapproved keys: extra"),
    ("replace audit concurrency group", _replace_audit_concurrency_group, "audit concurrency group must be 'cache-budget-audit'"),
    ("enable audit cancellation", _enable_audit_cancel_in_progress, "audit concurrency cancel-in-progress must be false"),
    ("replace audit job name", _replace_audit_job_name, "audit job name must be exactly 'audit Actions cache budget'"),
    ("replace audit runs-on", _replace_audit_runs_on, "audit job runs-on must be exactly 'ubuntu-latest'"),
    ("replace audit timeout", _replace_audit_timeout, "audit job timeout-minutes must be exactly 5"),
    ("remove steps", _remove_steps, "audit job steps must be a list"),
    ("remove calibration step", _remove_calibration_step, "audit job must contain exactly one named calibration step"),
    ("remove live-audit step", _remove_live_audit_step, "audit job must contain exactly one named live-audit step"),
    ("move calibration after live audit", _move_calibration_after_live_audit, "calibration step must precede the live-audit step"),
    ("insert arbitrary audit shell step", _insert_audit_shell_step, "audit job steps must be exactly checkout, setup-node, calibration, and live-audit runner in that order"),
    *[
        (
            f"add audit checkout configuration {key}",
            _add_audit_checkout_config(key, value),
            f"audit checkout configuration must not declare unapproved keys: {key}",
        )
        for key, value in [
            ("repository", "attacker/public-repo"),
            ("path", "attacker"),
            ("token", "${{ secrets.GITHUB_TOKEN }}"),
            ("fetch-depth", 0),
            ("submodules", True),
        ]
    ],
    *[
        (
            f"add audit checkout step {key}",
            _add_audit_checkout_step_key(key, value),
            f"audit checkout step must not declare unapproved keys: {key}",
        )
        for key, value in [
            ("if", "always()"),
            ("env", {"X": "1"}),
            ("id", "checkout"),
            ("timeout-minutes", 1),
            ("continue-on-error", True),
        ]
    ],
    ("checkout audit mutable action ref", _checkout_audit_mutable_action_ref, "audit checkout action must be pinned to the approved commit"),
    ("persist audit checkout credentials", _checkout_audit_persists_credentials, "audit checkout must disable persisted credentials"),
    ("add audit setup-node step key", _add_audit_setup_node_step_key, "audit setup-node step must not declare unapproved keys: id"),
    ("setup-node audit mutable action ref", _setup_node_audit_mutable_action_ref, "audit setup-node action must be pinned to the approved commit"),
    ("setup-node audit unexpected config", _setup_node_audit_unexpected_config, "audit setup-node step must use exactly node-version 22"),
    ("add audit calibration step key", _add_calibration_step_key, "audit calibration step must not declare unapproved keys: shell"),
    ("append || true to calibration command", _append_true_to_calibration_command, "audit calibration command must be exactly 'node --test .github/scripts/audit-cache-budget.test.mjs'"),
    ("remove live-audit run", _remove_live_audit_run, "live-audit command must be exactly 'node .github/scripts/run-cache-budget-audit.mjs'"),
    ("append || true to the live command", _append_true, "live-audit command must be exactly 'node .github/scripts/run-cache-budget-audit.mjs'"),
    ("set live-step continue-on-error", _step_continue_on_error, "live-audit step must not declare 'continue-on-error'"),
    ("set audit-job continue-on-error", _job_continue_on_error, "audit job must not declare 'continue-on-error'"),
    ("add an if to the live step", _step_if, "live-audit step must not declare 'if'"),
    ("replace audit live token", _replace_audit_live_token, "live-audit step must bind GITHUB_TOKEN to secrets.GITHUB_TOKEN"),
    ("add audit live step key", _add_audit_live_step_key, "live-audit step must not declare unapproved keys: shell"),
    (
        "add issues write to the audit",
        _add_audit_issue_write_permission,
        "top-level permissions must be exactly actions: read and contents: read",
    ),
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


ReporterMutation = tuple[str, Callable[[dict[str, Any]], Any], str]
REPORTER_MUTATIONS: list[ReporterMutation] = [
    ("replace reporter with a YAML list", _replace_reporter_with_list, "reporter workflow must be a mapping"),
    ("add reporter workflow key", _add_reporter_workflow_key, "reporter workflow must not declare unapproved keys: defaults"),
    ("set reporter on to a list", _set_reporter_on_to_list, "reporter workflow 'on' must be a mapping"),
    ("set reporter workflow_run to a list", _set_reporter_workflow_run_to_list, "reporter trigger 'workflow_run' must be a mapping"),
    ("add reporter trigger", _add_reporter_trigger, "reporter workflow 'on' must not declare unapproved keys: push"),
    ("add reporter workflow_run key", _add_reporter_workflow_run_key, "reporter trigger 'workflow_run' must not declare unapproved keys: branches"),
    ("replace reporter workflow source", _replace_reporter_source, f"reporter workflow_run.workflows must be exactly ['{validator.AUDIT_WORKFLOW_NAME}']"),
    ("rename reporter subscription", _rename_reporter_subscription, f"reporter workflow_run.workflows must be exactly ['{validator.AUDIT_WORKFLOW_NAME}']"),
    ("replace reporter workflow types", _replace_reporter_types, "reporter workflow_run.types must be exactly ['completed']"),
    *[
        (
            f"remove reporter permission {permission}",
            _remove_reporter_permission(permission),
            "reporter permissions must be exactly actions: read, contents: read, and issues: write",
        )
        for permission in sorted(validator.REPORTER_PERMISSIONS)
    ],
    (
        "add reporter permission pull-requests",
        _add_reporter_permission,
        "reporter permissions must be exactly actions: read, contents: read, and issues: write",
    ),
    ("set reporter concurrency to a list", _set_reporter_concurrency_to_list, "reporter concurrency must be a mapping"),
    ("add reporter concurrency key", _add_reporter_concurrency_key, "reporter concurrency must not declare unapproved keys: extra"),
    ("replace reporter concurrency group", _replace_reporter_concurrency_group, "reporter concurrency group must be 'cache-budget-audit-reporter'"),
    ("enable reporter cancellation", _enable_reporter_cancel_in_progress, "reporter concurrency cancel-in-progress must be false"),
    ("remove reporter jobs", _remove_reporter_jobs, "reporter workflow jobs must be a mapping"),
    ("add reporter job", _add_reporter_job, "reporter workflow jobs must not declare unapproved keys: attacker"),
    ("set reporter job to a list", _set_reporter_job_to_list, "reporter job 'reconcile' must be a mapping"),
    ("set reporter job permissions", _reporter_job_permissions_override, "reporter job must not declare a permissions override"),
    ("add reporter job key", _add_reporter_job_key, "reporter job must not declare unapproved keys: defaults"),
    ("set reporter job continue-on-error", _reporter_job_continue_on_error, "reporter job must not declare 'continue-on-error'"),
    ("replace reporter job name", _replace_reporter_job_name, "reporter job name must be exactly 'reconcile cache-budget audit issue'"),
    ("replace reporter runs-on", _replace_reporter_runs_on, "reporter job runs-on must be exactly 'ubuntu-latest'"),
    ("replace reporter timeout", _replace_reporter_timeout, "reporter job timeout-minutes must be exactly 5"),
    ("replace trusted reporter condition", _replace_reporter_condition, "reporter job must restrict to trusted default-branch schedule, push, or workflow_dispatch audits"),
    ("remove reporter steps", _remove_reporter_steps, "reporter job steps must be a list"),
    ("remove reporter checkout", _remove_reporter_checkout, "reporter job steps must be exactly checkout, setup-node, and reconciliation runner in that order"),
    ("insert arbitrary reporter shell step", _insert_reporter_shell_step, "reporter job steps must be exactly checkout, setup-node, and reconciliation runner in that order"),
    *[
        (
            f"add checkout configuration {key}",
            _add_checkout_key(key, value),
            f"reporter checkout configuration must not declare unapproved keys: {key}",
        )
        for key, value in [
            ("repository", "attacker/public-repo"),
            ("path", "attacker"),
            ("token", "${{ secrets.GITHUB_TOKEN }}"),
            ("fetch-depth", 0),
            ("submodules", True),
        ]
    ],
    *[
        (
            f"add checkout step {key}",
            _add_checkout_step_key(key, value),
            f"reporter checkout step must not declare unapproved keys: {key}",
        )
        for key, value in [
            ("if", "always()"),
            ("env", {"X": "1"}),
            ("id", "checkout"),
            ("timeout-minutes", 1),
            ("continue-on-error", True),
        ]
    ],
    ("checkout triggering SHA", _checkout_triggering_sha, "reporter checkout must use the repository default branch ref"),
    ("checkout mutable action ref", _checkout_mutable_action_ref, "reporter checkout action must be pinned to the approved commit"),
    ("persist checkout credentials", _checkout_persists_credentials, "reporter checkout must disable persisted credentials"),
    ("setup-node mutable action ref", _setup_node_mutable_action_ref, "reporter setup-node action must be pinned to the approved commit"),
    ("add reporter setup-node step key", _add_reporter_setup_node_step_key, "reporter setup-node step must not declare unapproved keys: id"),
    ("setup-node unexpected config", _setup_node_unexpected_config, "reporter setup-node step must use exactly node-version 22"),
    ("remove reporter runner", _remove_reporter_runner, "reporter job steps must be exactly checkout, setup-node, and reconciliation runner in that order"),
    ("replace reporter runner token", _replace_reporter_runner_token, "reporter reconciliation runner must bind GITHUB_TOKEN to secrets.GITHUB_TOKEN"),
    *[
        (
            f"add reporter runner {key}",
            _add_runner_key(key, value),
            f"reporter reconciliation runner must not declare unapproved keys: {key}",
        )
        for key, value in [("shell", "bash"), ("working-directory", "attacker"), ("id", "report")]
    ],
    ("add an if to reporter runner", _reporter_runner_if, "reporter reconciliation runner must not declare 'if'"),
    ("set reporter runner continue-on-error", _reporter_runner_continue_on_error, "reporter reconciliation runner must not declare 'continue-on-error'"),
    ("append || true to reporter runner", _append_true_to_reporter_runner, "reporter reconciliation command must be exactly 'node .github/scripts/report-cache-budget-audit.mjs'"),
]


def test_real_cache_budget_audit_workflow_is_accepted():
    assert validator.validate_workflow(_real_workflow()) == []


def test_real_cache_budget_audit_reporter_workflow_is_accepted():
    assert validator.validate_reporter_workflow(_real_reporter_workflow()) == []


def test_audit_workflow_name_rejects_a_third_same_named_workflow(tmp_path: Path):
    (tmp_path / WORKFLOW_PATH.name).write_text(WORKFLOW_PATH.read_text(encoding="utf-8"), encoding="utf-8")
    (tmp_path / REPORTER_WORKFLOW_PATH.name).write_text(
        REPORTER_WORKFLOW_PATH.read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    (tmp_path / "unrelated-cache-audit.yml").write_text(
        f"name: {validator.AUDIT_WORKFLOW_NAME}\n",
        encoding="utf-8",
    )

    assert validator.validate_audit_workflow_name_uniqueness(tmp_path) == [
        "audit workflow name 'cache budget audit' must be unique; found in: "
        "cache-budget-audit.yml, unrelated-cache-audit.yml"
    ]


@pytest.mark.parametrize(("description", "mutate", "expected"), MUTATIONS, ids=[item[0] for item in MUTATIONS])
def test_each_single_wiring_mutation_is_rejected(
    description: str, mutate: Callable[[dict[str, Any]], Any], expected: str
):
    document = copy.deepcopy(_real_workflow())
    mutated = mutate(document)
    errors = validator.validate_workflow(document if mutated is None else mutated)
    assert errors == [expected], f"{description}: expected {expected!r}, got {errors!r}"


@pytest.mark.parametrize(
    ("description", "mutate", "expected"),
    REPORTER_MUTATIONS,
    ids=[item[0] for item in REPORTER_MUTATIONS],
)
def test_each_single_reporter_wiring_mutation_is_rejected(
    description: str, mutate: Callable[[dict[str, Any]], Any], expected: str
):
    document = copy.deepcopy(_real_reporter_workflow())
    mutated = mutate(document)
    errors = validator.validate_reporter_workflow(document if mutated is None else mutated)
    assert errors == [expected], f"{description}: expected {expected!r}, got {errors!r}"


def _run_cli(
    path: Path, reporter_path: Path | None = None
) -> subprocess.CompletedProcess[str]:
    command = [sys.executable, str(VALIDATOR_PATH), "--workflow", str(path)]
    if reporter_path is not None:
        command.extend(["--reporter-workflow", str(reporter_path)])
    return subprocess.run(
        command,
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


def test_cli_rejects_a_duplicate_yaml_key_before_wiring_validation(tmp_path: Path):
    path = tmp_path / "duplicate-key.yml"
    source = WORKFLOW_PATH.read_text(encoding="utf-8")
    path.write_text(
        source.replace(
            "          persist-credentials: false",
            "          persist-credentials: true\n          persist-credentials: false",
            1,
        ),
        encoding="utf-8",
    )

    result = _run_cli(path)
    output = result.stdout + result.stderr

    assert result.returncode == 1, output
    assert result.stdout.startswith(
        "cache-budget-audit workflow wiring: FAIL -- workflow YAML is invalid:"
    )
    assert "found duplicate key 'persist-credentials'" in result.stdout
    assert "Traceback" not in output


def test_cli_reports_missing_reporter_workflow_file(tmp_path: Path):
    path = tmp_path / "missing-reporter.yml"
    result = _run_cli(WORKFLOW_PATH, path)
    output = result.stdout + result.stderr
    assert result.returncode == 1, output
    assert result.stdout == f"cache-budget-audit workflow wiring: FAIL -- reporter workflow file '{path}' does not exist\n"
    assert "Traceback" not in output


def test_cli_reports_unparseable_reporter_workflow_yaml(tmp_path: Path):
    path = tmp_path / "invalid-reporter.yml"
    path.write_text("on: [\n", encoding="utf-8")
    result = _run_cli(WORKFLOW_PATH, path)
    output = result.stdout + result.stderr
    assert result.returncode == 1, output
    assert result.stdout.startswith(
        "cache-budget-audit workflow wiring: FAIL -- reporter workflow YAML is invalid:"
    )
    assert "Traceback" not in output


def test_cli_reports_unreadable_reporter_workflow_file(tmp_path: Path):
    path = tmp_path / "reporter-directory"
    path.mkdir()
    result = _run_cli(WORKFLOW_PATH, path)
    output = result.stdout + result.stderr
    assert result.returncode == 1, output
    assert result.stdout.startswith(
        f"cache-budget-audit workflow wiring: FAIL -- reporter workflow file '{path}' could not be read:"
    )
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
