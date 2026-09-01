# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Contract tests for the shared Cargo lock and migrated hook detectors."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time
from unittest.mock import patch

import pytest


ROOT = Path(__file__).resolve().parents[1]
CODEX_HOOKS = ROOT / ".codex" / "hooks"
# macOS system bash 3.2 is typically resolved from this PATH prefix.
BASH32_CALIB_PATH = "/bin:/usr/bin:/sbin:/usr/sbin"


def run_hook(name: str, payload: dict) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CODEX_HOOKS / name)],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        cwd=ROOT,
        check=False,
    )


def copied_changelog_hook(tmp_path: Path) -> Path:
    """Create the minimal root that lets the hook execute its real checker."""
    root = tmp_path / "repository"
    hooks = root / ".codex" / "hooks"
    tools = root / "tools"
    hooks.mkdir(parents=True)
    tools.mkdir()
    (root / "changelog.d").mkdir()
    shutil.copy2(CODEX_HOOKS / "changelog_advisory.py", hooks)
    shutil.copy2(ROOT / "tools" / "aggregate_changelog.sh", tools)
    return hooks / "changelog_advisory.py"


def bash_major_version(env: dict[str, str]) -> int | None:
    result = subprocess.run(
        ["bash", "-c", "echo ${BASH_VERSINFO[0]}"],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    try:
        return int(result.stdout.strip())
    except ValueError:
        return None


def bash32_path_env() -> dict[str, str]:
    """PATH that resolves the platform bash 3.2 calibration lane when present."""
    env = os.environ.copy()
    env["PATH"] = BASH32_CALIB_PATH
    major = bash_major_version(env)
    if major is None or major >= 4:
        pytest.skip("bash 3.2 calibration PATH not available on this host")
    return env


def bash32_only_candidates_env() -> dict[str, str]:
    """Restrict advisory bash discovery to the platform bash 3.2 lane."""
    env = bash32_path_env()
    env["CODEX_CHANGELOG_BASH_CANDIDATES"] = "/bin/bash"
    return env


def bash4_path_env() -> dict[str, str]:
    """PATH that resolves bash 4+ for the fragment-violation control."""
    for prefix in ("/opt/homebrew/bin", "/usr/local/bin"):
        bash = Path(prefix) / "bash"
        if not bash.is_file():
            continue
        env = os.environ.copy()
        env["PATH"] = f"{prefix}:{env.get('PATH', '')}"
        major = bash_major_version(env)
        if major is not None and major >= 4:
            env["CODEX_CHANGELOG_BASH_CANDIDATES"] = str(bash)
            return env
    major = bash_major_version(os.environ.copy())
    if major is not None and major >= 4:
        bash = shutil.which("bash")
        if bash is None:
            pytest.skip("bash 4+ not found for changelog violation control")
        env = os.environ.copy()
        env["CODEX_CHANGELOG_BASH_CANDIDATES"] = bash
        return env
    pytest.skip("bash 4+ not found for changelog violation control")


def load_changelog_hook_module(hook: Path):
    spec = importlib.util.spec_from_file_location("changelog_advisory", hook)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def run_changelog_hook(
    hook: Path, repo_root: Path, env: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(hook)],
        cwd=repo_root,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def event_command(events: Path, label: str, release: Path | None = None) -> str:
    program = (
        "import pathlib,sys,time;"
        "f=pathlib.Path(sys.argv[1]).open('a');"
        "f.write(f'{sys.argv[2]}:start\\n');f.flush();"
        "release=pathlib.Path(sys.argv[3]);"
        "(time.sleep(0.3) if sys.argv[3]=='-' else exec('while not release.exists():\\n time.sleep(0.01)'));"
        "f.write(f'{sys.argv[2]}:end\\n');f.close()"
    )
    release_arg = "-" if release is None else str(release)
    return " ".join(
        [
            shlex.quote(sys.executable),
            "-c",
            shlex.quote(program),
            shlex.quote(str(events)),
            label,
            shlex.quote(release_arg),
        ]
    )


def wait_for_start(events: Path, label: str) -> None:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if events.exists() and f"{label}:start" in events.read_text(encoding="utf-8"):
            return
        time.sleep(0.01)
    raise AssertionError(f"{label} did not start before deadline")


def event_order(events: Path) -> list[str]:
    return events.read_text(encoding="utf-8").splitlines()


def test_codex_hooks_register_cargo_rewrite_and_shared_detectors() -> None:
    hooks = json.loads((ROOT / ".codex" / "hooks.json").read_text(encoding="utf-8"))["hooks"]
    pre_tool = hooks["PreToolUse"]
    assert [group["matcher"] for group in pre_tool] == ["Bash", "apply_patch|Edit|Write"]
    assert "cargo_pre_tool_use.py" in pre_tool[0]["hooks"][0]["command"]
    assert "snapshot_guard.py" in pre_tool[1]["hooks"][0]["command"]
    post_tool = hooks["PostToolUse"]
    assert post_tool[0]["matcher"] == "apply_patch|Edit|Write"
    assert "spdx_guard.py" in post_tool[0]["hooks"][0]["command"]
    assert "changelog_advisory.py" in post_tool[0]["hooks"][1]["command"]


def test_cargo_pre_tool_use_rewrites_only_commands_that_mention_cargo() -> None:
    skipped = run_hook("cargo_pre_tool_use.py", {"cwd": str(ROOT), "tool_input": {"command": "git status"}})
    assert skipped.returncode == 0
    assert skipped.stdout == ""

    original = "CARGO_TARGET_DIR=/tmp/fsl-target cargo test --locked"
    rewritten = run_hook(
        "cargo_pre_tool_use.py", {"cwd": str(ROOT), "tool_input": {"command": original}}
    )
    assert rewritten.returncode == 0, rewritten.stderr
    output = json.loads(rewritten.stdout)["hookSpecificOutput"]
    assert output["permissionDecision"] == "allow"
    command = output["updatedInput"]["command"]
    assert "cargo_lock.py" in command
    assert shlex.split(command)[-1] == original


def test_snapshot_pre_tool_use_denies_direct_snapshot_patch() -> None:
    proc = run_hook(
        "snapshot_guard.py",
        {"tool_input": {"command": "*** Update File: tests/snapshots/corpus_snapshot.json"}},
    )
    assert proc.returncode == 0, proc.stderr
    output = json.loads(proc.stdout)["hookSpecificOutput"]
    assert output["permissionDecision"] == "deny"
    assert "compatibility-contract" in output["permissionDecisionReason"]


def test_changelog_advisory_allows_an_unavailable_checker(tmp_path: Path) -> None:
    hook = copied_changelog_hook(tmp_path)

    proc = run_changelog_hook(hook, hook.parents[2], env=bash32_only_candidates_env())

    assert proc.returncode == 0
    assert "changelog-advisory-unavailable" in proc.stderr
    assert "edit not blocked" in proc.stderr
    assert "Bash 4+ not found" in proc.stderr
    assert "changelog-fragment-violation" not in proc.stderr


def test_changelog_advisory_fail_open_on_checker_oserror(tmp_path: Path) -> None:
    hook = copied_changelog_hook(tmp_path)
    module = load_changelog_hook_module(hook)
    bash4 = Path(bash4_path_env()["CODEX_CHANGELOG_BASH_CANDIDATES"])
    stderr_chunks: list[str] = []

    with patch.object(module, "_find_bash4", return_value=bash4):
        with patch.object(
            module.subprocess,
            "run",
            side_effect=OSError("checker launch failed"),
        ):
            with patch.object(module.sys.stderr, "write", stderr_chunks.append):
                code = module.main()

    stderr = "".join(stderr_chunks)
    assert code == 0
    assert "changelog-advisory-unavailable" in stderr
    assert "checker launch failed" in stderr
    assert "edit not blocked" in stderr
    assert "changelog-fragment-violation" not in stderr


def test_changelog_advisory_fail_open_on_unexpected_checker_exit(tmp_path: Path) -> None:
    hook = copied_changelog_hook(tmp_path)
    checker = hook.parents[2] / "tools" / "aggregate_changelog.sh"
    checker.write_text(
        "#!/bin/sh\n"
        "echo 'checker internal fault' >&2\n"
        "exit 4\n",
        encoding="utf-8",
    )
    checker.chmod(0o755)

    proc = run_changelog_hook(hook, hook.parents[2], env=bash4_path_env())

    assert proc.returncode == 0
    assert "checker failed unexpectedly" in proc.stderr
    assert "edit not blocked" in proc.stderr
    assert "checker internal fault" in proc.stderr
    assert "changelog-fragment-violation" not in proc.stderr


def test_changelog_advisory_blocks_a_fragment_violation(tmp_path: Path) -> None:
    hook = copied_changelog_hook(tmp_path)
    repo_root = hook.parents[2]
    (repo_root / "changelog.d" / "not-a-fragment.md").write_text(
        "Fixed (#947): invalid name fixture.\n", encoding="utf-8"
    )

    proc = run_changelog_hook(hook, repo_root, env=bash4_path_env())

    assert proc.returncode == 2
    assert "changelog-fragment-violation" in proc.stderr
    assert "fix changelog.d/ fragments" in proc.stderr
    assert "changelog-fragment-name-invalid" in proc.stderr


def test_changelog_advisory_fail_open_on_bash32_even_with_fragment_violation(
    tmp_path: Path,
) -> None:
    hook = copied_changelog_hook(tmp_path)
    repo_root = hook.parents[2]
    (repo_root / "changelog.d" / "not-a-fragment.md").write_text(
        "Fixed (#947): invalid name fixture.\n", encoding="utf-8"
    )

    proc = run_changelog_hook(hook, repo_root, env=bash32_only_candidates_env())

    assert proc.returncode == 0
    assert "changelog-advisory-unavailable" in proc.stderr
    assert "edit not blocked" in proc.stderr
    assert "Bash 4+ not found" in proc.stderr
    assert "changelog-fragment-violation" not in proc.stderr


def test_unwrapped_commands_overlap_but_cargo_lock_serializes(tmp_path: Path) -> None:
    unwrapped_events = tmp_path / "unwrapped.txt"
    release = tmp_path / "release"
    first = subprocess.Popen(event_command(unwrapped_events, "first", release), shell=True)
    wait_for_start(unwrapped_events, "first")
    second = subprocess.Popen(event_command(unwrapped_events, "second", release), shell=True)
    wait_for_start(unwrapped_events, "second")
    release.touch()
    assert first.wait(timeout=5) == 0
    assert second.wait(timeout=5) == 0
    unwrapped = event_order(unwrapped_events)
    assert unwrapped.index("second:start") < unwrapped.index("first:end")

    wrapped_events = tmp_path / "wrapped.txt"
    wrapper = CODEX_HOOKS / "cargo_lock.py"
    lock_file = ROOT
    try:
        lock_file = Path(
            subprocess.run(
                ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
        ) / "fsl-cargo.lock"
        existed = lock_file.exists()
        first = subprocess.Popen(
            [sys.executable, str(wrapper), "--cwd", str(ROOT), "--timeout", "5", "--", event_command(wrapped_events, "first")]
        )
        wait_for_start(wrapped_events, "first")
        second = subprocess.Popen(
            [sys.executable, str(wrapper), "--cwd", str(ROOT), "--timeout", "5", "--", event_command(wrapped_events, "second")]
        )
        assert first.wait(timeout=5) == 0
        assert second.wait(timeout=5) == 0
        wrapped = event_order(wrapped_events)
        assert wrapped == ["first:start", "first:end", "second:start", "second:end"]
    finally:
        if isinstance(lock_file, Path) and not existed and lock_file.exists():
            lock_file.unlink()


def test_shared_detectors_reject_missing_headers_and_snapshot_paths(tmp_path: Path) -> None:
    source = tmp_path / "missing.py"
    source.write_text("print('missing header')\n", encoding="utf-8")
    spdx = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "check_spdx_headers.py"), "paths", str(source)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert spdx.returncode == 1
    assert "missing SPDX header" in spdx.stderr

    snapshot = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "check_generated_snapshot.py"), "tests/snapshots/corpus_snapshot.json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert snapshot.returncode == 2
    assert "compatibility-contract" in snapshot.stderr
