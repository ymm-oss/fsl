#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Validate the narrow privileged shape of the post-merge issue reporter.

The reporter has ``issues: write``.  Its workflow is therefore deliberately
small and parsed structurally: a line or substring scan cannot establish which
job GitHub actually runs, nor whether a comment or shell indirection supplies
the apparent writer command.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "post-merge-ci-reporter.yml"
TRUSTED_EVENTS_PATH = REPO_ROOT / ".github" / "scripts" / "trusted-workflow-events.json"

WORKFLOW_NAME = "post-merge CI reporter"
WORKFLOW_RUN_NAME = "product gate"
WORKFLOW_PERMISSIONS = {
    "actions": "read",
    "contents": "read",
    "issues": "write",
    "pull-requests": "read",
}
CONCURRENCY_GROUP = "post-merge-ci-reporter"
RECONCILE_JOB = "reconcile"
RECONCILE_JOB_NAME = "reconcile post-merge CI issue"
RECONCILE_RUNS_ON = "ubuntu-latest"
RECONCILE_TIMEOUT_MINUTES = "5"
CHECKOUT_ACTION = "actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5"
CHECKOUT_REF = "${{ github.event.repository.default_branch }}"
SETUP_NODE_ACTION = "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020"
NODE_VERSION = "22"
CALIBRATION_STEP_NAME = "Test reporter contract"
CALIBRATION_COMMAND = "node --test .github/scripts/report-post-merge-ci.test.mjs"
WRITER_STEP_NAME = "Create, update, or resolve CI issues"
WRITER_COMMAND = "node .github/scripts/report-post-merge-ci.mjs"
GITHUB_TOKEN = "${{ secrets.GITHUB_TOKEN }}"


class WorkflowLoader(yaml.SafeLoader):
    """Safe YAML composer that rejects duplicate keys before inspection."""

    def compose_mapping_node(self, anchor: str | None) -> yaml.MappingNode:
        node = super().compose_mapping_node(anchor)
        keys: dict[tuple[str, str], yaml.Node] = {}
        for key_node, _value_node in node.value:
            if not isinstance(key_node, yaml.ScalarNode):
                raise yaml.composer.ComposerError(
                    "while composing a mapping",
                    node.start_mark,
                    "found a non-scalar mapping key; GitHub Actions mappings must use scalar keys",
                    key_node.start_mark,
                )
            identity = (key_node.tag, key_node.value)
            previous = keys.get(identity)
            if previous is not None:
                raise yaml.composer.ComposerError(
                    "while composing a mapping",
                    previous.start_mark,
                    f"found duplicate key {key_node.value!r}",
                    key_node.start_mark,
                )
            keys[identity] = key_node
        return node


def load_yaml(path: Path) -> yaml.Node | None:
    """Compose YAML while retaining source marks for fail-closed diagnostics."""
    return yaml.compose(path.read_text(encoding="utf-8"), Loader=WorkflowLoader)


def load_trusted_events(path: Path) -> tuple[str, ...]:
    """Read the shared runtime/validator event authority."""
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, list) or not value or not all(isinstance(event, str) for event in value):
        raise ValueError(f"{path}: trusted workflow events must be a non-empty JSON string array")
    if len(set(value)) != len(value):
        raise ValueError(f"{path}: trusted workflow events must not contain duplicates")
    return tuple(value)


def expected_condition(events: tuple[str, ...]) -> str:
    """Build the exact gate from the shared trusted-event list."""
    disjunction = " || ".join(
        f"github.event.workflow_run.event == '{event}'" for event in events
    )
    return (
        "github.event.workflow_run.head_repository.full_name == github.repository && "
        "github.event.workflow_run.head_branch == github.event.repository.default_branch && "
        f"({disjunction})"
    )


def _scalar(node: yaml.Node | None) -> str | None:
    return node.value if isinstance(node, yaml.ScalarNode) else None


def _entry(node: yaml.Node | None, key: str) -> yaml.Node | None:
    if not isinstance(node, yaml.MappingNode):
        return None
    for key_node, value_node in node.value:
        if _scalar(key_node) == key:
            return value_node
    return None


def _line(node: yaml.Node | None) -> str:
    return str(node.start_mark.line + 1) if node is not None else "?"


def _normalise(expression: str) -> str:
    return " ".join(expression.split())


def _reject_unapproved_keys(
    mapping: yaml.MappingNode, allowed: set[str], label: str, errors: list[str], path: str
) -> None:
    """Fail closed on every key outside one approved structural shape."""
    unexpected = sorted(
        key
        for key_node, _value_node in mapping.value
        if (key := _scalar(key_node)) is None or key not in allowed
    )
    if unexpected:
        errors.append(
            f"{path}:{_line(mapping)}: {label} must not declare unapproved keys: "
            f"{', '.join(unexpected)}"
        )


