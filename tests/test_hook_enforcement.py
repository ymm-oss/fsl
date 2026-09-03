# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 FSL Authors
"""Contract tests for the shared Cargo lock and migrated hook detectors."""

from __future__ import annotations

import errno
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import time
from unittest.mock import patch

import pytest


ROOT = Path(__file__).resolve().parents[1]
CODEX_HOOKS = ROOT / ".codex" / "hooks"


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


def no_bash4_candidates_env() -> dict[str, str]:
    """Deterministically simulate bash 4+ discovery finding no candidate."""
    env = os.environ.copy()
    env["CODEX_CHANGELOG_BASH_CANDIDATES"] = ""
    return env


def bash4_path_env() -> dict[str, str]:
    """Return env pinning bash 4+ discovery to a known candidate."""
    # Skip only when this host truly has no bash 4+ for the blocking lane.
    # The no-bash4 fail-open controls use CODEX_CHANGELOG_BASH_CANDIDATES=""
    # instead and must not skip on Linux hosts whose /bin/bash is already 4+.
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

    proc = run_changelog_hook(hook, hook.parents[2], env=no_bash4_candidates_env())

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


def test_changelog_advisory_fail_open_when_no_bash4_even_with_fragment_violation(
    tmp_path: Path,
) -> None:
    hook = copied_changelog_hook(tmp_path)
    repo_root = hook.parents[2]
    (repo_root / "changelog.d" / "not-a-fragment.md").write_text(
        "Fixed (#947): invalid name fixture.\n", encoding="utf-8"
    )

    proc = run_changelog_hook(hook, repo_root, env=no_bash4_candidates_env())

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


PAYLOAD_RELEASE_DEADLINE = 30.0
REENTRANCY_ENV = "FSL_CARGO_LOCK_HELD"


def cargo_lock_repo(tmp_path: Path, name: str = "repo") -> Path:
    """A Git repository of this test's own.

    The wrapper locks one inode in the Git *common* directory, which every
    linked worktree of this repository -- including the developer's -- shares.
    A control that took that inode would have a verdict that depends on whether
    someone else is running Cargo right now, so each control gets its own
    repository instead.
    """
    repo = tmp_path / name
    repo.mkdir()
    git = ["git", "-C", str(repo)]
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run([*git, "config", "user.email", "issue946@example.invalid"], check=True)
    subprocess.run([*git, "config", "user.name", "issue946"], check=True)
    (repo / "seed").write_text("seed\n", encoding="utf-8")
    subprocess.run([*git, "add", "seed"], check=True)
    subprocess.run([*git, "commit", "-q", "-m", "seed"], check=True)
    return repo


def cargo_lock_file(repo: Path) -> Path:
    common = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    return Path(common) / "fsl-cargo.lock"


def cargo_lock_env(**overrides: str) -> dict[str, str]:
    """The ambient marker is removed: a control must not read one from outside."""
    env = dict(os.environ)
    env.pop(REENTRANCY_ENV, None)
    env.update(overrides)
    return env


def cargo_lock_argv(command: str, timeout: float, cwd: Path) -> list[str]:
    return [
        sys.executable,
        str(CODEX_HOOKS / "cargo_lock.py"),
        "--cwd",
        str(cwd),
        "--timeout",
        str(timeout),
        "--",
        command,
    ]


