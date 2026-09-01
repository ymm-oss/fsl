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
    steps = reconcile_steps or f"""      - uses: {validator.CHECKOUT_ACTION}
        with:
          ref: {validator.CHECKOUT_REF}
          persist-credentials: false

      - uses: {validator.SETUP_NODE_ACTION}
        with:
          node-version: \"{validator.NODE_VERSION}\"

      - name: {validator.CALIBRATION_STEP_NAME}
        run: {validator.CALIBRATION_COMMAND}

      - name: {validator.WRITER_STEP_NAME}
        env:
          GITHUB_TOKEN: {validator.GITHUB_TOKEN}
        run: {validator.WRITER_COMMAND}"""
    indented_condition = "".join(f"      {line}\n" for line in condition_text.splitlines())
    return f"""name: {validator.WORKFLOW_NAME}
on:
  workflow_run:
    workflows: [product gate]
    types: [completed]
permissions:
  actions: read
  contents: read
  issues: write
  pull-requests: read
concurrency:
  group: {validator.CONCURRENCY_GROUP}
  cancel-in-progress: false
jobs:
  {reconcile_name}:
    name: {validator.RECONCILE_JOB_NAME}
    if: >-
{indented_condition}    runs-on: {validator.RECONCILE_RUNS_ON}
    timeout-minutes: {validator.RECONCILE_TIMEOUT_MINUTES}
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
    decoy = workflow()
    live = workflow(
        reconcile_name="actual_writer",
        reconcile_condition="github.event.workflow_run.event == 'push'",
    )
    mutated = live.replace("jobs:\n", "jobs:\n" + decoy.split("jobs:\n", 1)[1])
    assert_rejected(audit(tmp_path, mutated), "workflow jobs must not declare unapproved keys: actual_writer")


def test_environment_built_writer_is_rejected(tmp_path: Path) -> None:
    # A shell sees this as the writer, but static contract inspection must not.
    errors = audit(
        tmp_path,
        workflow().replace(
            f"          GITHUB_TOKEN: {validator.GITHUB_TOKEN}\n        run: {validator.WRITER_COMMAND}",
            '          REPORTER: .github/scripts/report-post-merge-ci.mjs\n        run: node "$REPORTER"',
        ),
    )
    assert_rejected(errors, "writer environment must not declare unapproved keys: REPORTER")


def test_shell_indirection_is_rejected(tmp_path: Path) -> None:
    # The direct command text must not be reconstructed by a shell expansion.
    errors = audit(
        tmp_path,
        workflow().replace(
            validator.WRITER_COMMAND,
            'node "$(printf %s .github/scripts/report-post-merge-ci.mjs)"',
        ),
    )
    assert_rejected(errors, "writer command must be exactly")


def test_composite_action_cannot_hide_the_writer(tmp_path: Path) -> None:
    # The previous text search could not decide what a local composite executes.
    errors = audit(
        tmp_path,
        workflow().replace(
            f"        run: {validator.WRITER_COMMAND}",
            "        uses: ./.github/actions/post-merge-reporter",
        ),
    )
    assert_rejected(errors, "reporter writer step must not declare unapproved keys: uses")


def test_writer_in_a_second_job_is_rejected(tmp_path: Path) -> None:
    # A second writer would have a separate, unchecked gate.
    extra = f"""  other:
    runs-on: ubuntu-latest
    steps:
      - run: {validator.WRITER_COMMAND}
"""
    errors = audit(tmp_path, workflow(extra_jobs=extra))
    assert_rejected(errors, "workflow jobs must not declare unapproved keys: other")


def test_alternate_spelling_of_writer_in_a_second_job_is_rejected(tmp_path: Path) -> None:
    # The workflow permits no second job, irrespective of how its shell spells the writer path.
    extra = """  second_writer:
    runs-on: ubuntu-latest
    steps:
      - env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: node ./.github/scripts/report-post-merge-ci.mjs
