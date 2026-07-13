# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Frozen Python CLI compatibility plus an explicit Rust-only surface."""

from __future__ import annotations

import copy
import hashlib
import json
import subprocess
from pathlib import Path

import pytest

from tools.export_cli_contract import export_contract


ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust" / "target" / "debug" / "fslc"
RUST_ONLY_SURFACE = ROOT / "tests" / "rust_only_surface.json"


def _walk(node: dict):
    yield node
    for child in node["commands"]:
        yield from _walk(child)


def _nodes(contract: dict) -> dict[tuple[str, ...], dict]:
    walked = list(_walk(contract["root"]))
    nodes = {tuple(node["path"]): node for node in walked}
    assert len(nodes) == len(walked), "CLI contract paths must be unique"
    return nodes


def _help_sha256(node: dict) -> str:
    return hashlib.sha256(node["help"].encode()).hexdigest()


def _normalize_python_contract(contract: dict) -> dict:
    """Remove argparse-version artifacts that do not change CLI semantics."""
    normalized = copy.deepcopy(contract)
    for node in _walk(normalized["root"]):
        for action in node["actions"]:
            if action.get("positional") and action.get("nargs") == "*":
                action["required"] = False
    return normalized


def _rust_only_node(path: tuple[str, ...], nodes: dict[tuple[str, ...], dict]) -> dict:
    node = nodes[path]
    parent = nodes[path[:-1]]
    return {
        "path": list(path),
        "parent_index": next(
            index
            for index, child in enumerate(parent["commands"])
            if tuple(child["path"]) == path
        ),
        "prog": node["prog"],
        "help_sha256": _help_sha256(node),
        "actions": node["actions"],
        "commands": [child["path"][-1] for child in node["commands"]],
    }


def _surface_delta(python_contract: dict, rust_contract: dict) -> dict:
    """Return the complete structural surface added or changed by Rust."""
    python_contract = _normalize_python_contract(python_contract)
    assert set(rust_contract) == set(python_contract) == {"schema", "root"}
    assert rust_contract["schema"] == python_contract["schema"]
    python_nodes = _nodes(python_contract)
    rust_nodes = _nodes(rust_contract)
    expected_node_fields = {"path", "prog", "help", "actions", "commands"}
    assert all(set(node) == expected_node_fields for node in rust_nodes.values())

    missing_nodes = sorted(set(python_nodes) - set(rust_nodes))
    assert not missing_nodes, f"Rust is missing Python CLI nodes: {missing_nodes}"

    rust_only_nodes = [
        _rust_only_node(path, rust_nodes)
        for path in sorted(set(rust_nodes) - set(python_nodes))
    ]
    rust_only_actions = []
    action_overrides = []

    for path in sorted(python_nodes):
        python_node = python_nodes[path]
        rust_node = rust_nodes[path]
        assert rust_node["path"] == python_node["path"]
        assert rust_node["prog"] == python_node["prog"]

        python_children = [tuple(child["path"]) for child in python_node["commands"]]
        rust_children = [tuple(child["path"]) for child in rust_node["commands"]]
        assert [child for child in rust_children if child in python_nodes] == python_children

        python_actions = {action["dest"]: action for action in python_node["actions"]}
        rust_actions = {action["dest"]: action for action in rust_node["actions"]}
        missing_actions = [dest for dest in python_actions if dest not in rust_actions]
        assert not missing_actions, (
            f"Rust node {path} is missing Python actions: {missing_actions}"
        )
        assert [
            action["dest"]
            for action in rust_node["actions"]
            if action["dest"] in python_actions
        ] == list(python_actions)

        for index, action in enumerate(rust_node["actions"]):
            dest = action["dest"]
            if dest not in python_actions:
                rust_only_actions.append(
                    {"path": list(path), "index": index, "action": action}
                )
            elif action != python_actions[dest]:
                action_overrides.append(
                    {
                        "path": list(path),
                        "index": index,
                        "python": python_actions[dest],
                        "rust": action,
                    }
                )

    return {
        "rust_only_nodes": rust_only_nodes,
        "rust_only_actions": rust_only_actions,
        "action_overrides": action_overrides,
    }


