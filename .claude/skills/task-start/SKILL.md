---
name: task-start
description: Create or refresh the worktree-local active task packet before substantial FSL repository work. Reconciles the requested outcome with Git state, accepted contracts, and the authoritative implementation without starting implementation.
argument-hint: "[issue or goal]"
disable-model-invocation: true
allowed-tools:
  - Read
  - Grep
  - Glob
  - Bash(git status *)
  - Bash(git diff *)
  - Bash(git branch *)
---

# Start or refresh the active task

Create or update `.claude/work/active.md` from `.claude/work/active.template.md`.

1. Inspect `git status --short`, current branch/worktree, and any existing active packet.
2. Normalize `$ARGUMENTS` into an observable goal and explicit non-goals.
3. Identify the accepted language/design/CLI contract and whether the authority surface is native Rust,
   frozen Python compatibility/LSP, WASM, or documentation/skills.
4. Inspect only the implementation and tests required to establish current state. For broad exploration,
   delegate to `fsl-codebase-explorer` and retain only its evidence summary.
5. Record invariants, exact repository-relative evidence, expected scope, sibling paths, verification
   commands, uncertainty, and the first next action.
6. Compare the packet with the working tree. Remove stale claims and never mark a change or test complete
   without direct evidence.
7. Stop after writing the task packet. Do not implement the task in this skill invocation.

Do not copy raw logs, conversation narration, absolute machine paths, or unverified hypotheses into the
packet. A human should be able to resume from this file, Git, and accepted design documents alone.
