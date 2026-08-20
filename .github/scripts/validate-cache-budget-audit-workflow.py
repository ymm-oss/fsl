#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Validate the semantic wiring of the Actions cache-budget audit workflow.

The workflow is parsed before it is inspected so formatting, comments, key
order, quoting, and plain-versus-block scalar style do not affect this
contract.  The matching calibration suite derives each rejecting fixture from
the live workflow rather than carrying a second workflow snapshot.
"""
from __future__ import annotations

import argparse
import copy
import re
import shlex
from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "cache-budget-audit.yml"
DEFAULT_REPORTER_WORKFLOW = (
    REPO_ROOT / ".github" / "workflows" / "cache-budget-audit-reporter.yml"
)
AUDIT_JOB_ID = "audit"
CALIBRATION_STEP_NAME = "Calibrate the audit"
LIVE_AUDIT_STEP_NAME = "Audit the live cache budget"
LIVE_AUDIT_COMMAND = ["node", ".github/scripts/run-cache-budget-audit.mjs"]
REQUIRED_PERMISSIONS = {"actions": "read", "contents": "read"}
REPORTER_JOB_ID = "reconcile"
REPORTER_PERMISSIONS = {"actions": "read", "contents": "read", "issues": "write"}
REPORTER_WORKFLOW_RUN_SOURCE = "cache budget audit"
REPORTER_WORKFLOW_RUN_TYPES = ["completed"]
REPORTER_CONCURRENCY_GROUP = "cache-budget-audit-reporter"
REPORTER_CHECKOUT_ACTION = "actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5"
REPORTER_CHECKOUT_REF = "${{ github.event.repository.default_branch }}"
REPORTER_SETUP_NODE_ACTION = "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020"
REPORTER_NODE_VERSION = "22"
REPORTER_TOKEN = "${{ secrets.GITHUB_TOKEN }}"
REPORTER_RUNNER_STEP_NAME = "Create, update, or resolve cache budget audit issue"
REPORTER_COMMAND = ["node", ".github/scripts/report-cache-budget-audit.mjs"]
REPORTER_TRUSTED_CONDITION = " ".join(
    [
        "github.event.workflow_run.head_repository.full_name == github.repository &&",
        "github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
        "(github.event.workflow_run.event == 'push' ||",
        "github.event.workflow_run.event == 'schedule' ||",
        "github.event.workflow_run.event == 'workflow_dispatch')",
    ]
)
REQUIRED_TRIGGERS = ("schedule", "workflow_dispatch", "push")
REQUIRED_WATCHED_PATHS = {
    ".github/scripts/audit-cache-budget.mjs",
    ".github/scripts/audit-cache-budget.test.mjs",
    ".github/scripts/run-cache-budget-audit.mjs",
    ".github/scripts/report-cache-budget-audit.mjs",
    ".github/scripts/report-cache-budget-audit.test.mjs",
    ".github/workflows/cache-budget-audit.yml",
    ".github/workflows/cache-budget-audit-reporter.yml",
    ".github/workflows/ci.yml",
    ".github/workflows/merge-readiness.yml",
}


class WorkflowLoader(yaml.SafeLoader):
    """SafeLoader with YAML 1.2 boolean spelling for GitHub Actions keys."""


# PyYAML's YAML 1.1 resolver turns the Actions key ``on`` into ``True``.  GitHub
# Actions uses YAML 1.2 spelling, where only true/false are booleans, so retain
# those values while treating ``on`` as the string key it is in the workflow.
WorkflowLoader.yaml_implicit_resolvers = copy.deepcopy(yaml.SafeLoader.yaml_implicit_resolvers)
for first_character, resolvers in WorkflowLoader.yaml_implicit_resolvers.items():
    WorkflowLoader.yaml_implicit_resolvers[first_character] = [
        resolver for resolver in resolvers if resolver[0] != "tag:yaml.org,2002:bool"
    ]
WorkflowLoader.add_implicit_resolver(
    "tag:yaml.org,2002:bool",
    re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"),
    list("tTfF"),
)


def load_workflow(path: Path) -> Any:
    """Parse a workflow with the GitHub Actions-compatible safe loader."""
    return yaml.load(path.read_text(encoding="utf-8"), Loader=WorkflowLoader)


def _mapping(value: Any, label: str, errors: list[str]) -> Optional[Mapping[str, Any]]:
    if not isinstance(value, Mapping):
        errors.append(f"{label} must be a mapping")
        return None
    return value


def _named_step_indices(steps: list[Any], name: str) -> list[int]:
    return [
        index
        for index, step in enumerate(steps)
        if isinstance(step, Mapping) and step.get("name") == name
    ]


def _normalized_argv(command: Any) -> list[str] | None:
    if not isinstance(command, str):
        return None

    # A trailing newline is how a one-line YAML block scalar is represented.
    # Interior line breaks are shell command separators, so they cannot be an
    # argv-equivalent spelling of the single allowlisted command.
    normalized = command.strip()
    if "\n" in normalized or "\r" in normalized:
        return None
    try:
        return shlex.split(normalized, posix=True, comments=False)
    except ValueError:
        return None


def validate_workflow(document: Any) -> list[str]:
    """Return every violated cache-budget-audit wiring contract."""
    errors: list[str] = []
    workflow = _mapping(document, "workflow", errors)
    if workflow is None:
        return errors

    triggers = _mapping(workflow.get("on"), "workflow 'on'", errors)
    if triggers is not None:
        for trigger in REQUIRED_TRIGGERS:
            if trigger not in triggers:
                errors.append(f"required trigger '{trigger}' is absent")

        schedule = triggers.get("schedule")
        if "schedule" in triggers and (not isinstance(schedule, list) or not schedule):
            errors.append("trigger 'schedule' must be a non-empty list")

        # GitHub Actions' bare ``workflow_dispatch:`` spelling is an enabled
        # event and PyYAML represents it as None.  Treat it as enabled while
        # rejecting an explicit false value or a malformed configuration.
        dispatch = triggers.get("workflow_dispatch")
        if "workflow_dispatch" in triggers and dispatch is False:
            errors.append("trigger 'workflow_dispatch' must be enabled")
        elif "workflow_dispatch" in triggers and dispatch is not None and not isinstance(
            dispatch, Mapping
        ):
            errors.append("trigger 'workflow_dispatch' must be an event mapping")

        push = triggers.get("push")
        if "push" in triggers:
            push_mapping = _mapping(push, "trigger 'push'", errors)
            if push_mapping is not None:
                branches = push_mapping.get("branches")
                if not isinstance(branches, list) or "main" not in branches:
                    errors.append("trigger 'push.branches' must contain 'main'")
                paths = push_mapping.get("paths")
                if not isinstance(paths, list):
                    errors.append("trigger 'push.paths' must be a list")
                else:
                    missing_paths = REQUIRED_WATCHED_PATHS.difference(paths)
                    for path in sorted(missing_paths):
                        errors.append(f"required watched path '{path}' is absent")

    if workflow.get("permissions") != REQUIRED_PERMISSIONS:
        errors.append("top-level permissions must be exactly actions: read and contents: read")

    jobs = _mapping(workflow.get("jobs"), "workflow jobs", errors)
    if jobs is None:
        return errors
    job = _mapping(jobs.get(AUDIT_JOB_ID), f"job '{AUDIT_JOB_ID}'", errors)
    if job is None:
        return errors
    if "continue-on-error" in job:
        errors.append("audit job must not declare 'continue-on-error'")

    steps_value = job.get("steps")
    if not isinstance(steps_value, list):
        errors.append("audit job steps must be a list")
        return errors
    steps = steps_value

    calibration_indices = _named_step_indices(steps, CALIBRATION_STEP_NAME)
    live_audit_indices = _named_step_indices(steps, LIVE_AUDIT_STEP_NAME)
    if len(calibration_indices) != 1:
        errors.append("audit job must contain exactly one named calibration step")
    if len(live_audit_indices) != 1:
        errors.append("audit job must contain exactly one named live-audit step")
        return errors

    live_audit_index = live_audit_indices[0]
    if calibration_indices and calibration_indices[0] >= live_audit_index:
        errors.append("calibration step must precede the live-audit step")

    # _named_step_indices selects only Mapping instances, so the unique live
    # step above is necessarily a mapping.  Keeping a defensive non-mapping
    # branch here would create an unreachable, uncalibrated diagnostic.
    live_step = steps[live_audit_index]
    if "if" in live_step:
        errors.append("live-audit step must not declare 'if'")
    if "continue-on-error" in live_step:
        errors.append("live-audit step must not declare 'continue-on-error'")
    if _normalized_argv(live_step.get("run")) != LIVE_AUDIT_COMMAND:
        errors.append(
            "live-audit command must be exactly "
            "'node .github/scripts/run-cache-budget-audit.mjs'"
        )

    return errors


def _normalized_text(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    return " ".join(value.split())


def validate_reporter_workflow(document: Any) -> list[str]:
    """Return every violated cache-budget-audit reporter wiring contract."""
    errors: list[str] = []
    workflow = _mapping(document, "reporter workflow", errors)
    if workflow is None:
        return errors

    triggers = _mapping(workflow.get("on"), "reporter workflow 'on'", errors)
    if triggers is not None:
        workflow_run = _mapping(
            triggers.get("workflow_run"), "reporter trigger 'workflow_run'", errors
        )
        if workflow_run is not None:
            if workflow_run.get("workflows") != [REPORTER_WORKFLOW_RUN_SOURCE]:
                errors.append(
                    "reporter workflow_run.workflows must be exactly ['cache budget audit']"
                )
            if workflow_run.get("types") != REPORTER_WORKFLOW_RUN_TYPES:
                errors.append("reporter workflow_run.types must be exactly ['completed']")

    if workflow.get("permissions") != REPORTER_PERMISSIONS:
        errors.append(
            "reporter permissions must be exactly actions: read, contents: read, and issues: write"
        )

    concurrency = _mapping(workflow.get("concurrency"), "reporter concurrency", errors)
    if concurrency is not None:
        if concurrency.get("group") != REPORTER_CONCURRENCY_GROUP:
            errors.append("reporter concurrency group must be 'cache-budget-audit-reporter'")
        if concurrency.get("cancel-in-progress") is not False:
            errors.append("reporter concurrency cancel-in-progress must be false")

    jobs = _mapping(workflow.get("jobs"), "reporter workflow jobs", errors)
    if jobs is None:
        return errors
    job = _mapping(jobs.get(REPORTER_JOB_ID), f"reporter job '{REPORTER_JOB_ID}'", errors)
    if job is None:
        return errors

    if "permissions" in job:
        errors.append("reporter job must not declare a permissions override")
    if "continue-on-error" in job:
        errors.append("reporter job must not declare 'continue-on-error'")
    if _normalized_text(job.get("if")) != REPORTER_TRUSTED_CONDITION:
        errors.append(
            "reporter job must restrict to trusted default-branch schedule, push, or workflow_dispatch audits"
        )

    steps_value = job.get("steps")
    if not isinstance(steps_value, list):
        errors.append("reporter job steps must be a list")
        return errors
    steps = steps_value
    if not (
        len(steps) == 3
        and all(isinstance(step, Mapping) for step in steps)
        and isinstance(steps[0].get("uses"), str)
        and steps[0]["uses"].startswith("actions/checkout@")
        and isinstance(steps[1].get("uses"), str)
        and steps[1]["uses"].startswith("actions/setup-node@")
        and steps[2].get("name") == REPORTER_RUNNER_STEP_NAME
    ):
        errors.append(
            "reporter job steps must be exactly checkout, setup-node, and reconciliation runner in that order"
        )
        return errors

    checkout = steps[0]
    if checkout.get("uses") != REPORTER_CHECKOUT_ACTION:
        errors.append("reporter checkout action must be pinned to the approved commit")
    else:
        checkout_with = checkout.get("with")
        if not isinstance(checkout_with, Mapping) or checkout_with.get("ref") != REPORTER_CHECKOUT_REF:
            errors.append("reporter checkout must use the repository default branch ref")
        if not isinstance(checkout_with, Mapping) or checkout_with.get("persist-credentials") is not False:
            errors.append("reporter checkout must disable persisted credentials")

    setup_node = steps[1]
    if setup_node.get("uses") != REPORTER_SETUP_NODE_ACTION:
        errors.append("reporter setup-node action must be pinned to the approved commit")
    if setup_node.get("with") != {"node-version": REPORTER_NODE_VERSION}:
        errors.append("reporter setup-node step must use exactly node-version 22")

    runner = steps[2]
    if runner.get("env") != {"GITHUB_TOKEN": REPORTER_TOKEN}:
        errors.append("reporter reconciliation runner must bind GITHUB_TOKEN to secrets.GITHUB_TOKEN")
    if "if" in runner:
        errors.append("reporter reconciliation runner must not declare 'if'")
    if "continue-on-error" in runner:
        errors.append("reporter reconciliation runner must not declare 'continue-on-error'")
    if _normalized_argv(runner.get("run")) != REPORTER_COMMAND:
        errors.append(
            "reporter reconciliation command must be exactly "
            "'node .github/scripts/report-cache-budget-audit.mjs'"
        )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=DEFAULT_WORKFLOW)
    parser.add_argument("--reporter-workflow", type=Path, default=DEFAULT_REPORTER_WORKFLOW)
    args = parser.parse_args()

    try:
        document = load_workflow(args.workflow)
    except FileNotFoundError:
        errors = [f"workflow file '{args.workflow}' does not exist"]
    except OSError as error:
        errors = [f"workflow file '{args.workflow}' could not be read: {error.strerror}"]
    except yaml.YAMLError as error:
        errors = [f"workflow YAML is invalid: {error}"]
    else:
        errors = validate_workflow(document)
    try:
        reporter_document = load_workflow(args.reporter_workflow)
    except FileNotFoundError:
        errors.append(f"reporter workflow file '{args.reporter_workflow}' does not exist")
    except OSError as error:
        errors.append(
            f"reporter workflow file '{args.reporter_workflow}' could not be read: {error.strerror}"
        )
    except yaml.YAMLError as error:
        errors.append(f"reporter workflow YAML is invalid: {error}")
    else:
        errors.extend(validate_reporter_workflow(reporter_document))
    if errors:
        for error in errors:
            print(f"cache-budget-audit workflow wiring: FAIL -- {error}")
        return 1
    print("cache-budget-audit workflow wiring: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
