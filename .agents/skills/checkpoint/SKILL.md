---
name: checkpoint
description: Reconcile tasks/active.md with Git, changed files, and observed verification before compaction, clearing, or handoff.
---

# Checkpoint an FSL task

Use this skill at a meaningful boundary, before `/compact`, before `/clear`, and before handing work to another agent or person.

1. Re-read `tasks/active.md`, `git status --short`, and the relevant diff.
2. Verify changed files and observed command output instead of relying on conversation history.
3. Preserve the goal, authority surface, current state, decisions, invariants, evidence, changed files,
   failed approaches, verification results, discovered follow-ups, remaining work, and one concrete
   next action.
4. Remove narration, raw logs, superseded hypotheses, and machine-specific absolute paths.
5. Do not claim a test or check passed unless its output was observed in this worktree.
6. Reconcile every discovered soundness defect: it must be fixed in scope, linked to an existing or
   newly created issue, or marked `authorization required` with an explicit user request. Do not mark
   the task complete while such a finding exists only in chat, review output, or agent memory.

The result must let a fresh Codex session continue safely without reconstructing the conversation.
