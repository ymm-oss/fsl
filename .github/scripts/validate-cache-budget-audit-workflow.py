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
AUDIT_JOB_ID = "audit"
CALIBRATION_STEP_NAME = "Calibrate the audit"
LIVE_AUDIT_STEP_NAME = "Audit the live cache budget"
LIVE_AUDIT_COMMAND = ["node", ".github/scripts/run-cache-budget-audit.mjs"]
REQUIRED_PERMISSIONS = {"actions": "read", "contents": "read"}
REQUIRED_TRIGGERS = ("schedule", "workflow_dispatch", "push")
REQUIRED_WATCHED_PATHS = {
    ".github/scripts/audit-cache-budget.mjs",
    ".github/scripts/audit-cache-budget.test.mjs",
    ".github/scripts/run-cache-budget-audit.mjs",
    ".github/workflows/cache-budget-audit.yml",
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

    live_step = steps[live_audit_index]
    if not isinstance(live_step, Mapping):
        errors.append("named live-audit step must be a mapping")
        return errors
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=DEFAULT_WORKFLOW)
    args = parser.parse_args()

    errors = validate_workflow(load_workflow(args.workflow))
    if errors:
        for error in errors:
            print(f"cache-budget-audit workflow wiring: FAIL -- {error}")
        return 1
    print("cache-budget-audit workflow wiring: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