def cargo_lock_payload(tmp_path: Path) -> Path:
    """A payload addressed by bare tokens only.

    ``event_command`` quotes its arguments, which is correct for one wrapper but
    not for a nested one: the outer ``bash -lc`` consumes that quoting, and the
    inner wrapper rejoins what is left with spaces, so a quoted ``-c`` program
    arrives split. Every argument here is a path or a bare label instead, which
    survives any nesting depth unchanged. The wait for the release file is
    bounded so that a payload left behind by a failing control cannot hold the
    repository lock indefinitely.
    """
    script = tmp_path / "cargo_lock_payload.py"
    script.write_text(
        "import pathlib, sys, time\n"
        "events, label, release, code = sys.argv[1:5]\n"
        "handle = pathlib.Path(events).open('a', encoding='utf-8')\n"
        "handle.write(label + ':start\\n')\n"
        "handle.flush()\n"
        "if release == '-':\n"
        "    time.sleep(0.3)\n"
        "else:\n"
        "    target = pathlib.Path(release)\n"
        "    deadline = time.monotonic() + PAYLOAD_RELEASE_DEADLINE\n"
        "    while not target.exists() and time.monotonic() < deadline:\n"
        "        time.sleep(0.01)\n"
        "handle.write(label + ':end\\n')\n"
        "handle.close()\n"
        "sys.exit(int(code))\n".replace(
            "PAYLOAD_RELEASE_DEADLINE", str(PAYLOAD_RELEASE_DEADLINE)
        ),
        encoding="utf-8",
    )
    return script


def cargo_lock_payload_command(payload: Path, events: Path, label: str, release: str, code: int) -> str:
    return " ".join([sys.executable, str(payload), str(events), label, release, str(code)])


def cargo_lock_spawn(
    command: str, timeout: float, cwd: Path, **env_overrides: str
) -> subprocess.Popen[bytes]:
    """Start a wrapper in its own session so the whole tree can be reaped.

    A payload left running holds the repository-wide Cargo lock. Killing only
    the direct child leaves the ``bash -lc`` grandchild holding it, so every
    control starts a session it can kill as a group.
    """
    return subprocess.Popen(
        cargo_lock_argv(command, timeout, cwd),
        start_new_session=True,
        env=cargo_lock_env(**env_overrides),
    )


def cargo_lock_reap(*processes: subprocess.Popen[bytes] | None) -> None:
    """Kill each group, whether or not its leader has already exited.

    A shell can exit while leaving a descendant of its process group running,
    so ``poll()`` returning a status is not evidence that the group is empty.
    """
    for process in processes:
        if process is None:
            continue
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        if process.poll() is None:
            process.kill()
        process.wait(timeout=10)


def cargo_lock_order(events: Path) -> list[str]:
    if not events.exists():
        return []
    return events.read_text(encoding="utf-8").splitlines()


def test_a_nested_cargo_lock_invocation_passes_through_instead_of_deadlocking(
    tmp_path: Path,
) -> None:
    """Issue #946: the inner wrapper must not wait for the lock its parent holds.

    Detector: with the guard reverted this returns 2 with ``timed out`` on
    stderr after the inner ``--timeout``, never reaching the payload's own exit
    code -- and the outer wrapper holds the repository-wide lock for that whole
    time, which is what stalls unrelated worktrees. The elapsed time is reported
    beside the bound because "eventually" is not what the issue asks for.
    """
    repo = cargo_lock_repo(tmp_path)
    events = tmp_path / "events.txt"
    payload = cargo_lock_payload(tmp_path)
    inner_timeout = 4.0
    innermost = cargo_lock_payload_command(payload, events, "ran", "-", 7)
    nested = " ".join(cargo_lock_argv(innermost, inner_timeout, repo))

    process = cargo_lock_spawn(nested, 30, repo)
    started = time.monotonic()
    try:
        returncode = process.wait(timeout=60)
    finally:
        cargo_lock_reap(process)
    elapsed = time.monotonic() - started

    assert returncode == 7, f"expected 7, produced {returncode}"
    assert cargo_lock_order(events) == ["ran:start", "ran:end"]
    assert elapsed < inner_timeout, f"expected < {inner_timeout}s, produced {elapsed:.2f}s"