"""
    errors = audit(tmp_path, workflow(extra_jobs=extra))
    assert_rejected(errors, "workflow jobs must not declare unapproved keys: second_writer")


def test_writer_step_cannot_be_disabled_with_an_if_condition(tmp_path: Path) -> None:
    errors = audit(
        tmp_path,
        workflow().replace(
            f"      - name: {validator.WRITER_STEP_NAME}\n",
            f"      - name: {validator.WRITER_STEP_NAME}\n"
            "        if: github.event.workflow_run.event == 'pull_request'\n",
        ),
    )
    assert_rejected(errors, "reporter writer step must not declare unapproved keys: if")


def test_missing_reconcile_job_fails_closed(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow(reconcile_name="renamed"))
    assert_rejected(errors, "must declare the `reconcile` job")


def test_invalid_yaml_fails_closed(tmp_path: Path) -> None:
    errors = audit(tmp_path, "jobs:\n  reconcile: [\n")
    assert_rejected(errors, "invalid YAML")


def test_checkout_repository_override_is_rejected(tmp_path: Path) -> None:
    # An allowlist catches this security-critical input without naming it as dangerous.
    errors = audit(
        tmp_path,
        workflow().replace(
            f"          ref: {validator.CHECKOUT_REF}\n",
            f"          repository: octocat/Hello-World\n          ref: {validator.CHECKOUT_REF}\n",
        ),
    )
    assert_rejected(errors, "checkout configuration must not declare unapproved keys: repository")


def test_checkout_action_must_stay_at_the_approved_sha(tmp_path: Path) -> None:
    errors = audit(
        tmp_path,
        workflow().replace(validator.CHECKOUT_ACTION, "actions/checkout@0000000000000000000000000000000000000000"),
    )
    assert_rejected(errors, "checkout action must be exactly")


def test_checkout_ref_must_remain_the_default_branch_expression(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow().replace(validator.CHECKOUT_REF, "master"))
    assert_rejected(errors, "checkout ref must be exactly")


def test_checkout_must_disable_persisted_credentials(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow().replace("persist-credentials: false", "persist-credentials: true"))
    assert_rejected(errors, "checkout persist-credentials must be exactly 'false'")


def test_checkout_requires_persist_credentials_input(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow().replace("          persist-credentials: false\n", ""))
    assert_rejected(errors, "checkout persist-credentials must be exactly 'false'")


def test_extra_step_before_writer_is_rejected(tmp_path: Path) -> None:
    errors = audit(
        tmp_path,
        workflow().replace(
            f"      - name: {validator.WRITER_STEP_NAME}",
            "      - name: Alter checked-out tree\n        run: echo altered\n\n"
            f"      - name: {validator.WRITER_STEP_NAME}",
        ),
    )
    assert_rejected(errors, "steps must be exactly checkout, setup-node")


def test_reordered_steps_are_rejected(tmp_path: Path) -> None:
    checkout = f"""      - uses: {validator.CHECKOUT_ACTION}
        with:
          ref: {validator.CHECKOUT_REF}
          persist-credentials: false"""
    setup_node = f"""      - uses: {validator.SETUP_NODE_ACTION}
        with:
          node-version: \"{validator.NODE_VERSION}\""""
    errors = audit(tmp_path, workflow().replace(f"{checkout}\n\n{setup_node}", f"{setup_node}\n\n{checkout}"))
    assert_rejected(errors, "steps must be exactly checkout, setup-node")


def test_unexpected_step_key_fails_closed(tmp_path: Path) -> None:
    # ``audience`` is not a danger-specific denylist entry; no unapproved key is allowed.
    errors = audit(
        tmp_path,
        workflow().replace('          node-version: "22"\n', '          node-version: "22"\n        audience: internal\n'),
    )
    assert_rejected(errors, "setup-node step must not declare unapproved keys: audience")


def test_job_permissions_override_is_rejected(tmp_path: Path) -> None:
    errors = audit(
        tmp_path,
        workflow().replace(
            f"    timeout-minutes: {validator.RECONCILE_TIMEOUT_MINUTES}\n",
            f"    timeout-minutes: {validator.RECONCILE_TIMEOUT_MINUTES}\n    permissions:\n      contents: write\n",
        ),
    )
    assert_rejected(errors, "must not declare a permissions override")


def test_workflow_permissions_cannot_gain_additional_authority(tmp_path: Path) -> None:
    errors = audit(tmp_path, workflow().replace("  contents: read\n", "  contents: write\n"))
    assert_rejected(errors, "workflow permissions.contents must be exactly 'read'")


def test_workflow_default_shell_is_rejected(tmp_path: Path) -> None:
    errors = audit(
        tmp_path,
        workflow().replace("jobs:\n", "defaults:\n  run:\n    shell: bash -e {0}\njobs:\n"),
    )
    assert_rejected(errors, "workflow must not declare unapproved keys: defaults")
