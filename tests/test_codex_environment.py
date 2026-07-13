# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Contract tests for the checked-in Codex environment."""

import json
from pathlib import Path
import subprocess
import sys
import tomllib


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


def test_agents_instructions_fit_the_configured_budget() -> None:
    assert (ROOT / "AGENTS.md").stat().st_size <= 32_768
