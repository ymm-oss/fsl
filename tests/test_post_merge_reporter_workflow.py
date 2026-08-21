# SPDX-License-Identifier: Apache-2.0

"""Calibration controls for the privileged post-merge reporter workflow."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

MODULE_PATH = (
    Path(__file__).resolve().parents[1]
    / ".github"
    / "scripts"
    / "validate_post_merge_reporter_workflow.py"
)
SPEC = importlib.util.spec_from_file_location("validate_post_merge_reporter_workflow", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)


def trusted_events(tmp_path: Path) -> Path:
    path = tmp_path / "events.json"
    path.write_text('["push", "schedule", "workflow_dispatch"]\n', encoding="utf-8")
    return path


def condition(*, schedule: str = "github.event.workflow_run.event == 'schedule'") -> str:
    return "\n".join(
        [
            "github.event.workflow_run.head_repository.full_name == github.repository &&",
            "github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
            "(github.event.workflow_run.event == 'push' ||",
            f"{schedule} ||",
            "github.event.workflow_run.event == 'workflow_dispatch')",
        ]
    )


def workflow(
    *,
    reconcile_condition: str | None = None,
    reconcile_steps: str | None = None,
    extra_jobs: str = "",
    reconcile_name: str = "reconcile",
) -> str:
    condition_text = reconcile_condition or condition()
    steps = reconcile_steps or f"      - run: {validator.WRITER_COMMAND}"
    return f"""name: probe
on:
  workflow_run:
    workflows: [product gate]
    types: [completed]
jobs:
  {reconcile_name}:
    if: >-
{''.join(f'      {line}\n' for line in condition_text.splitlines())}    runs-on: ubuntu-latest
    steps:
{steps}
{extra_jobs}"""


def audit(tmp_path: Path, content: str) -> list[str]:
    path = tmp_path / "post-merge-ci-reporter.yml"
    path.write_text(content, encoding="utf-8")
    return validator.audit_workflow(path, trusted_events(tmp_path))


def assert_rejected(errors: list[str], diagnostic: str) -> None:
    assert errors, "the mutation must not pass silently"
    assert any(diagnostic in error for error in errors), errors


def test_committed_reporter_workflow_satisfies_the_privileged_shape_contract() -> None:
    errors = validator.audit_workflow(validator.WORKFLOW_PATH, validator.TRUSTED_EVENTS_PATH)
    assert errors == []


def test_legitimately_folded_trusted_condition_is_accepted(tmp_path: Path) -> None:
    # Formatting changes are YAML semantics, not a reason to reject the contract.
    folded = "\n".join(
        [
            "github.event.workflow_run.head_repository.full_name == github.repository &&",
            "github.event.workflow_run.head_branch ==",
            "github.event.repository.default_branch &&",
            "(github.event.workflow_run.event == 'push' ||",
            "github.event.workflow_run.event == 'schedule' ||",
            "github.event.workflow_run.event == 'workflow_dispatch')",
        ]
    )
    assert audit(tmp_path, workflow(reconcile_condition=folded)) == []


def test_commented_out_predicate_cannot_counterfeit_schedule(tmp_path: Path) -> None:
    # The original whole-file regex scan accepted a comment as the live predicate.
    mutated = workflow(
        reconcile_condition="\n".join(
            [
                "github.event.workflow_run.head_repository.full_name == github.repository &&",
                "github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
                "(github.event.workflow_run.event == 'push' ||",
                "github.event.workflow_run.event == 'workflow_dispatch')",
            ]
        )
    ).replace("jobs:\n", "# github.event.workflow_run.event == 'schedule'\njobs:\n")
    assert_rejected(audit(tmp_path, mutated), "must equal the shared trusted-event condition exactly")


def test_schedule_and_false_is_not_the_trusted_condition(tmp_path: Path) -> None:
    # A token-set comparison missed this actionlint-valid semantic negation.
    errors = audit(
        tmp_path,
        workflow(
            reconcile_condition=condition(
                schedule="github.event.workflow_run.event == 'schedule' && false"
            )
        ),
    )
    assert_rejected(errors, "must equal the shared trusted-event condition exactly")


def test_blank_line_does_not_hide_a_negating_scalar_suffix(tmp_path: Path) -> None:
    # The line scanner stopped at the blank line even though YAML retained the suffix.
    suffixed = f"{condition()}\n\n&& github.event.workflow_run.event != 'schedule'"
    errors = audit(tmp_path, workflow(reconcile_condition=suffixed))
    assert_rejected(errors, "must equal the shared trusted-event condition exactly")


def test_renamed_live_writer_and_reconcile_decoy_is_rejected(tmp_path: Path) -> None:
    # The old writer-substring finder inspected the decoy rather than the live job.
    decoy_steps = "      - run: 'true'"
    decoy = workflow(reconcile_steps=decoy_steps)
    live = workflow(
        reconcile_name="actual_writer",
        reconcile_condition="github.event.workflow_run.event == 'push'",
    )
    mutated = live.replace("jobs:\n", "jobs:\n" + decoy.split("jobs:\n", 1)[1])
    assert_rejected(audit(tmp_path, mutated), "must contain exactly one direct literal")


def test_environment_built_writer_is_rejected(tmp_path: Path) -> None:
    # A shell sees this as the writer, but static contract inspection must not.
    steps = f"""      - env:
          REPORTER: .github/scripts/report-post-merge-ci.mjs
        run: node \"$REPORTER\""""
    errors = audit(tmp_path, workflow(reconcile_steps=steps))
    assert_rejected(errors, "must not invoke the reporter through environment or shell indirection")


def test_shell_indirection_is_rejected(tmp_path: Path) -> None:
    # The direct command text must not be reconstructed by a shell expansion.
    steps = "      - run: 'node \"$(printf %s .github/scripts/report-post-merge-ci.mjs)\"'"
    errors = audit(tmp_path, workflow(reconcile_steps=steps))
    assert_rejected(errors, "must not invoke the reporter through environment or shell indirection")


def test_composite_action_cannot_hide_the_writer(tmp_path: Path) -> None:
    # The previous text search could not decide what a local composite executes.
    steps = "      - uses: ./.github/actions/post-merge-reporter"
    errors = audit(tmp_path, workflow(reconcile_steps=steps))
    assert_rejected(errors, "must not invoke a local composite action")


def test_writer_in_a_second_job_is_rejected(tmp_path: Path) -> None:
    # A second writer would have a separate, unchecked gate.
    extra = f"""  other:
    runs-on: ubuntu-latest
    steps:
      - run: {validator.WRITER_COMMAND}
"""
    errors = audit(tmp_path, workflow(extra_jobs=extra))
    assert_rejected(errors, "may appear only in `reconcile`, not `other`")


def test_missing_reconcile_job_fails_closed(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow(reconcile_name="renamed"))
    assert_rejected(errors, "must declare the `reconcile` job")


def test_invalid_yaml_fails_closed(tmp_path: Path) -> None:
    errors = audit(tmp_path, "jobs:\n  reconcile: [\n")
    assert_rejected(errors, "invalid YAML")
