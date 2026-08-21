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

RECONCILE_JOB = "reconcile"
WRITER_COMMAND = "node .github/scripts/report-post-merge-ci.mjs"


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


def _direct_writer_steps(job: yaml.MappingNode) -> list[yaml.MappingNode]:
    steps = _entry(job, "steps")
    if not isinstance(steps, yaml.SequenceNode):
        return []
    return [
        step
        for step in steps.value
        if isinstance(step, yaml.MappingNode) and _scalar(_entry(step, "run")) == WRITER_COMMAND
    ]


def _indirect_writer_steps(job: yaml.MappingNode) -> list[yaml.MappingNode]:
    """Find visible attempts to place the writer command behind shell/env syntax."""
    steps = _entry(job, "steps")
    if not isinstance(steps, yaml.SequenceNode):
        return []
    indirect: list[yaml.MappingNode] = []
    for step in steps.value:
        if not isinstance(step, yaml.MappingNode):
            continue
        run = _scalar(_entry(step, "run"))
        if run is None or run == WRITER_COMMAND:
            continue
        if WRITER_COMMAND in run:
            indirect.append(step)
            continue
        # A literal note in ``env`` is not an invocation. Only a command that
        # asks Node to evaluate a shell-expanded value is an attempted writer
        # indirection; this catches ``node \"$REPORTER\"`` without treating a
        # decoy ``env: NOTE: ...`` as executable behavior.
        if run.lstrip().startswith("node ") and "$" in run:
            indirect.append(step)
    return indirect


def _local_composite_steps(job: yaml.MappingNode) -> list[yaml.MappingNode]:
    """Local composites can hide a writer command from this static boundary."""
    steps = _entry(job, "steps")
    if not isinstance(steps, yaml.SequenceNode):
        return []
    return [
        step
        for step in steps.value
        if isinstance(step, yaml.MappingNode)
        and (_scalar(_entry(step, "uses")) or "").startswith("./")
    ]


def validate_document(document: yaml.Node | None, events: tuple[str, ...], path: str) -> list[str]:
    """Return every structural contract violation in one parsed workflow."""
    errors: list[str] = []
    jobs = _entry(document, "jobs")
    if not isinstance(jobs, yaml.MappingNode):
        return [f"{path}: workflow must declare a top-level `jobs:` mapping"]

    job_nodes = {
        name: job
        for key, job in jobs.value
        if (name := _scalar(key)) is not None and isinstance(job, yaml.MappingNode)
    }
    reconcile = job_nodes.get(RECONCILE_JOB)
    if reconcile is None:
        return [f"{path}: privileged reporter must declare the `{RECONCILE_JOB}` job"]

    condition = _scalar(_entry(reconcile, "if"))
    expected = expected_condition(events)
    if condition is None:
        errors.append(f"{path}:{_line(reconcile)}: `{RECONCILE_JOB}` must declare the trusted `if:` condition")
    elif _normalise(condition) != expected:
        errors.append(
            f"{path}:{_line(_entry(reconcile, 'if'))}: `{RECONCILE_JOB}` `if:` must equal "
            "the shared trusted-event condition exactly"
        )

    direct_in_reconcile = _direct_writer_steps(reconcile)
    if len(direct_in_reconcile) != 1:
        errors.append(
            f"{path}:{_line(reconcile)}: `{RECONCILE_JOB}` must contain exactly one direct literal "
            f"`run: {WRITER_COMMAND}` step"
        )
    for step in _indirect_writer_steps(reconcile):
        errors.append(
            f"{path}:{_line(step)}: `{RECONCILE_JOB}` must not invoke the reporter through "
            "environment or shell indirection"
        )
    for step in _local_composite_steps(reconcile):
        errors.append(
            f"{path}:{_line(step)}: `{RECONCILE_JOB}` must not invoke a local composite action; "
            "the reporter writer must remain a direct literal run step"
        )

    for job_name, job in job_nodes.items():
        if job_name == RECONCILE_JOB:
            continue
        for step in _direct_writer_steps(job):
            errors.append(
                f"{path}:{_line(step)}: reporter writer command may appear only in `{RECONCILE_JOB}`, "
                f"not `{job_name}`"
            )
        for step in _indirect_writer_steps(job):
            errors.append(
                f"{path}:{_line(step)}: reporter writer command may not be invoked through "
                f"environment or shell indirection in `{job_name}`"
            )
        for step in _local_composite_steps(job):
            errors.append(
                f"{path}:{_line(step)}: local composite actions are not permitted in `{job_name}`; "
                "they could hide the reporter writer"
            )
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