def test_an_unrelated_invocation_waits_while_a_pass_through_child_runs(
    tmp_path: Path,
) -> None:
    """Issue #946: the guard must not release the lock the outer invocation holds.

    Detector, for the opposite mistake from the test above: a "guard" that
    simply stopped taking the lock would satisfy that one and fail this one,
    because the unrelated invocation would start before the pass-through child
    finished.
    """
    repo = cargo_lock_repo(tmp_path)
    events = tmp_path / "events.txt"
    release = tmp_path / "release"
    payload = cargo_lock_payload(tmp_path)
    innermost = cargo_lock_payload_command(payload, events, "inner", str(release), 0)
    unrelated = cargo_lock_payload_command(payload, events, "other", "-", 0)

    nested = cargo_lock_spawn(" ".join(cargo_lock_argv(innermost, 30, repo)), 30, repo)
    other = None
    try:
        wait_for_start(events, "inner")
        other = cargo_lock_spawn(unrelated, 30, repo)
        time.sleep(1)
        release.touch()
        assert nested.wait(timeout=30) == 0
        assert other.wait(timeout=30) == 0
    finally:
        cargo_lock_reap(nested, other)

    assert cargo_lock_order(events) == ["inner:start", "inner:end", "other:start", "other:end"]


def test_a_marker_from_another_repository_does_not_bypass_this_lock(tmp_path: Path) -> None:
    """Issue #946: being inside *a* wrapper is not being inside *this* lock's wrapper.

    Detector for a re-entry guard that trusts the marker too broadly, not for
    the defect this issue reports: a marker saying only "somebody holds
    something" lets a wrapper for repository B skip repository B's lock merely
    because its caller holds repository A's. Reverting the guard leaves this
    green, because a wrapper that never bypasses cannot bypass wrongly -- so it
    establishes nothing about the original defect, and everything about the
    shape of the fix.
    """
    repo_a = cargo_lock_repo(tmp_path, "repo_a")
    repo_b = cargo_lock_repo(tmp_path, "repo_b")
    events = tmp_path / "events.txt"
    release = tmp_path / "release"
    payload = cargo_lock_payload(tmp_path)

    holder = cargo_lock_spawn(
        cargo_lock_payload_command(payload, events, "b-holder", str(release), 0), 30, repo_b
    )
    nested = None
    try:
        wait_for_start(events, "b-holder")
        inner = " ".join(
            cargo_lock_argv(
                cargo_lock_payload_command(payload, events, "b-nested", "-", 0), 30, repo_b
            )
        )
        nested = cargo_lock_spawn(inner, 30, repo_a)
        time.sleep(1)
        release.touch()
        assert holder.wait(timeout=30) == 0
        assert nested.wait(timeout=30) == 0
    finally:
        cargo_lock_reap(holder, nested)

    assert cargo_lock_order(events) == [
        "b-holder:start",
        "b-holder:end",
        "b-nested:start",
        "b-nested:end",
    ]


def test_a_marker_left_behind_after_the_lock_was_released_does_not_bypass(
    tmp_path: Path,
) -> None:
    """Issue #946: the marker outlives the wrapper that minted it.

    Detector for a re-entry guard that does not check whether the lock is still
    held, not for the defect this issue reports. A shell started under the
    wrapper can leave a long-lived descendant; once the wrapper exits and
    releases the lock, that descendant still carries the marker, and must take
    the lock like anyone else. Reverting the guard leaves this green for the
    same reason as the control above.
    """
    repo = cargo_lock_repo(tmp_path)
    events = tmp_path / "events.txt"
    release = tmp_path / "release"
    payload = cargo_lock_payload(tmp_path)
    stale_marker = str(cargo_lock_file(repo))

    stale = cargo_lock_spawn(
        cargo_lock_payload_command(payload, events, "stale", str(release), 0),
        30,
        repo,
        **{REENTRANCY_ENV: stale_marker},
    )
    other = None
    try:
        wait_for_start(events, "stale")
        other = cargo_lock_spawn(
            cargo_lock_payload_command(payload, events, "other", "-", 0), 30, repo
        )
        time.sleep(1)
        release.touch()
        assert stale.wait(timeout=30) == 0
        assert other.wait(timeout=30) == 0
    finally:
        cargo_lock_reap(stale, other)

    assert cargo_lock_order(events) == ["stale:start", "stale:end", "other:start", "other:end"]


