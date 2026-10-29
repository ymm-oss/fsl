#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Validate that every ``Swatinem/rust-cache`` step uses the audited key generation.

Each step must pass ``prefix-key`` equal to ``CURRENT_KEY_PREFIX`` in
``.github/scripts/audit-cache-budget.mjs`` and ``env-vars`` equal to
``PROFILE_HASH_ENV`` in ``.github/scripts/cargo-profile-hash.mjs``, and the
step immediately before it must run that script under the same ``if:``.
Saving must exclude PR and merge-group events; unconditional restore-only steps
must retain boolean ``save-if: false`` (see "Actions cache budget" in DESIGN-ci.md).
Their identities are pinned independently in ``RESTORE_ONLY_JOBS``.

Why each half fails closed: a saving step and a restore-only step that differ
in either input compute different keys, so the restore-only lane silently goes
cold; a workflow prefix that differs from the audit's constant makes the audit
judge the wrong generation (a leftover entry would satisfy, or a current one
would fail, its default-branch presence rule); and a cache step whose hash
step did not run hashes an unset variable, so a ``[profile.*]`` edit stops
re-keying it. See docs/design/DESIGN-ci.md, "Cache key generation".

Like ``validate_toolchain_pin.py`` this composes YAML instead of scanning lines
(docs/design/DESIGN-ci.md, "Automation contracts use parser-backed workflow
inspection").
"""

from __future__ import annotations

import argparse
import copy
import re
import sys
from collections.abc import Iterable
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION = "Swatinem/rust-cache"
AUDIT_SCRIPT = Path(".github/scripts/audit-cache-budget.mjs")
HASH_SCRIPT = Path(".github/scripts/cargo-profile-hash.mjs")
HASH_COMMAND = "node .github/scripts/cargo-profile-hash.mjs rust/Cargo.toml"
SAVE_GUARD = "${{ github.event_name != 'pull_request' && github.event_name != 'merge_group' }}"
RESTORE_ONLY_JOBS = frozenset({
    ("ci.yml", "fsl-logic"), ("ci.yml", "semantic-mutation-mutants"),
    ("merge-readiness.yml", "rust-compile"), ("merge-readiness.yml", "core-contracts"),
})
# ci.yml has nine rust-cache steps and merge-readiness.yml two. This floor
# catches missing steps; restore-only identities above are an exact contract.
MINIMUM_AUDITED_STEPS = 11

PREFIX_CONSTANT = re.compile(r'^export const CURRENT_KEY_PREFIX = "([^"]+)";$', re.MULTILINE)
ENV_CONSTANT = re.compile(r'^export const PROFILE_HASH_ENV = "([^"]+)";$', re.MULTILINE)


class WorkflowLoader(yaml.SafeLoader):
    """Safe YAML composer that rejects duplicate mapping keys before inspection.

    A later duplicate ``prefix-key:`` would otherwise silently replace the value
    this audit examined (same reasoning as ``validate_toolchain_pin.py``).
    """

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


def _entry(node: yaml.Node | None, key: str) -> yaml.Node | None:
    if not isinstance(node, yaml.MappingNode):
        return None
    for key_node, value_node in node.value:
        if isinstance(key_node, yaml.ScalarNode) and key_node.value == key:
            return value_node
    return None


def _scalar(node: yaml.Node | None) -> str | None:
    return node.value.strip() if isinstance(node, yaml.ScalarNode) else None


def _restore_only(node: yaml.Node | None) -> bool:
    return (
        isinstance(node, yaml.ScalarNode)
        and node.tag == "tag:yaml.org,2002:bool"
        and node.value.lower() == "false"
    )


def read_constant(path: Path, pattern: re.Pattern[str]) -> str:
    match = pattern.search(path.read_text(encoding="utf-8"))
    if match is None:
        raise ValueError(f"{path}: no `{pattern.pattern}` declaration")
    return match.group(1)


def audit_steps(steps: yaml.Node | None, path: str, prefix: str, env_var: str, *, restore_only: bool) -> tuple[int, list[str]]:
    if not isinstance(steps, yaml.SequenceNode):
        return 0, []
    audited = 0
    errors: list[str] = []
    for index, step in enumerate(steps.value):
        uses = _scalar(_entry(step, "uses"))
        if uses is None or not uses.startswith(f"{ACTION}@"):
            continue
        audited += 1
        where = f"{path}:{step.start_mark.line + 1}"
        inputs = _entry(step, "with")
        save_if = _entry(inputs, "save-if")
        valid = _restore_only(save_if) if restore_only else _scalar(save_if) == SAVE_GUARD
        if not valid:
            errors.append(f"{where}: `save-if` must be {'boolean false' if restore_only else SAVE_GUARD}")
        if _scalar(_entry(inputs, "prefix-key")) != prefix:
            errors.append(f"{where}: `with: prefix-key:` must be {prefix!r} (audit-cache-budget.mjs CURRENT_KEY_PREFIX)")
        if _scalar(_entry(inputs, "env-vars")) != env_var:
            errors.append(f"{where}: `with: env-vars:` must be {env_var!r} (cargo-profile-hash.mjs PROFILE_HASH_ENV)")
        previous = steps.value[index - 1] if index > 0 else None
        if _scalar(_entry(previous, "run")) != HASH_COMMAND:
            errors.append(f"{where}: the step immediately before must run `{HASH_COMMAND}`")
            continue
        for override in ("working-directory", "shell", "env"):
            if _entry(previous, override) is not None:
                errors.append(f"{where}: the hash step must not set `{override}:`")
        if _scalar(_entry(previous, "if")) != _scalar(_entry(step, "if")):
            errors.append(f"{where}: the hash step's `if:` must equal the cache step's `if:`")
    return audited, errors


def _yaml_paths(directory: Path) -> list[Path]:
    iterator: Iterable[Path] = directory.iterdir() if directory.exists() else ()
    return sorted(path for path in iterator if path.is_file() and path.suffix in {".yaml", ".yml"})


def audit_repository(
    repository: Path, *, minimum_audited_steps: int = MINIMUM_AUDITED_STEPS,
    restore_only_jobs: frozenset[tuple[str, str]] = RESTORE_ONLY_JOBS,
) -> tuple[int, list[str]]:
    prefix = read_constant(repository / AUDIT_SCRIPT, PREFIX_CONSTANT)
    env_var = read_constant(repository / HASH_SCRIPT, ENV_CONSTANT)
    errors: list[str] = []
    audited = 0
    seen_restore_only = set()
    for path in _yaml_paths(repository / ".github" / "workflows"):
        relative = path.relative_to(repository).as_posix()
        try:
            document = yaml.compose(path.read_text(encoding="utf-8"), Loader=WorkflowLoader)
        except yaml.YAMLError as error:
            mark = getattr(error, "problem_mark", None)
            location = f":{mark.line + 1}" if mark is not None else ""
            errors.append(f"{relative}{location}: invalid YAML: {error}")
            continue
        jobs = _entry(document, "jobs")
        if not isinstance(jobs, yaml.MappingNode):
            continue
        for job_key, job in jobs.value:
            identity = (path.name, job_key.value)
            restore_only = identity in restore_only_jobs
            count, step_errors = audit_steps(_entry(job, "steps"), relative, prefix, env_var, restore_only=restore_only)
            if count and restore_only:
                seen_restore_only.add(identity)
            audited += count
            errors.extend(step_errors)
    if audited < minimum_audited_steps:
        errors.append(
            f"audited {audited} `{ACTION}` step(s), fewer than the {minimum_audited_steps} this audit expects"
        )
    for filename, job in sorted(restore_only_jobs - seen_restore_only):
        errors.append(f"{filename}:{job}: required restore-only rust-cache step is absent")
    return audited, errors


def calibrate_save_policy(repository: Path) -> list[str]:
    """Report live guard event tables and calibrate isolated guard/role mutations."""
    prefix = read_constant(repository / AUDIT_SCRIPT, PREFIX_CONSTANT)
    env_var = read_constant(repository / HASH_SCRIPT, ENV_CONSTANT)
    errors = []
    for filename in ("ci.yml", "merge-readiness.yml"):
        path = repository / ".github/workflows" / filename
        document = yaml.compose(path.read_text(encoding="utf-8"), Loader=WorkflowLoader)
        for job_key, job in _entry(document, "jobs").value:
            steps = _entry(job, "steps")
            for index, step in enumerate(steps.value):
                if not (_scalar(_entry(step, "uses")) or "").startswith(f"{ACTION}@"):
                    continue
                guard = _entry(_entry(step, "with"), "save-if")
                where = f"{filename}:{job_key.value}:{step.start_mark.line + 1}"
                if guard is None:
                    errors.append(f"{where}: missing save-if")
                    continue
                restore_only = (filename, job_key.value) in RESTORE_ONLY_JOBS
                outcomes = {}
                terms = re.findall(r"github\.event_name != '([^']+)'", guard.value)
                for event in ("pull_request", "merge_group", "push", "schedule", "workflow_dispatch"):
                    actual = not _restore_only(guard) and all(event != excluded for excluded in terms)
                    expected = not restore_only and event not in ("pull_request", "merge_group")
                    if actual != expected:
                        errors.append(f"{where}: {event}: expected {expected}, got {actual}")
                    outcomes[event] = actual
                mutations = [SAVE_GUARD] if restore_only else ["false", guard.value.replace(" && github.event_name != 'merge_group'", "")]
                for value in mutations:
                    mutant = copy.deepcopy(steps)
                    changed = _entry(_entry(mutant.value[index], "with"), "save-if")
                    changed.value, changed.tag = value, f"tag:yaml.org,2002:{'bool' if value == 'false' else 'str'}"
                    _, findings = audit_steps(mutant, filename, prefix, env_var, restore_only=restore_only)
                    if len(findings) != 1 or "`save-if`" not in findings[0]:
                        errors.append(f"{where}: mutant {value!r}: expected one save-if finding, got {findings}")
                errors.extend(audit_steps(steps, filename, prefix, env_var, restore_only=restore_only)[1])
                print(f"{where}: {outcomes}; {len(mutations)} guard/role mutations checked; original restored")
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repository", type=Path, default=REPO_ROOT)
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args(argv)
    try:
        audited, errors = audit_repository(args.repository)
        if args.selftest and not errors:
            errors.extend(calibrate_save_policy(args.repository))
    except (OSError, ValueError) as error:
        print(f"rust-cache key audit: FAIL -- {error}", file=sys.stderr)
        return 1
    if errors:
        print(f"rust-cache key audit: FAIL -- {len(errors)} finding(s)", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    print(f"rust-cache key audit: PASS -- {audited} `{ACTION}` step(s) use the audited key generation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
