---
name: checkpoint
description: Reconcile and save the current FSL task state before compaction, clearing, handoff, or ending substantial work.
disable-model-invocation: true
allowed-tools:
  - Read
  - Grep
  - Glob
  - Bash(git status *)
  - Bash(git diff *)
---

# Checkpoint the active task

Update `.claude/work/active.md`. If it does not exist, create it from the template only when a substantial
task is actually active.

Verify the current conversation's claims against:

- `git status --short`
- `git diff --stat` and the relevant diff
- files and symbols referenced by the task packet
- exact observed command output for verification claims

Preserve the goal, authority surface, invariants, decisions with reasons, exact evidence, changed files,
completed work, failed approaches, exact test outcomes, remaining work, and one next action. Remove
superseded hypotheses, raw logs, conversational narration, completed plans, and machine-specific paths.

Do not claim a test passed because it was planned, started, or passed in an earlier tree state. Do not use
the checkpoint to introduce a new design decision or expand scope silently.