def _assert_rust_surface_is_allowed(python_contract: dict, rust_contract: dict) -> None:
    policy = json.loads(RUST_ONLY_SURFACE.read_text(encoding="utf-8"))
    assert set(policy) == {
        "rust_only_nodes",
        "rust_only_actions",
        "action_overrides",
        "help_overrides",
    }
    assert _surface_delta(python_contract, rust_contract) == {
        key: policy[key]
        for key in ("rust_only_nodes", "rust_only_actions", "action_overrides")
    }

    python_nodes = _nodes(python_contract)
    rust_nodes = _nodes(rust_contract)
    actual_help_overrides = [
        {
            "path": list(path),
            "python_sha256": _help_sha256(python_node),
            "rust_sha256": _help_sha256(rust_nodes[path]),
        }
        for path, python_node in python_nodes.items()
        if rust_nodes[path]["help"] != python_node["help"]
    ]
    assert sorted(actual_help_overrides, key=lambda item: item["path"]) == policy[
        "help_overrides"
    ]


@pytest.fixture(scope="module")
def rust_contract() -> dict:
    subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "fslc-rust"],
        cwd=ROOT / "rust",
        check=True,
    )
    proc = subprocess.run(
        [str(RUST), "--cli-contract"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert proc.returncode == 0, proc.stderr
    return json.loads(proc.stdout)


def test_cli_contract_export_is_deterministic_and_complete():
    first = export_contract()
    second = export_contract()
    assert first == second
    paths = {tuple(node["path"]) for node in _walk(first["root"])}
    assert ("verify",) in paths
    assert ("ai", "replay") in paths
    assert ("domain", "generate") in paths


def test_rust_surface_extends_frozen_python_contract_only_by_policy(rust_contract):
    _assert_rust_surface_is_allowed(export_contract(), rust_contract)


def test_unlisted_choice_mismatch_is_rejected(rust_contract):
    mutant = copy.deepcopy(rust_contract)
    verify = _nodes(mutant)[("verify",)]
    engine = next(action for action in verify["actions"] if action["dest"] == "engine")
    engine["choices"][-1] = "explict"

    with pytest.raises(AssertionError):
        _assert_rust_surface_is_allowed(export_contract(), mutant)


def test_unlisted_help_drift_is_rejected(rust_contract):
    mutant = copy.deepcopy(rust_contract)
    _nodes(mutant)[()]["help"] = "arbitrary help drift\n"

    with pytest.raises(AssertionError):
        _assert_rust_surface_is_allowed(export_contract(), mutant)


def test_argparse_star_positional_required_artifact_is_normalized(rust_contract):
    for required in (False, True):
        python_contract = export_contract()
        rest = next(
            action
            for action in _nodes(python_contract)[("refine",)]["actions"]
            if action["dest"] == "rest"
        )
        rest["required"] = required
        _assert_rust_surface_is_allowed(python_contract, rust_contract)


def test_duplicate_rust_only_command_path_is_rejected(rust_contract):
    mutant = copy.deepcopy(rust_contract)
    approval = copy.deepcopy(_nodes(mutant)[("approval",)])
    mutant["root"]["commands"].append(approval)

    with pytest.raises(AssertionError, match="paths must be unique"):
        _assert_rust_surface_is_allowed(export_contract(), mutant)


def test_rust_parser_rejects_unlisted_engine_choice(rust_contract):
    proc = subprocess.run(
        [str(RUST), "verify", "specs/cart_v1.fsl", "--engine", "explict"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert proc.returncode == 2
    assert "--engine must be bmc, induction, or explicit" in proc.stdout


def test_unknown_help_path_is_rejected(rust_contract):
    proc = subprocess.run(
        [str(RUST), "not-a-command", "--help"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert proc.returncode == 2


def test_help_after_leaf_command_arguments_uses_that_command(rust_contract):
    proc = subprocess.run(
        [str(RUST), "verify", "specs/cart_v1.fsl", "--help"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert proc.returncode == 0
    assert proc.stdout == _nodes(rust_contract)[("verify",)]["help"]


def test_rust_help_matches_embedded_contract_at_every_command_path(rust_contract):
    for node in _walk(rust_contract["root"]):
        proc = subprocess.run(
            [str(RUST), *node["path"], "--help"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert proc.returncode == 0, (node["path"], proc.stdout, proc.stderr)
        assert proc.stdout == node["help"], node["path"]
