# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Complete Python-to-Rust public CLI surface contract."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from tools.export_cli_contract import export_contract


ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust" / "target" / "debug" / "fslc"


def _walk(node: dict):
    yield node
    for child in node["commands"]:
        yield from _walk(child)


def test_cli_contract_export_is_deterministic_and_complete():
    first = export_contract()
    second = export_contract()
    assert first == second
    paths = {tuple(node["path"]) for node in _walk(first["root"])}
    assert ("verify",) in paths
    assert ("ai", "replay") in paths
    assert ("domain", "generate") in paths


def test_rust_embeds_the_current_python_cli_contract():
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
    assert json.loads(proc.stdout) == export_contract()


def test_rust_help_matches_argparse_at_every_command_path():
    contract = export_contract()
    for node in _walk(contract["root"]):
        proc = subprocess.run(
            [str(RUST), *node["path"], "--help"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert proc.returncode == 0, (node["path"], proc.stdout, proc.stderr)
        assert proc.stdout == node["help"], node["path"]
