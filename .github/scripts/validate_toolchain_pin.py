#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Validate the Rust-toolchain action pins in repository-owned Actions YAML.

This audit deliberately composes YAML instead of scanning lines.  Its predecessor
was rewritten fourteen times because quoted values, comments, folded scalars,
flow mappings, and indentation are YAML grammar, not line-oriented syntax.  The
node tree preserves source marks for diagnostics while keeping the audit tied to
the values GitHub Actions actually receives.
"""

from __future__ import annotations

import argparse
import re
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
CARGO_TOML = REPO_ROOT / "rust" / "Cargo.toml"

ACTION = "dtolnay/rust-toolchain"
EXPECTED_REF = "1.98.0"
FLOATING_REFS = frozenset({"stable", "beta", "nightly"})
COMMIT_SHA = re.compile(r"\A[0-9a-f]{40}\Z")
EXPRESSION = re.compile(r"\$\{\{")
MINIMUM_AUDITED_REFERENCES = 11

# These are exact repository-relative paths, not exemptions.  A path listed
# here is held to the release/MSRV contract and must contain a checked step.
MSRV_PATHS: Mapping[str, str] = {
    ".github/workflows/release.yml": (
        "builds release artifacts at rust/Cargo.toml's declared MSRV using a "
        "commit-pinned action"
    ),
}


class WorkflowLoader(yaml.SafeLoader):
    """Safe YAML composer that rejects duplicate keys before inspection.

    ``yaml.compose`` does not call constructors, so overriding
    ``construct_mapping`` would be a no-op here.  Reject at composer time
    instead; this keeps source marks and prevents a later ``uses:`` or ``with:``
    from silently replacing the value the audit examined.
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


@dataclass(frozen=True)
class ToolchainReference:
    """One repository-owned use of the toolchain action."""

    path: str
    line: int
    ref: str
    owner: yaml.MappingNode


def load_yaml(path: Path) -> yaml.Node | None:
    """Compose YAML with source marks and duplicate-key rejection."""
    return yaml.compose(path.read_text(encoding="utf-8"), Loader=WorkflowLoader)


def _scalar(node: yaml.Node | None) -> str | None:
    return node.value if isinstance(node, yaml.ScalarNode) else None


def _entry(node: yaml.Node | None, key: str) -> yaml.Node | None:
    if not isinstance(node, yaml.MappingNode):
        return None
    for key_node, value_node in node.value:
        if _scalar(key_node) == key:
            return value_node
    return None


def _reference(uses_node: yaml.Node | None, owner: yaml.MappingNode, path: str) -> ToolchainReference | None:
    uses = _scalar(uses_node)
    if uses is None or not uses.startswith(f"{ACTION}@"):
        return None
    return ToolchainReference(
        path=path,
        line=uses_node.start_mark.line + 1,
        ref=uses[len(ACTION) + 1 :],
        owner=owner,
    )


def _step_references(steps: yaml.Node | None, path: str) -> list[ToolchainReference]:
    if not isinstance(steps, yaml.SequenceNode):
        return []
    references: list[ToolchainReference] = []
    for step in steps.value:
        if not isinstance(step, yaml.MappingNode):
            continue
        reference = _reference(_entry(step, "uses"), step, path)
        if reference is not None:
            references.append(reference)
    return references


def workflow_references(document: yaml.Node | None, path: str) -> list[ToolchainReference]:
    """Find step uses in all jobs and defensively inspect job-level uses too.

    A job-level ``uses`` is a reusable-workflow call, not a legal action call;
    any local reusable workflow it names is another file under ``workflows`` and
    is inspected independently.  Still inspecting a syntactically action-like
    job ``uses`` makes an attempted bypass fail rather than disappear.
    """
    jobs = _entry(document, "jobs")
    if not isinstance(jobs, yaml.MappingNode):
        return []
    references: list[ToolchainReference] = []
    for _job_name, job in jobs.value:
        if not isinstance(job, yaml.MappingNode):
            continue
        reference = _reference(_entry(job, "uses"), job, path)
        if reference is not None:
            references.append(reference)
        references.extend(_step_references(_entry(job, "steps"), path))
    return references


def composite_action_references(document: yaml.Node | None, path: str) -> list[ToolchainReference]:
    """Find action uses in a local composite action's ``runs.steps``."""
    runs = _entry(document, "runs")
    if not isinstance(runs, yaml.MappingNode) or _scalar(_entry(runs, "using")) != "composite":
        return []
    return _step_references(_entry(runs, "steps"), path)


def declared_msrv(cargo_toml_text: str) -> str | None:
    match = re.search(r'^\s*rust-version\s*=\s*"([^"]+)"', cargo_toml_text, re.MULTILINE)
    return match.group(1) if match else None


