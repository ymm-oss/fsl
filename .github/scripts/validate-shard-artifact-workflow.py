#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0

"""Parser-backed contract audit for stable sharded artifact cohorts."""

from __future__ import annotations

import argparse
from collections.abc import Mapping
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
PROVENANCE_SCHEMA = "fslc.shard-artifact-provenance.v1"
HELPER = "./tools/check-shard-artifact-cohort.sh"


class UniqueKeyLoader(yaml.SafeLoader):
    """Reject duplicate YAML keys so an audited value cannot be shadowed."""


def _construct_mapping(loader: UniqueKeyLoader, node: yaml.MappingNode, deep: bool = False) -> dict[Any, Any]:
    mapping: dict[Any, Any] = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise yaml.constructor.ConstructorError(
                "while constructing a mapping", node.start_mark, f"duplicate key {key!r}", key_node.start_mark
            )
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _construct_mapping
)


def load_workflow(path: Path) -> Mapping[str, Any]:
    document = yaml.load(path.read_text(encoding="utf-8"), Loader=UniqueKeyLoader)
    if not isinstance(document, Mapping):
        raise ValueError("workflow document must be a mapping")
    return document


def _mapping(value: Any) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _steps(job: Mapping[str, Any]) -> list[Mapping[str, Any]]:
    value = job.get("steps")
    if not isinstance(value, list):
        return []
    return [step for step in value if isinstance(step, Mapping)]


def _named_step(job: Mapping[str, Any], name: str) -> Mapping[str, Any] | None:
    matches = [step for step in _steps(job) if step.get("name") == name]
    return matches[0] if len(matches) == 1 else None


def _require_scalar(errors: list[str], owner: str, actual: Any, expected: str) -> None:
    if actual != expected:
        errors.append(f"{owner}: expected {expected!r}, actual {actual!r}")


def audit_workflow(document: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    jobs = _mapping(document.get("jobs"))
    lanes = (
        {
            "producer": "rust-tests",
            "aggregator": "rust-workspace",
            "upload": "Preserve test-shard inventory",
            "provenance": "Record test-shard provenance",
            "download": "Download rust test-shard inventories",
            "verify": "Verify shard completeness",
            "artifact": "rust-test-shard-${{ matrix.shard }}-${{ github.run_id }}",
            "path": "rust/target/test-shards/*",
            "pattern": "rust-test-shard-*-${{ github.run_id }}",
            "mode": "rust",
            "lane": "rust-tests",
            "guard": "Require complete rust-checks and rust-tests evidence",
            "guard_tokens": ("RUST_CHECKS", "RUST_TESTS"),
        },
        {
            "producer": "semantic-mutation-operators",
            "aggregator": "semantic-mutation",
            "upload": "Preserve operator shard evidence",
            "provenance": "Record operator-shard provenance",
            "download": "Download operator shard manifests",
            "verify": "Verify operator shard completeness",
            "artifact": "semantic-mutation-operators-${{ matrix.shard }}-${{ github.run_id }}",
            "path": "rust/target/fault-operators/logs/**",
            "pattern": "semantic-mutation-operators-*-${{ github.run_id }}",
            "mode": "semantic",
            "lane": "semantic-mutation-operators",
            "guard": "Require complete operator-shard and mutants-lane evidence",
            "guard_tokens": ("SEMANTIC_MUTATION_OPERATORS", "SEMANTIC_MUTATION_MUTANTS"),
        },
    )

    for lane in lanes:
        producer = _mapping(jobs.get(lane["producer"]))
        aggregator = _mapping(jobs.get(lane["aggregator"]))
        if not producer:
            errors.append(f"jobs.{lane['producer']}: required producer job is absent")
            continue
        if not aggregator:
            errors.append(f"jobs.{lane['aggregator']}: required aggregator job is absent")
            continue

        upload = _named_step(producer, lane["upload"])
        provenance = _named_step(producer, lane["provenance"])
        download = _named_step(aggregator, lane["download"])
        verify = _named_step(aggregator, lane["verify"])
        guard = _named_step(aggregator, lane["guard"])
        for label, step in (("upload", upload), ("provenance", provenance), ("download", download), ("verify", verify), ("dependency guard", guard)):
            if step is None:
                errors.append(f"{lane['producer']}->{lane['aggregator']}: expected exactly one {label} step")

        if upload is not None:
            with_inputs = _mapping(upload.get("with"))
            _require_scalar(errors, f"{lane['producer']} upload name", with_inputs.get("name"), lane["artifact"])
            _require_scalar(errors, f"{lane['producer']} upload path", with_inputs.get("path"), lane["path"])
            if with_inputs.get("overwrite") is not True:
                errors.append(f"{lane['producer']} upload overwrite: expected True, actual {with_inputs.get('overwrite')!r}")
            _require_scalar(errors, f"{lane['producer']} upload if", upload.get("if"), "always() && steps.scope.outputs.run == 'true'")

        if provenance is not None:
            run = provenance.get("run")
            if not isinstance(run, str):
                errors.append(f"{lane['producer']} provenance run: expected script scalar, actual {run!r}")
            else:
                for token in (PROVENANCE_SCHEMA, lane["lane"], "full_sha256", "shard_sha256", "git rev-parse HEAD"):
                    if token not in run:
                        errors.append(f"{lane['producer']} provenance run: expected token {token!r}, actual script omitted it")

        if download is not None:
            _require_scalar(errors, f"{lane['aggregator']} download pattern", _mapping(download.get("with")).get("pattern"), lane["pattern"])

        if verify is not None:
            expected_run = (
                f'{HELPER} {lane["mode"]} '
                f'{"rust-test-shards" if lane["mode"] == "rust" else "semantic-mutation-operators"} '
                '"${{ github.run_id }}" "${{ github.run_attempt }}" "$(git rev-parse HEAD)" 3'
            )
            _require_scalar(errors, f"{lane['aggregator']} verifier", verify.get("run"), expected_run)

        _require_scalar(errors, f"jobs.{lane['aggregator']}.if", aggregator.get("if"), "always()")
        if guard is not None:
            run = guard.get("run")
            env = _mapping(guard.get("env"))
            for token in lane["guard_tokens"]:
                if token not in env:
                    errors.append(f"{lane['aggregator']} dependency guard env: expected {token!r}, actual keys {sorted(env)}")
                if not isinstance(run, str) or token not in run:
                    errors.append(f"{lane['aggregator']} dependency guard run: expected token {token!r}, actual {run!r}")

    # These unsharded artifacts are intentionally attempt-scoped and guard the
    # audit against an indiscriminate global replacement.
    mutants = _named_step(_mapping(jobs.get("semantic-mutation-mutants")), "Preserve mutation evidence")
    if mutants is None:
        errors.append("semantic-mutation-mutants: expected unsharded upload step")
    else:
        _require_scalar(
            errors,
            "semantic-mutation-mutants upload name",
            _mapping(mutants.get("with")).get("name"),
            "semantic-mutation-mutants-${{ github.run_id }}-${{ github.run_attempt }}",
        )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("workflow", nargs="?", type=Path, default=WORKFLOW)
    args = parser.parse_args()
    try:
        errors = audit_workflow(load_workflow(args.workflow))
    except (OSError, ValueError, yaml.YAMLError) as error:
        print(f"shard artifact workflow audit: ERROR: {error}")
        return 2
    if errors:
        for error in errors:
            print(f"shard artifact workflow audit: FAIL: {error}")
        return 1
    print("shard artifact workflow audit: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
