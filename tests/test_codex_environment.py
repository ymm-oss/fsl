# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Contract tests for the checked-in Codex environment."""

import json
from pathlib import Path
import re
import subprocess
import sys
import warnings

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

import pytest


ROOT = Path(__file__).resolve().parents[1]
CODEX = ROOT / ".codex"
AGENT_SKILLS = ROOT / ".agents" / "skills"


def test_required_environment_files_exist() -> None:
    required = [
        CODEX / "config.toml",
        CODEX / "hooks.json",
        CODEX / "hooks" / "session_context.py",
        CODEX / "agents" / "evidence-explorer.toml",
        CODEX / "agents" / "independent-reviewer.toml",
        AGENT_SKILLS / "task-start" / "SKILL.md",
        AGENT_SKILLS / "checkpoint" / "SKILL.md",
        ROOT / "tasks" / "active.template.md",
    ]
    assert all(path.is_file() for path in required)


def test_project_config_bounds_context_and_delegation() -> None:
    config = tomllib.loads((CODEX / "config.toml").read_text(encoding="utf-8"))
    assert config["project_doc_max_bytes"] == 32_768
    assert config["approval_policy"] == "on-request"
    assert config["sandbox_mode"] == "workspace-write"
    assert config["web_search"] == "cached"
    assert config["features"] == {
        "hooks": True,
        "multi_agent": True,
        "memories": False,
        "goals": True,
    }
    assert config["agents"] == {"max_threads": 4, "max_depth": 1}


def test_custom_agents_are_read_only_and_well_formed() -> None:
    for name in ["evidence-explorer.toml", "independent-reviewer.toml"]:
        agent = tomllib.loads((CODEX / "agents" / name).read_text(encoding="utf-8"))
        assert agent["name"]
        assert agent["description"]
        assert agent["developer_instructions"]
        assert agent["sandbox_mode"] == "read-only"


def test_session_start_hook_is_root_relative_and_bounded() -> None:
    hooks = json.loads((CODEX / "hooks.json").read_text(encoding="utf-8"))
    group = hooks["hooks"]["SessionStart"][0]
    assert group["matcher"] == "startup|resume|clear|compact"
    command = group["hooks"][0]["command"]
    assert "git rev-parse --show-toplevel" in command
    assert ".codex/hooks/session_context.py" in command

    proc = subprocess.run(
        [sys.executable, str(CODEX / "hooks" / "session_context.py")],
        input=json.dumps({"cwd": str(ROOT), "source": "startup"}),
        capture_output=True,
        text=True,
        cwd=ROOT,
        check=False,
    )
    assert proc.returncode == 0, proc.stderr
    assert "Session source: startup" in proc.stdout
    assert "Branch:" in proc.stdout
    assert "## Working Tree" in proc.stdout
    assert "## Active Task State" in proc.stdout
    assert str(ROOT) not in proc.stdout
    assert len(proc.stdout.splitlines()) <= 280


def test_untrusted_checked_in_hook_entries_are_reported_locally() -> None:
    user_config = Path.home() / ".codex" / "config.toml"
    # Persisted hook trust is machine-local and absent in CI. Skip there rather
    # than failing: this diagnostic reports local untrusted entries only, and a
    # trust requirement would make every clean CI environment permanently red.
    if not user_config.is_file():
        pytest.skip("local Codex configuration is unavailable")

    hooks_path = (CODEX / "hooks.json").resolve()
    hooks = json.loads(hooks_path.read_text(encoding="utf-8"))["hooks"]
    user_state = tomllib.loads(user_config.read_text(encoding="utf-8"))
    trusted_entries = user_state.get("hooks", {}).get("state", {})
    untrusted_entries = []

    for event, groups in hooks.items():
        event_key = re.sub(r"(?<!^)(?=[A-Z])", "_", event).lower()
        for group_index, group in enumerate(groups):
            for hook_index, _hook in enumerate(group["hooks"]):
                state_key = f"{hooks_path}:{event_key}:{group_index}:{hook_index}"
                if state_key not in trusted_entries:
                    untrusted_entries.append(
                        f"{event_key}:{group_index}:{hook_index}"
                    )

    if untrusted_entries:
        warnings.warn(
            "untrusted checked-in Codex hook entries (not a test failure): "
            + ", ".join(untrusted_entries),
            stacklevel=1,
        )


def test_task_skills_require_explicit_invocation() -> None:
    for name in ["task-start", "checkpoint"]:
        metadata = (AGENT_SKILLS / name / "agents" / "openai.yaml").read_text(
            encoding="utf-8"
        )
        assert "allow_implicit_invocation: false" in metadata


def test_canonical_fsl_skills_are_discoverable_without_copies() -> None:
    names = [
        "fsl",
        "fsl-business",
        "fsl-delivery",
        "fsl-design",
        "fsl-design-review",
        "fsl-from-code",
        "fsl-requirements",
    ]
    for name in names:
        link = AGENT_SKILLS / name
        assert link.is_symlink()
        assert link.resolve() == (ROOT / "skills" / name).resolve()
        assert (link / "SKILL.md").is_file()


def test_active_task_is_worktree_local() -> None:
    proc = subprocess.run(
        ["git", "check-ignore", "-q", "tasks/active.md"],
        cwd=ROOT,
        check=False,
    )
    assert proc.returncode == 0


def test_semantic_findings_cannot_disappear_between_agents_and_checkpoint() -> None:
    template = (ROOT / "tasks" / "active.template.md").read_text(encoding="utf-8")
    task_start = (AGENT_SKILLS / "task-start" / "SKILL.md").read_text(
        encoding="utf-8"
    )
    checkpoint = (AGENT_SKILLS / "checkpoint" / "SKILL.md").read_text(
        encoding="utf-8"
    )
    explorer = (CODEX / "agents" / "evidence-explorer.toml").read_text(
        encoding="utf-8"
    )
    reviewer = (CODEX / "agents" / "independent-reviewer.toml").read_text(
        encoding="utf-8"
    )

    assert "## Discovered follow-ups" in template
    assert "authorization required" in template
    assert "behavior-bearing AST/enum variants" in task_start
    assert "Reconcile every discovered soundness defect" in checkpoint
    assert "hollow semantics" in explorer
    assert "calibrated negative control" in reviewer


def test_agents_instructions_fit_the_configured_budget() -> None:
    assert (ROOT / "AGENTS.md").stat().st_size <= 32_768