def audit_pinned_reference(reference: ToolchainReference, expected: str) -> list[str]:
    if EXPRESSION.search(reference.ref):
        return [
            f"{reference.path}:{reference.line} resolves {ACTION} through expression "
            f"{reference.ref!r}; a run-time ref cannot be shown pinned, so write @{expected} literally"
        ]
    if reference.ref in FLOATING_REFS:
        return [
            f"{reference.path}:{reference.line} uses {ACTION}@{reference.ref}; a floating "
            f"channel can turn main red without a repository change, so pin @{expected}"
        ]
    if reference.ref != expected:
        return [
            f"{reference.path}:{reference.line} uses {ACTION}@{reference.ref}, but repository "
            f"development workflows must use @{expected}"
        ]
    return []


def audit_msrv_reference(reference: ToolchainReference, msrv: str | None) -> list[str]:
    errors: list[str] = []
    if not COMMIT_SHA.match(reference.ref):
        errors.append(
            f"{reference.path}:{reference.line} uses {ACTION}@{reference.ref}, but the release "
            "workflow must pin the action by a 40-character commit SHA"
        )
    if msrv is None:
        errors.append(
            f"{reference.path}:{reference.line} is held to the MSRV contract, but rust/Cargo.toml "
            "declares no rust-version"
        )
        return errors
    # ``env`` is intentionally not consulted: only this direct child of ``with``
    # reaches an action input.  Aliases are already resolved by the composer.
    toolchain = _scalar(_entry(_entry(reference.owner, "with"), "toolchain"))
    if toolchain is None:
        errors.append(
            f"{reference.path}:{reference.line} must pass direct `with: toolchain:` input naming "
            f"declared MSRV {msrv}; env and nested with keys are not action inputs"
        )
    elif toolchain != msrv and not toolchain.startswith(f"{msrv}."):
        errors.append(
            f"{reference.path}:{reference.line} passes `toolchain: {toolchain}` but rust/Cargo.toml "
            f"declares rust-version {msrv!r}"
        )
    return errors


def _yaml_paths(directory: Path) -> list[Path]:
    iterator: Iterable[Path] = directory.iterdir() if directory.exists() else ()
    return sorted(
        path
        for path in iterator
        if path.is_file() and path.suffix in {".yaml", ".yml"}
    )


def audit_repository(
    repository: Path,
    msrv: str | None,
    *,
    expected: str = EXPECTED_REF,
    minimum_audited_references: int = MINIMUM_AUDITED_REFERENCES,
    msrv_paths: Mapping[str, str] = MSRV_PATHS,
) -> tuple[int, dict[str, int], list[str]]:
    """Audit workflows and every local composite-action descriptor.

    External reusable workflows are not repository-owned YAML and cannot be
    audited here.  Local reusable workflows are ordinary files under
    ``.github/workflows``; local composites are discovered by ``action.yml`` or
    ``action.yaml`` anywhere in this repository.
    """
    workflow_directory = repository / ".github" / "workflows"
    if not workflow_directory.is_dir():
        raise OSError(f"workflow directory {workflow_directory} does not exist")
    workflow_paths = _yaml_paths(workflow_directory)
    composite_paths = sorted(
        path
        for path in repository.rglob("action.y*ml")
        if ".git" not in path.parts and path.is_file()
    )

    errors: list[str] = []
    audited = 0
    msrv_seen = dict.fromkeys(msrv_paths, 0)
    for path in [*workflow_paths, *composite_paths]:
        relative = path.relative_to(repository).as_posix()
        try:
            document = load_yaml(path)
        except OSError as error:
            errors.append(f"{relative}: could not be read: {error.strerror or error}")
            continue
        except yaml.YAMLError as error:
            mark = getattr(error, "problem_mark", None)
            location = f":{mark.line + 1}" if mark is not None else ""
            errors.append(f"{relative}{location}: invalid YAML: {error}")
            continue

        references = (
            workflow_references(document, relative)
            if path in workflow_paths
            else composite_action_references(document, relative)
        )
        for reference in references:
            if relative in msrv_paths:
                msrv_seen[relative] += 1
                errors.extend(audit_msrv_reference(reference, msrv))
            else:
                audited += 1
                errors.extend(audit_pinned_reference(reference, expected))

    if audited < minimum_audited_references:
        errors.append(
            f"audited only {audited} development toolchain reference(s); expected at least "
            f"{minimum_audited_references}. The audit must not pass after losing its inputs"
        )
    for path, count in msrv_seen.items():
        if count == 0:
            errors.append(
                f"{path!r} is declared as held to the MSRV contract but contains no {ACTION} step"
            )
    return audited, msrv_seen, errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=REPO_ROOT)
    parser.add_argument("--cargo-toml", type=Path, default=CARGO_TOML)
    args = parser.parse_args()
    try:
        msrv = declared_msrv(args.cargo_toml.read_text(encoding="utf-8"))
        audited, msrv_seen, errors = audit_repository(args.repository, msrv)
    except OSError as error:
        print(f"rust toolchain pin: FAIL -- {error}")
        return 1

    if errors:
        for error in errors:
            print(f"rust toolchain pin: FAIL -- {error}")
        return 1
    held = ", ".join(f"{path} ({count})" for path, count in msrv_seen.items())
    print(
        f"rust toolchain pin: PASS -- {audited} reference(s) at @{EXPECTED_REF}; "
        f"MSRV contract held in {held}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
