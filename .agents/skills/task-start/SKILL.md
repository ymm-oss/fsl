---
name: task-start
description: Create or refresh the verified, resumable task packet in tasks/active.md before substantial FSL repository work.
---

# Start an FSL task

Use this skill before substantial implementation, debugging, review, or design work.

1. Inspect the current branch and `git status --short`.
2. Restate the requested outcome as an observable goal and list explicit non-goals.
3. Identify the authority surface: relevant contracts, design documents, tests, native Rust paths, and user constraints.
4. Inspect only the narrow evidence needed to establish the current state. Delegate broad, read-heavy exploration to `evidence_explorer` when it reduces the main context.
5. Create or replace `tasks/active.md` from `tasks/active.template.md`.
6. Record invariants, evidence, scope, sibling paths, verification commands, remaining uncertainty, and one concrete next action.
7. Remove stale or speculative content from a previous task.
8. Stop before implementation so the task packet can be checked independently.

Keep the packet concise. Do not paste raw logs, full search results, secrets, or machine-specific absolute paths.