def _expect_mapping(
    node: yaml.Node | None, label: str, errors: list[str], path: str
) -> yaml.MappingNode | None:
    if not isinstance(node, yaml.MappingNode):
        errors.append(f"{path}:{_line(node)}: {label} must be a mapping")
        return None
    return node


def _expect_scalar(
    node: yaml.Node | None, expected: str, label: str, errors: list[str], path: str
) -> None:
    if _scalar(node) != expected:
        errors.append(f"{path}:{_line(node)}: {label} must be exactly {expected!r}")


def _expect_scalar_mapping(
    node: yaml.Node | None,
    expected: dict[str, str],
    label: str,
    errors: list[str],
    path: str,
) -> None:
    mapping = _expect_mapping(node, label, errors, path)
    if mapping is None:
        return
    _reject_unapproved_keys(mapping, set(expected), label, errors, path)
    for key, value in expected.items():
        _expect_scalar(_entry(mapping, key), value, f"{label}.{key}", errors, path)


def validate_document(document: yaml.Node | None, events: tuple[str, ...], path: str) -> list[str]:
    """Return every structural contract violation in one parsed workflow."""
    errors: list[str] = []
    workflow = _expect_mapping(document, "workflow", errors, path)
    if workflow is None:
        return errors
    _reject_unapproved_keys(
        workflow,
        {"name", "on", "permissions", "concurrency", "jobs"},
        "workflow",
        errors,
        path,
    )
    _expect_scalar(_entry(workflow, "name"), WORKFLOW_NAME, "workflow name", errors, path)
    trigger = _expect_mapping(_entry(workflow, "on"), "workflow on", errors, path)
    if trigger is not None:
        _reject_unapproved_keys(trigger, {"workflow_run"}, "workflow on", errors, path)
        workflow_run = _expect_mapping(_entry(trigger, "workflow_run"), "workflow_run trigger", errors, path)
        if workflow_run is not None:
            _reject_unapproved_keys(workflow_run, {"workflows", "types"}, "workflow_run trigger", errors, path)
            workflows = _entry(workflow_run, "workflows")
            types = _entry(workflow_run, "types")
            if not isinstance(workflows, yaml.SequenceNode) or [_scalar(item) for item in workflows.value] != [WORKFLOW_RUN_NAME]:
                errors.append(f"{path}:{_line(workflows)}: workflow_run.workflows must be exactly ['{WORKFLOW_RUN_NAME}']")
            if not isinstance(types, yaml.SequenceNode) or [_scalar(item) for item in types.value] != ["completed"]:
                errors.append(f"{path}:{_line(types)}: workflow_run.types must be exactly ['completed']")
    _expect_scalar_mapping(_entry(workflow, "permissions"), WORKFLOW_PERMISSIONS, "workflow permissions", errors, path)
    concurrency = _expect_mapping(_entry(workflow, "concurrency"), "workflow concurrency", errors, path)
    if concurrency is not None:
        _reject_unapproved_keys(concurrency, {"group", "cancel-in-progress"}, "workflow concurrency", errors, path)
        _expect_scalar(_entry(concurrency, "group"), CONCURRENCY_GROUP, "workflow concurrency.group", errors, path)
        _expect_scalar(
            _entry(concurrency, "cancel-in-progress"),
            "false",
            "workflow concurrency.cancel-in-progress",
            errors,
            path,
        )

    jobs = _entry(workflow, "jobs")
    if not isinstance(jobs, yaml.MappingNode):
        return [f"{path}: workflow must declare a top-level `jobs:` mapping"]

    _reject_unapproved_keys(jobs, {RECONCILE_JOB}, "workflow jobs", errors, path)
    if _entry(jobs, RECONCILE_JOB) is None:
        return [*errors, f"{path}: privileged reporter must declare the `{RECONCILE_JOB}` job"]
    reconcile = _expect_mapping(
        _entry(jobs, RECONCILE_JOB), f"`{RECONCILE_JOB}` job", errors, path
    )
    if reconcile is None:
        return errors

    if _entry(reconcile, "permissions") is not None:
        errors.append(
            f"{path}:{_line(_entry(reconcile, 'permissions'))}: `{RECONCILE_JOB}` must not declare "
            "a permissions override"
        )
    _reject_unapproved_keys(
        reconcile,
        {"name", "if", "runs-on", "timeout-minutes", "steps"},
        f"`{RECONCILE_JOB}` job",
        errors,
        path,
    )
    _expect_scalar(_entry(reconcile, "name"), RECONCILE_JOB_NAME, "reconcile job name", errors, path)
    _expect_scalar(_entry(reconcile, "runs-on"), RECONCILE_RUNS_ON, "reconcile job runs-on", errors, path)
    _expect_scalar(
        _entry(reconcile, "timeout-minutes"),
        RECONCILE_TIMEOUT_MINUTES,
        "reconcile job timeout-minutes",
        errors,
        path,
    )

    condition = _scalar(_entry(reconcile, "if"))
    expected = expected_condition(events)
    if condition is None:
        errors.append(f"{path}:{_line(reconcile)}: `{RECONCILE_JOB}` must declare the trusted `if:` condition")
    elif _normalise(condition) != expected:
        errors.append(
            f"{path}:{_line(_entry(reconcile, 'if'))}: `{RECONCILE_JOB}` `if:` must equal "
            "the shared trusted-event condition exactly"
        )

    steps = _entry(reconcile, "steps")
    if not isinstance(steps, yaml.SequenceNode):
        errors.append(
            f"{path}:{_line(steps)}: `{RECONCILE_JOB}` steps must be a sequence"
        )
        return errors
    if not (
        len(steps.value) == 4
        and all(isinstance(step, yaml.MappingNode) for step in steps.value)
        and (_scalar(_entry(steps.value[0], "uses")) or "").startswith("actions/checkout@")
        and (_scalar(_entry(steps.value[1], "uses")) or "").startswith("actions/setup-node@")
        and _scalar(_entry(steps.value[2], "name")) == CALIBRATION_STEP_NAME
        and _scalar(_entry(steps.value[3], "name")) == WRITER_STEP_NAME
    ):
        errors.append(
            f"{path}:{_line(steps)}: `{RECONCILE_JOB}` steps must be exactly checkout, setup-node, "
            "reporter calibration, and direct writer in that order"
        )
        return errors

    checkout, setup_node, calibration, writer = steps.value
    _reject_unapproved_keys(checkout, {"uses", "with"}, "checkout step", errors, path)
    _expect_scalar(_entry(checkout, "uses"), CHECKOUT_ACTION, "checkout action", errors, path)
    checkout_with = _expect_mapping(_entry(checkout, "with"), "checkout configuration", errors, path)
    if checkout_with is not None:
        _reject_unapproved_keys(
            checkout_with, {"ref", "persist-credentials"}, "checkout configuration", errors, path
        )
        _expect_scalar(_entry(checkout_with, "ref"), CHECKOUT_REF, "checkout ref", errors, path)
        _expect_scalar(
            _entry(checkout_with, "persist-credentials"),
            "false",
            "checkout persist-credentials",
            errors,
            path,
        )

    _reject_unapproved_keys(setup_node, {"uses", "with"}, "setup-node step", errors, path)
    _expect_scalar(_entry(setup_node, "uses"), SETUP_NODE_ACTION, "setup-node action", errors, path)
    setup_with = _expect_mapping(_entry(setup_node, "with"), "setup-node configuration", errors, path)
    if setup_with is not None:
        _reject_unapproved_keys(setup_with, {"node-version"}, "setup-node configuration", errors, path)
        _expect_scalar(_entry(setup_with, "node-version"), NODE_VERSION, "setup-node version", errors, path)

    _reject_unapproved_keys(calibration, {"name", "run"}, "reporter calibration step", errors, path)
    _expect_scalar(_entry(calibration, "name"), CALIBRATION_STEP_NAME, "calibration step name", errors, path)
    _expect_scalar(_entry(calibration, "run"), CALIBRATION_COMMAND, "calibration command", errors, path)

    _reject_unapproved_keys(writer, {"name", "env", "run"}, "reporter writer step", errors, path)
    _expect_scalar(_entry(writer, "name"), WRITER_STEP_NAME, "writer step name", errors, path)
    writer_env = _expect_mapping(_entry(writer, "env"), "writer environment", errors, path)
    if writer_env is not None:
        _reject_unapproved_keys(writer_env, {"GITHUB_TOKEN"}, "writer environment", errors, path)
        _expect_scalar(_entry(writer_env, "GITHUB_TOKEN"), GITHUB_TOKEN, "writer GITHUB_TOKEN", errors, path)
    _expect_scalar(_entry(writer, "run"), WRITER_COMMAND, "writer command", errors, path)
    return errors


def audit_workflow(workflow_path: Path, trusted_events_path: Path) -> list[str]:
    """Load both authorities fail-closed, then validate the parsed workflow."""
    try:
        events = load_trusted_events(trusted_events_path)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        return [f"{trusted_events_path}: cannot load shared trusted events: {error}"]
    try:
        document = load_yaml(workflow_path)
    except OSError as error:
        return [f"{workflow_path}: could not be read: {error.strerror or error}"]
    except yaml.YAMLError as error:
        mark = getattr(error, "problem_mark", None)
        location = f":{mark.line + 1}" if mark is not None else ""
        return [f"{workflow_path}{location}: invalid YAML: {error}"]
    return validate_document(document, events, str(workflow_path))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=WORKFLOW_PATH)
    parser.add_argument("--trusted-events", type=Path, default=TRUSTED_EVENTS_PATH)
    args = parser.parse_args()
    errors = audit_workflow(args.workflow, args.trusted_events)
    if errors:
        for error in errors:
            print(f"post-merge reporter workflow: FAIL -- {error}")
        return 1
    print("post-merge reporter workflow: PASS -- privileged reconcile shape is pinned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
