# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Contract tests for the checked-in Claude Code environment."""

from fnmatch import fnmatchcase
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
from typing import Optional


ROOT = Path(__file__).resolve().parents[1]
CLAUDE = ROOT / ".claude"


def run_hook(
    name: str, payload: dict, *, env: Optional[dict] = None
) -> subprocess.CompletedProcess:
    hook_env = os.environ.copy()
    hook_env["CLAUDE_PROJECT_DIR"] = str(ROOT)
    if env:
        hook_env.update(env)
    return subprocess.run(
        [sys.executable, str(CLAUDE / "hooks" / name)],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        cwd=ROOT,
        env=hook_env,
        check=False,
    )


def test_required_environment_files_exist() -> None:
    required = [
        "rules/rust-verifier.md",
        "rules/python-reference.md",
        "rules/fsl-specs.md",
        "rules/wasm-docs.md",
        "agents/fsl-codebase-explorer.md",
        "agents/fsl-test-diagnostician.md",
        "skills/task-start/SKILL.md",
        "skills/checkpoint/SKILL.md",
        "work/active.template.md",
        "hooks/session_context.py",
    ]
    assert all((CLAUDE / path).is_file() for path in required)


def test_normal_entrypoint_triggers_rust_architecture_rule_for_rust_only() -> None:
    entrypoint = (ROOT / "CLAUDE.md").read_text(encoding="utf-8")
    assert entrypoint.startswith("@AGENTS.md\n")
    assert (
        "Inspect `git status --short` and the relevant implementation before editing."
        in entrypoint
    )

    rule = (CLAUDE / "rules" / "rust-verifier.md").read_text(encoding="utf-8")
    front_matter = rule.split("---", 2)[1]
    patterns = [
        line.removeprefix('- "').removesuffix('"')
        for line in map(str.strip, front_matter.splitlines())
        if line.startswith('- "')
    ]

    def applies_to(path: str) -> bool:
        return any(fnmatchcase(path, pattern) for pattern in patterns)

    assert applies_to("rust/fsl-runtime/src/lib.rs")
    assert applies_to("rust/fslc/src/main.rs")
    assert applies_to("rust/Cargo.toml")
    assert applies_to("rust/fsl-runtime/Cargo.toml")
    assert not applies_to("src/fslc/runtime.py")
    assert not applies_to("pyproject.toml")
    assert not applies_to("docs/DESIGN-rust-components.md")


def test_rust_architecture_rule_carries_the_accepted_contract() -> None:
    rule = (CLAUDE / "rules" / "rust-verifier.md").read_text(encoding="utf-8")
    normalized_rule = " ".join(rule.split())
    required_contracts = [
        "DESIGN-rust-components.md",
        "DESIGN-rust-component-internals.md",
        "`fsl-syntax` owns source fidelity",
        "`fsl-core` owns checked models and Public Kernel",
        "`fsl-runtime` owns concrete Monitor/replay/BFS semantics",
        "`fsl-solver` owns the backend-neutral boundary",
        "`fsl-solver-z3` and `fsl-solver-z3js` are native and browser adapters",
        "`fsl-verifier` owns symbolic engines",
        "`fsl-tools` owns derived artifacts",
        "`fslc-rust`, `fsl-wasm`, and `fsl-lsp` own native delivery, Worker delivery, and "
        "editor projection",
        "`fsl-runtime` must not depend directly or transitively on solver or Z3 crates",
        "raw-output",
        "JSON envelopes",
        "exit codes",
        "Public Kernel",
        "replay contracts",
        "native/Worker parity",
        "not permission for an eager rewrite",
        "source-changing C2",
        "negative control",
        "testgen::relative_spec_path",
        "#423",
        "implicit filesystem dependency",
    ]
    assert all(contract in normalized_rule for contract in required_contracts)


