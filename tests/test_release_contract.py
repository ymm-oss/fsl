# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Contract tests for the native GitHub Release procedure."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]


def test_release_matrix_has_exactly_the_supported_native_targets() -> None:
    workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(
        encoding="utf-8"
    )
    assert re.findall(r"^\s+target: (\S+)$", workflow, re.MULTILINE) == [
        "macos-arm64",
        "linux-x64",
        "linux-arm64",
        "windows-x64",
    ]
    assert "macos-15-intel" not in workflow
    assert "fslc-macos-x64" not in workflow
    attach_guard = (
        "if: github.event_name == 'push' && "
        "startsWith(github.ref, 'refs/tags/v')"
    )
    assert workflow.count(attach_guard) == 3
    assert '"fslc ${GITHUB_REF_NAME#v}"' in workflow
    assert '$env:GITHUB_REF_NAME.Substring(1)' in workflow
    assert "binary version does not match tag" in workflow


def test_release_skill_defers_to_the_documented_distribution_contract() -> None:
    runbook = (ROOT / "docs" / "RELEASE.md").read_text(encoding="utf-8")
    internal_skill = ROOT / ".claude" / "skills" / "release"
    codex_link = ROOT / ".codex" / "skills" / "release"
    skill = (internal_skill / "SKILL.md").read_text(encoding="utf-8")
    normalized_runbook = " ".join(runbook.split())
    normalized_skill = " ".join(skill.split())

    assert "Read `docs/RELEASE.md` completely" in skill
    assert codex_link.is_symlink()
    assert codex_link.resolve() == internal_skill.resolve()
    assert not (ROOT / "skills" / "release").exists()
    for contract in [
        "short-lived branch -> main -> production -> vX.Y.Z",
        "Never tag `main`",
    ]:
        assert contract in normalized_runbook
        assert contract in normalized_skill
    assert "git tag -a vX.Y.Z PRODUCTION_SHA" in normalized_runbook
    assert "tag `vX.Y.Z` at the gated `production` HEAD" in normalized_skill
    for expected in [
        "macOS arm64",
        "Linux x64",
        "Linux arm64",
        "Windows x64",
        "workflow_dispatch",
        "explicit confirmation",
        "PyPI",
    ]:
        assert expected in normalized_runbook
        assert expected in normalized_skill
