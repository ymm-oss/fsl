# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Reachability metatest for the CI validator inventory (issue #962; extended to
tools/check_rust_*.py by issue #761 stage 2)."""

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
            {
                "path": "tools/check_rust_required.py",
                "tier": "required",
                "entrypoints": ["tools/check-merge-readiness.sh"],
            },
            {
                "path": "tools/check_rust_exempt.py",
                "tier": "exempt",
                "exempt_reason": "manual-developer-run",
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
  python3 tools/check_rust_required.py
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
    (fixture / "tools" / "check_rust_required.py").write_text("# required harness probe\n", encoding="utf-8")
    (fixture / "tools" / "check_rust_exempt.py").write_text("# exempt harness probe\n", encoding="utf-8")
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


def test_inventory_rejects_untracked_rust_harness(tmp_path: Path):
    # issue #761 stage 2: the tools/check_rust_*.py sibling of
    # test_inventory_rejects_untracked_validator above, exercised through the
    # real git-tracked-discovery path (git_tracked_rust_harness_modules), not
    # a hand-built discovered= list.
    fixture = bootstrap_fixture(tmp_path)
    (fixture / "tools" / "check_rust_probe_untracked.py").write_text(
        "# probe\n", encoding="utf-8"
    )
    subprocess.run(
        ["git", "add", "tools/check_rust_probe_untracked.py"],
        cwd=fixture,
        check=True,
        capture_output=True,
        text=True,
    )
    result = run_tool("check", cwd=fixture)
    assert result.returncode != 0
    assert "untracked validator module" in result.stderr
    assert "tools/check_rust_probe_untracked.py" in result.stderr


def test_inventory_calibration_wiring_detector_for_rust_harness(tmp_path: Path):
    fixture = bootstrap_fixture(tmp_path)
    probe = fixture / "tools" / "check_rust_probe_wiring.py"
    probe.write_text("# probe\n", encoding="utf-8")
    subprocess.run(
        ["git", "add", "tools/check_rust_probe_wiring.py"],
        cwd=fixture,
        check=True,
        capture_output=True,
        text=True,
    )

    # No-fault/fault-side calibration on the same execution path: an
    # unclassified new harness fails `check` (fault side) with the identical
    # finding text the tests/test_*.py family produces above, then `generate`
    # accepts an explicit --exempt classification (no-fault side) instead of
    # requiring wiring, proving the two paths (wired-or-exempt) both work for
    # this file family, not only the wired one.
    unwired = run_tool("check", cwd=fixture)
    assert unwired.returncode != 0
    assert "untracked validator module" in unwired.stderr
    assert "check_rust_probe_wiring.py" in unwired.stderr

    generate_blocked = run_tool("generate", cwd=fixture)
    assert generate_blocked.returncode != 0
    assert "require explicit classification" in (generate_blocked.stderr or generate_blocked.stdout)

    generate_bad_path = run_tool(
        "generate", "--exempt", "tools/check_rust_probe_wiring.py",  # missing ":reason"
        cwd=fixture,
    )
    assert generate_bad_path.returncode != 0

    generated_exempt = run_tool(
        "generate",
        "--exempt",
        "tools/check_rust_probe_wiring.py:pending-native-migration",
        cwd=fixture,
    )
    assert generated_exempt.returncode == 0, generated_exempt.stderr or generated_exempt.stdout
    inventory = json.loads(
        (fixture / ".github" / "ci-validator-inventory.json").read_text(encoding="utf-8")
    )
    probe_entry = next(
        entry
        for entry in inventory["validators"]
        if entry["path"] == "tools/check_rust_probe_wiring.py"
    )
    assert probe_entry["tier"] == "exempt"
    assert probe_entry["exempt_reason"] == "pending-native-migration"
    exempt_ok = run_tool("check", cwd=fixture)
    assert exempt_ok.returncode == 0, exempt_ok.stderr or exempt_ok.stdout

    # Now show the wired path also works, from the same starting inventory
    # (re-add the probe as untracked-again by resetting its classification is
    # unnecessary: wiring it makes `required` the correct tier regardless of
    # the exempt row generate just wrote, matching build_entry's own
    # wiring-takes-priority-over-prior rule).
    merge_script = fixture / "tools" / "check-merge-readiness.sh"
    merge_script.write_text(
        merge_script.read_text(encoding="utf-8").replace(
            "python3 tools/check_rust_required.py",
            "python3 tools/check_rust_required.py\n  python3 tools/check_rust_probe_wiring.py",
        ),
        encoding="utf-8",
    )
    generated_required = run_tool("generate", cwd=fixture)
    assert generated_required.returncode == 0, generated_required.stderr or generated_required.stdout
    inventory = json.loads(
        (fixture / ".github" / "ci-validator-inventory.json").read_text(encoding="utf-8")
    )
    probe_entry = next(
        entry
        for entry in inventory["validators"]
        if entry["path"] == "tools/check_rust_probe_wiring.py"
    )
    assert probe_entry["tier"] == "required"
    assert "tools/check-merge-readiness.sh" in probe_entry["entrypoints"]
    wired = run_tool("check", cwd=fixture)
    assert wired.returncode == 0, wired.stderr or wired.stdout


def test_inventory_rejects_declared_reason_outside_the_closed_set(tmp_path: Path):
    # Condition-4 boundary: --exempt cannot mint a brand-new reason string on
    # the spot. Only EXEMPT_REASONS' declared values are accepted, so
    # widening what a harness may claim requires editing this tool's source
    # (reviewable), not a one-line data change.
    fixture = bootstrap_fixture(tmp_path)
    probe = fixture / "tools" / "check_rust_probe_reason.py"
    probe.write_text("# probe\n", encoding="utf-8")
    subprocess.run(
        ["git", "add", "tools/check_rust_probe_reason.py"],
        cwd=fixture,
        check=True,
        capture_output=True,
        text=True,
    )
    result = run_tool(
        "generate",
        "--exempt",
        "tools/check_rust_probe_reason.py:invented-on-the-spot",
        cwd=fixture,
    )
    assert result.returncode != 0
    assert "unknown exempt reason" in (result.stderr or result.stdout)