def test_settings_use_project_root_and_protect_snapshot() -> None:
    settings = json.loads((CLAUDE / "settings.json").read_text(encoding="utf-8"))
    commands = [
        hook["command"]
        for groups in settings["hooks"].values()
        for group in groups
        for hook in group["hooks"]
    ]
    assert commands
    assert all("${CLAUDE_PROJECT_DIR}" in command for command in commands)
    assert "Edit(/tests/snapshots/corpus_snapshot.json)" in settings["permissions"]["deny"]


def test_session_start_returns_bounded_context() -> None:
    proc = run_hook("session_context.py", {"cwd": str(ROOT)})
    assert proc.returncode == 0, proc.stderr
    output = json.loads(proc.stdout)
    specific = output["hookSpecificOutput"]
    assert specific["hookEventName"] == "SessionStart"
    context = specific["additionalContext"]
    assert "Current branch:" in context
    assert "Working tree:" in context
    assert len(context.splitlines()) <= 45


def test_snapshot_guard_blocks_direct_write() -> None:
    proc = run_hook(
        "snapshot_guard.py",
        {"tool_input": {"file_path": "tests/snapshots/corpus_snapshot.json"}},
    )
    assert proc.returncode == 2
    assert "compatibility-contract" in proc.stderr


def test_spdx_guard_checks_new_python_source(tmp_path: Path) -> None:
    source = tmp_path / "new_module.py"
    source.write_text("print('missing header')\n", encoding="utf-8")
    proc = run_hook("spdx_guard.py", {"tool_input": {"file_path": str(source)}})
    assert proc.returncode == 2

    source.write_text(
        "# SPDX-License-Identifier: Apache-2.0\n"
        "# Copyright 2026 Example\n",
        encoding="utf-8",
    )
    proc = run_hook("spdx_guard.py", {"tool_input": {"file_path": str(source)}})
    assert proc.returncode == 0, proc.stderr


def test_native_check_hook_ignores_non_fsl_edits() -> None:
    proc = run_hook("fslc_check.py", {"tool_input": {"file_path": "README.md"}})
    assert proc.returncode == 0
    assert not proc.stderr


def test_native_check_command_targets_rust_cli() -> None:
    namespace = runpy.run_path(str(CLAUDE / "hooks" / "fslc_check.py"))
    command = namespace["native_check_command"](ROOT, "specs/cart_v1.fsl")
    assert command[:2] == ["cargo", "run"]
    assert "fslc-rust" in command
    assert command[command.index("--bin") + 1] == "fslc"
    assert str(ROOT / "rust" / "Cargo.toml") in command
    assert ".venv" not in " ".join(command)


def test_changelog_reminder_covers_rust_and_python_product_paths() -> None:
    namespace = runpy.run_path(str(CLAUDE / "hooks" / "changelog_reminder.py"))
    needs_reminder = namespace["needs_reminder"]
    assert needs_reminder(["rust/fsl-core/src/lib.rs"])
    assert needs_reminder(["src/fslc/model.py"])
    assert not needs_reminder(["rust/fsl-core/src/lib.rs", "CHANGELOG.md"])
    assert not needs_reminder(["docs/README.md"])


def test_active_task_is_worktree_local() -> None:
    proc = subprocess.run(
        ["git", "check-ignore", "-q", ".claude/work/active.md"],
        cwd=ROOT,
        check=False,
    )
    assert proc.returncode == 0


def test_claude_assets_do_not_route_product_work_to_python() -> None:
    paths = [
        CLAUDE / "agents" / "fsl-soundness-reviewer.md",
        CLAUDE / "agents" / "fsl-vacuity-reviewer.md",
        CLAUDE / "skills" / "add-language-feature" / "SKILL.md",
        CLAUDE / "skills" / "new-spec" / "SKILL.md",
    ]
    content = "\n".join(path.read_text(encoding="utf-8") for path in paths)
    assert ".venv/bin/python -m fslc" not in content
    assert "src/fslc/bmc.py" not in content
    assert "fslc-rust" in content
