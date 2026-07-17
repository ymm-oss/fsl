# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Thin Git adapter for bounded semantic specification diff."""
from __future__ import annotations

import io
import subprocess
import tarfile
import tempfile
from pathlib import Path

from .model import FslError
from .semantic_diff import semantic_diff


MATERIALIZATION = "git_archive_full_tree"


def _git(repo: Path, *args: str) -> str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(repo), *args],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise FslError("git executable not found", kind="io") from exc
    except subprocess.CalledProcessError as exc:
        detail = exc.stderr.decode("utf-8", errors="replace").strip()
        raise FslError(detail or "git command failed", kind="io") from exc
    return completed.stdout.decode("utf-8").strip()


def _repo_root(start: Path) -> Path:
    return Path(_git(start, "rev-parse", "--show-toplevel"))


def _commit(repo: Path, revision: str) -> str:
    return _git(repo, "rev-parse", "--verify", f"{revision}^{{commit}}")


def _split_range(value: str) -> tuple[str, str]:
    if value.count("..") != 1 or "..." in value:
        raise FslError("--git expects exactly BASE..HEAD", kind="semantics")
    base, head = value.split("..", 1)
    if not base or not head:
        raise FslError("--git expects exactly BASE..HEAD", kind="semantics")
    return base, head


def _materialize(repo: Path, revision: str, destination: Path) -> None:
    try:
        archive = subprocess.run(
            ["git", "-C", str(repo), "archive", "--format=tar", revision],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout
    except subprocess.CalledProcessError as exc:
        detail = exc.stderr.decode("utf-8", errors="replace").strip()
        raise FslError(detail or f"cannot materialize {revision}", kind="io") from exc
    destination = destination.resolve()
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        for member in bundle.getmembers():
            target = (destination / member.name).resolve()
            if target != destination and destination not in target.parents:
                raise FslError("git archive contains an unsafe path", kind="io")
        bundle.extractall(destination)


def _relative_spec(repo: Path, value: str) -> str:
    path = Path(value)
    if path.is_absolute():
        try:
            path = path.resolve().relative_to(repo.resolve())
        except ValueError as exc:
            raise FslError("diff spec must be inside the Git repository", kind="io") from exc
    normalized = Path(*[part for part in path.parts if part not in ("", ".")])
    if ".." in normalized.parts:
        raise FslError("diff spec must be inside the Git repository", kind="io")
    return normalized.as_posix()


def _changed_specs(repo: Path, base: str, head: str) -> list[str]:
    output = _git(
        repo,
        "diff",
        "--name-only",
        "--diff-filter=ACMR",
        base,
        head,
        "--",
        "*.fsl",
    )
    return sorted(line for line in output.splitlines() if line.endswith(".fsl"))


def _vcs_metadata(range_value: str, base_input: str, head_input: str,
                  base: str, head: str) -> dict:
    return {
        "kind": "git",
        "range": range_value,
        "base": {"revision": base_input, "commit": base},
        "head": {"revision": head_input, "commit": head},
        "materialization": MATERIALIZATION,
    }


def semantic_diff_git(range_value: str, spec_path: str | None = None, depth: int = 8,
                      mapping_path: str | None = None, forbid=None, cwd=None) -> dict:
    """Materialize both revisions and delegate comparison to ``semantic_diff``."""
    repo = _repo_root(Path(cwd or Path.cwd()))
    base_input, head_input = _split_range(range_value)
    base = _commit(repo, base_input)
    head = _commit(repo, head_input)
    specs = (
        [_relative_spec(repo, spec_path)]
        if spec_path is not None
        else _changed_specs(repo, base, head)
    )
    if not specs:
        raise FslError("Git range contains no changed .fsl files", kind="io")

    vcs = _vcs_metadata(range_value, base_input, head_input, base, head)
    with tempfile.TemporaryDirectory(prefix="fslc-diff-base-") as base_tmp, \
            tempfile.TemporaryDirectory(prefix="fslc-diff-head-") as head_tmp:
        base_tree = Path(base_tmp)
        head_tree = Path(head_tmp)
        _materialize(repo, base, base_tree)
        _materialize(repo, head, head_tree)
        comparisons = []
        for spec in specs:
            old_path = base_tree / spec
            new_path = head_tree / spec
            if not old_path.is_file() or not new_path.is_file():
                raise FslError(
                    f"'{spec}' must exist in both revisions for semantic diff",
                    kind="io",
                )
            mapping = mapping_path
            if mapping_path and not Path(mapping_path).is_absolute():
                candidate = head_tree / _relative_spec(repo, mapping_path)
                if candidate.is_file():
                    mapping = str(candidate)
            result = semantic_diff(
                str(old_path),
                str(new_path),
                depth,
                mapping,
                forbid,
            )
            result["old"]["file"] = f"{base_input}:{spec}"
            result["new"]["file"] = f"{head_input}:{spec}"
            result["vcs"] = vcs
            comparisons.append(result)

    if spec_path is not None:
        return comparisons[0]
    violations = sorted({
        kind
        for result in comparisons
        for kind in result.get("gate", {}).get("violations", [])
    })
    return {
        "result": "semantic_diff_batch",
        "vcs": vcs,
        "specs": specs,
        "comparisons": comparisons,
        "gate": {
            "violations": violations,
            "passed": not violations,
        },
    }