def test_a_login_profile_that_unsets_the_marker_does_not_restore_the_deadlock(
    tmp_path: Path,
) -> None:
    """Issue #946: ``/bin/bash -lc`` runs the profile before the command.

    Detector. Passing the marker only through the child's environment leaves it
    at the mercy of the user's own profile, and a profile that unsets it
    reintroduces exactly the self-deadlock this issue is about.
    """
    repo = cargo_lock_repo(tmp_path)
    home = tmp_path / "home"
    home.mkdir()
    for profile in (".bash_profile", ".profile"):
        (home / profile).write_text(f"unset {REENTRANCY_ENV}\n", encoding="utf-8")
    events = tmp_path / "events.txt"
    payload = cargo_lock_payload(tmp_path)
    innermost = cargo_lock_payload_command(payload, events, "ran", "-", 0)
    nested = " ".join(cargo_lock_argv(innermost, 4, repo))

    process = cargo_lock_spawn(nested, 30, repo, HOME=str(home))
    try:
        returncode = process.wait(timeout=60)
    finally:
        cargo_lock_reap(process)

    assert returncode == 0, f"expected 0, produced {returncode}"
    assert cargo_lock_order(events) == ["ran:start", "ran:end"]


def test_two_linked_worktrees_still_serialize_through_one_lock(tmp_path: Path) -> None:
    """Preservation control: it passes with the guard and with the guard reverted.

    It establishes only that the change did not disturb the behaviour the issue
    says must not change -- one lock inode per Git common directory, shared by
    every linked worktree. It is not a detector for the re-entry defect, and
    reverting the guard leaves it green.
    """
    primary = cargo_lock_repo(tmp_path)
    secondary = tmp_path / "secondary"
    subprocess.run(
        ["git", "-C", str(primary), "worktree", "add", "-q", str(secondary)], check=True
    )
    events = tmp_path / "events.txt"
    release = tmp_path / "release"
    payload = cargo_lock_payload(tmp_path)

    first = cargo_lock_spawn(
        cargo_lock_payload_command(payload, events, "first", str(release), 0), 30, primary
    )
    second = None
    try:
        wait_for_start(events, "first")
        second = cargo_lock_spawn(
            cargo_lock_payload_command(payload, events, "second", "-", 0), 30, secondary
        )
        time.sleep(1)
        release.touch()
        assert first.wait(timeout=30) == 0
        assert second.wait(timeout=30) == 0
    finally:
        cargo_lock_reap(first, second)

    assert cargo_lock_order(events) == ["first:start", "first:end", "second:start", "second:end"]


def test_without_a_marker_a_plain_invocation_behaves_as_before(tmp_path: Path) -> None:
    """Preservation control: it passes with the guard and with the guard reverted.

    The issue asks for evidence that behaviour is unchanged when the marker is
    absent. Reverting the guard leaves this green, which is what makes it a
    preservation control rather than a detector: it establishes that ordinary
    single-level use -- exit-code propagation, and timing out under real
    contention -- was not disturbed.
    """
    repo = cargo_lock_repo(tmp_path)
    events = tmp_path / "events.txt"
    release = tmp_path / "release"
    payload = cargo_lock_payload(tmp_path)

    plain = cargo_lock_spawn(
        cargo_lock_payload_command(payload, events, "plain", "-", 7), 30, repo
    )
    try:
        assert plain.wait(timeout=30) == 7
    finally:
        cargo_lock_reap(plain)
    assert cargo_lock_order(events) == ["plain:start", "plain:end"]

    events.unlink()
    holder = cargo_lock_spawn(
        cargo_lock_payload_command(payload, events, "holder", str(release), 0), 30, repo
    )
    blocked = None
    try:
        wait_for_start(events, "holder")
        blocked = subprocess.Popen(
            cargo_lock_argv(cargo_lock_payload_command(payload, events, "blocked", "-", 0), 2, repo),
            start_new_session=True,
            env=cargo_lock_env(),
            stderr=subprocess.PIPE,
            text=True,
        )
        blocked_stderr = blocked.communicate(timeout=30)[1] or ""
        blocked_code = blocked.returncode
        release.touch()
        assert holder.wait(timeout=30) == 0
    finally:
        cargo_lock_reap(holder, blocked)

    assert blocked_code == 2, f"expected 2, produced {blocked_code}"
    assert "timed out" in blocked_stderr, f"expected 'timed out', produced {blocked_stderr!r}"


