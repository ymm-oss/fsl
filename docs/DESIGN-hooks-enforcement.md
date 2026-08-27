<!-- SPDX-License-Identifier: Apache-2.0 -->

# Hook and required-lane enforcement boundaries

Status: Accepted (2026-08-27, #922)

## Decision

Repository policy has one primary enforcement owner for each mechanically
testable rule. Required CI owns rules that must hold for every merge; local
Codex hooks are guardrails for pre-side-effect resource coordination and early
feedback, not a replacement for required CI or human approval.

Codex `PreToolUse` rewrites a Bash command mentioning `cargo` to the repository
Cargo wrapper. The wrapper holds an exclusive advisory `fcntl.flock` on
`$(git rev-parse --git-common-dir)/fsl-cargo.lock` while it executes the
original command. It waits for a lock holder, up to 3600 seconds, and exits
with an error on timeout; it never falls back to concurrent execution. Git's
common directory is shared by linked worktrees, so the one lock inode covers
the observed cross-worktree contention. Kernel-managed advisory locks release
on process exit, including interruption, so a dead PID or PID reuse does not
leave a stale ownership record.

The Codex hook returns `permissionDecision: "allow"` plus an `updatedInput`
command, the supported `PreToolUse` rewrite contract. The wrapper executes the
original shell string with `/bin/bash` (compatible with macOS bash 3.2) rather than trying to parse Cargo flags or target
directories, deliberately serializing every Cargo invocation in this
repository common directory. This is more conservative than a target-keyed
scheme and is the chosen response to the existing shared-resource collision.

## Activation and entry-level trust

Adding an entry to `.codex/hooks.json` declares it but does not make it run.
Codex requires a machine-local, entry-level `trusted_hash` in the developer's
`~/.codex/config.toml` under `[hooks.state]`. Its key has the form
`<hooks.json path>:<event>:<group index>:<hook index>`, so every newly added
entry starts untrusted even when another entry in the same file is trusted.

The checked-in `.codex/config.toml` setting `[features] hooks = true` enables
the feature but is not a substitute for that persisted trust decision. Codex
also exposes `--dangerously-bypass-hook-trust` for an invocation that runs
enabled hooks without persisted trust; whether to use it routinely is an
operational decision and is deliberately not made by this design.

[Issue #929](https://github.com/ymm-oss/fsl/issues/929)'s five-minute sample
captured six independent Cargo processes in two launch forms (directly under
Codex and through `/bin/zsh -c`). None had `cargo_lock.py` as a parent, and
no wrapper process appeared at any sample point. At that observation, only a
`session_start` hook entry was trusted; the Cargo `PreToolUse` entry and the
remaining checked-in entries were not. Consequently, descriptions of hook
behavior in this document state the behavior only after the developer has
accepted each entry's trust prompt.

Trust is local to a developer machine and therefore unavailable to CI. Hook
configuration contract tests may verify a checked-in entry's existence,
matcher, and command, but cannot establish that it is trusted on the machine
that will run it. Hooks must not be counted as merge-time enforcement;
required CI remains the merge-time owner regardless of hook configuration or
local trust state.

## CI-owned checks and shared detectors

The DESIGN-index rule remains owned by the bidirectional map test in
`tests/test_coupled_change_meta.py`; the changelog-fragment naming rule remains
owned by `tools/aggregate_changelog.sh`. Codex calls that exact checker as a
`PostToolUse` advisory after a file edit; it provides fast feedback without
duplicating the parser or classifier. Required CI remains the merge-time
owner.

Direct edits to `tests/snapshots/corpus_snapshot.json` use the common
`tools/check_generated_snapshot.py` detector. Codex calls it before an
`apply_patch` edit, and Claude continues to call it before an `Edit`/`Write`
edit during rollout. The detector blocks the edit and points to the only
regeneration command.

Source-header checking uses the common `tools/check_spdx_headers.py` detector.
The required automation lane checks every newly added source file in
`BASE_SHA...HEAD_SHA`; it intentionally does not retroactively fail historical
source files that predate the rule. Codex and Claude adapters invoke that same
detector after edits for fast feedback. The detector requires the Apache-2.0
SPDX identifier for Python, Rust, JavaScript, TypeScript, and shell source,
and the established copyright line for new Python source.

## Claude hook migration

`session_context.py` remains a Claude-specific session aid. The snapshot
and SPDX hooks remain temporarily, but now act only as adapters to shared
detectors. The per-edit Cargo `fslc_check.py` hook is removed: it skipped Bash
writes and aggravated the lock contention that the Codex wrapper resolves.
The `changelog_reminder.py` Stop hook is removed: it returned success after a
message, observed only one worktree, and duplicated a narrower classification
than the canonical changelog checker now called as Codex advisory feedback.

## Human approval seam

Task-local history policy and evidence sufficiency stay in `AGENTS.md` and the
worktree-local task packet, not in a generic hook heuristic. A task that
declares append-only history requires a new commit unless an exception is
explicitly recorded in the task packet. A partial or single successful command is
never enough to complete a task: the task packet records each required command, expected and
produced results, and the current commit/binary identity; whoever delegated the
task decides whether the evidence is sufficient.

## Controls and limits

The lock control launches two non-Cargo probe commands from the main and a
linked worktree. Without the wrapper their start/end intervals overlap; through
the wrapper, the first end precedes the second start. The probes use no Cargo
binary, so they exercise only lock acquisition and release. The shared
snapshot and SPDX detectors each have rejecting tests, and the SPDX detector is
run in both selftest and live required-lane modes.

Codex hooks can be disabled or bypassed and hosted tools do not traverse the
local hook path. Required CI remains the merge boundary. The hook output shape
and coverage limitations follow the official [Codex Hooks guide](https://learn.chatgpt.com/docs/hooks), rechecked on 2026-08-27.
