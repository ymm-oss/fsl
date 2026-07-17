@AGENTS.md

# Claude Code instructions

## Sources of truth

- The working tree and verified repository artifacts are current state; conversation history is not.
- The native Rust workspace is authoritative. Treat `src/fslc/` as a frozen compatibility/LSP
  surface unless the requested outcome explicitly crosses that boundary.
- Accepted decisions live in `docs/DESIGN-*.md`. Current task state lives in
  `.claude/work/active.md` when that local file exists.
- Do not treat proposals, plans, auto memory, or an earlier session's claims as implemented behavior.

## Starting or resuming work

1. Inspect `git status --short` and the relevant implementation before editing.
2. If `.claude/work/active.md` exists, compare it with the working tree and report stale claims.
3. For a substantial task, use `/task-start` to create or refresh the task packet before implementation.
4. State the requested outcome, affected authority surface, invariant, and narrow verification plan.
5. Do not read unrelated directories without a concrete reason.

## Implementation policy

- Prefer the smallest contract-preserving change. Do not add fallback behavior or compatibility work
  unless the requirement or an established public contract calls for it.
- For new language behavior, change the Rust implementation first. Change the frozen Python reference
  only when an accepted compatibility decision requires both implementations to move.
- Preserve the runtime/solver dependency boundary and symbolic/concrete/BFS agreement.
- Keep CLI stdout machine-readable where the command contract requires JSON; diagnostics and progress
  belong on the established channel.
- Never suppress an error, weaken a spec, hand-edit a generated snapshot, or expand an allowlist merely
  to make a check green.

## Verification and review

1. Run the narrowest relevant Rust test or native `fslc` command first.
2. Inspect the diff and exercise the changed contract with positive, negative, and boundary evidence.
3. Run broader Rust, compatibility, or browser gates in proportion to the affected surface.
4. Use the specialized FSL reviewers after semantics, coupled language files, or specs change.
5. Before ending or compacting substantial work, run `/checkpoint` and preserve exact test outcomes.

## Context management

- Delegate broad exploration and verbose failure diagnosis to the dedicated read-only agents.
- Return compact evidence reports, not raw grep output or full logs, to the main conversation.
- Preserve exact paths, symbols, commands, exit codes, and failing test names in summaries.
- Use `/compact` at phase boundaries and `/clear` when switching to unrelated work. A fresh session must
  be able to reconstruct the task from the task packet, Git diff, tests, and accepted design documents.
- Auto memory is a convenience cache only. Team rules, task state, and design decisions belong in the
  repository locations above.