def load_cargo_lock_module():
    spec = importlib.util.spec_from_file_location(
        "cargo_lock_under_test", CODEX_HOOKS / "cargo_lock.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_a_lock_error_that_is_not_contention_is_reported_at_once(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Issue #946: only ``BlockingIOError`` means "somebody else holds it".

    Detector. A guard that catches ``OSError`` broadly cannot tell contention
    from a filesystem that has no locks, so it waits out ``--timeout`` -- up to
    the 3600-second default -- and then reports a timeout that never happened.
    That is the stall this issue exists to remove, reached by a different route.

    The fault is injected rather than asserted about the source text, and the
    verdict does not depend on ambient state: with ``flock`` patched no lock is
    taken, so nothing here contends with another worktree.
    """
    module = load_cargo_lock_module()
    lock_file = module.lock_path(ROOT)
    existed = lock_file.exists()
    injected = OSError(errno.ENOLCK, "No locks available")

    def refuse(*_args: object, **_kwargs: object) -> None:
        raise injected

    try:
        with patch.object(module.fcntl, "flock", side_effect=refuse):
            started = time.monotonic()
            returncode = module.run("true", ROOT, 30.0)
            elapsed = time.monotonic() - started
    finally:
        if not existed and lock_file.exists():
            lock_file.unlink()

    produced = capsys.readouterr().err
    assert returncode == 2, f"expected 2, produced {returncode}"
    assert elapsed < 5, f"expected < 5s, produced {elapsed:.2f}s"
    assert "No locks available" in produced, f"expected the real error, produced {produced!r}"
    assert "timed out" not in produced, f"expected no timeout claim, produced {produced!r}"


def test_a_signalled_shell_is_reported_as_the_shell_would_report_it(
    tmp_path: Path,
) -> None:
    """The wrapper must not turn a signal into a number no shell produces.

    Detector, added after reading the implementation rather than before: the
    unmodified baseline returns ``subprocess``'s negative code, which
    ``sys.exit`` masks to 247 for SIGKILL. 137 is what a shell reports, and it
    is what "return the command's exit code unchanged" means to a caller.

    The signal is injected at the ``subprocess.run`` boundary because killing a
    real ``bash -lc`` from inside a control would race with its own teardown.
    """
    module = load_cargo_lock_module()
    lock_file = module.lock_path(ROOT)
    existed = lock_file.exists()
    real_run = module.subprocess.run

    def kill_only_the_shell(argv, *args, **kwargs):
        # `common_directory` shells out to git through the same name, so the
        # injection has to be narrowed to the shell invocation itself.
        if list(argv[:1]) == ["/bin/bash"]:
            return subprocess.CompletedProcess(args=argv, returncode=-9)
        return real_run(argv, *args, **kwargs)

    try:
        with patch.object(module.subprocess, "run", side_effect=kill_only_the_shell):
            returncode = module.run("true", ROOT, 30.0)
    finally:
        if not existed and lock_file.exists():
            lock_file.unlink()

    assert returncode == 137, f"expected 137, produced {returncode}"


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
