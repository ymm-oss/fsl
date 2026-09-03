# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Reachability metatest for the CI validator inventory (issue #962)."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "check_ci_validator_inventory.py"


def run_tool(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    root = cwd or ROOT
    return subprocess.run(
        [sys.executable, str(TOOL), "--root", str(root), *args],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )


def write_fixture_inventory(fixture: Path) -> None:
    inventory = {
        "schema_version": 1,
        "required_gate_entrypoints": [
            "tools/check-merge-readiness.sh",
            ".github/workflows/site-reference-freshness.yml",
        ],
        "validators": [
            {
                "path": "tests/test_required.py",
                "tier": "required",
                "entrypoints": ["tools/check-merge-readiness.sh"],
            },
            {
                "path": "tests/test_exempt.py",
                "tier": "exempt",
                "exempt_reason": "frozen-python-compatibility",
            },
        ],
    }
    (fixture / ".github" / "ci-validator-inventory.json").write_text(
        json.dumps(inventory, indent=2) + "\n",
        encoding="utf-8",
    )


def bootstrap_fixture(tmp_path: Path) -> Path:
    fixture = tmp_path / "fixture"
    fixture.mkdir()
    (fixture / "tests").mkdir()
    (fixture / "tools").mkdir()
    (fixture / ".github" / "workflows").mkdir(parents=True)
    (fixture / "tools" / "check-merge-readiness.sh").write_text(
        """#!/usr/bin/env bash
set -euo pipefail
check_automation() {
  python3 -m pytest tests/test_required.py -v
}
case "${1:-all}" in
  automation) check_automation ;;
  *) echo "usage: $0 automation" >&2; exit 2 ;;
esac
""",
        encoding="utf-8",
    )
    (fixture / ".github" / "workflows" / "site-reference-freshness.yml").write_text(
        "name: site reference freshness\non: pull_request\njobs: {}\n",
        encoding="utf-8",
    )
    (fixture / "tests" / "test_required.py").write_text("# required probe\n", encoding="utf-8")
    (fixture / "tests" / "test_exempt.py").write_text("# exempt probe\n", encoding="utf-8")
    write_fixture_inventory(fixture)
    subprocess.run(["git", "init"], cwd=fixture, check=True, capture_output=True, text=True)
    subprocess.run(["git", "add", "."], cwd=fixture, check=True, capture_output=True, text=True)
    return fixture


def test_checker_selftest_passes():
    result = run_tool("selftest")
    assert result.returncode == 0, result.stderr or result.stdout


def test_live_inventory_matches_repository():
    result = run_tool("check")
    assert result.returncode == 0, result.stderr or result.stdout


def test_inventory_rejects_untracked_validator(tmp_path: Path):
    fixture = bootstrap_fixture(tmp_path)
    (fixture / "tests" / "test_probe_untracked.py").write_text("# probe\n", encoding="utf-8")
    subprocess.run(
        ["git", "add", "tests/test_probe_untracked.py"],
        cwd=fixture,
        check=True,
        capture_output=True,
        text=True,
    )
    result = run_tool("check", cwd=fixture)
    assert result.returncode != 0
    assert "untracked validator module" in result.stderr


def test_inventory_calibration_wiring_detector(tmp_path: Path):
    fixture = bootstrap_fixture(tmp_path)
    probe = fixture / "tests" / "test_probe_wiring.py"
    probe.write_text("# probe\n", encoding="utf-8")
    subprocess.run(
        ["git", "add", "tests/test_probe_wiring.py"],
        cwd=fixture,
        check=True,
        capture_output=True,
        text=True,
    )

    unwired = run_tool("check", cwd=fixture)
    assert unwired.returncode != 0
    assert "untracked validator module" in unwired.stderr
    assert "test_probe_wiring.py" in unwired.stderr

    generate_blocked = run_tool("generate", cwd=fixture)
    assert generate_blocked.returncode != 0
    assert "require explicit classification" in (generate_blocked.stderr or generate_blocked.stdout)

    merge_script = fixture / "tools" / "check-merge-readiness.sh"
    merge_script.write_text(
        merge_script.read_text(encoding="utf-8").replace(
            "tests/test_required.py -v",
            "tests/test_required.py -v\n  python3 -m pytest tests/test_probe_wiring.py -v",
        ),
        encoding="utf-8",
    )
    generated = run_tool("generate", cwd=fixture)
    assert generated.returncode == 0, generated.stderr or generated.stdout

    inventory = json.loads(
        (fixture / ".github" / "ci-validator-inventory.json").read_text(encoding="utf-8")
    )
    probe_entry = next(
        entry for entry in inventory["validators"] if entry["path"] == "tests/test_probe_wiring.py"
    )
    assert probe_entry["tier"] == "required"
    assert "tools/check-merge-readiness.sh" in probe_entry["entrypoints"]

    wired = run_tool("check", cwd=fixture)
    assert wired.returncode == 0, wired.stderr or wired.stdout
