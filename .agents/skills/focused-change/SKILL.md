---
name: focused-change
description: Make a bounded repository change with narrow discovery, minimal diffs, staged validation, concise evidence, and controlled command output. Use for ordinary feature work, bug fixes, test additions, and small refactors with clear acceptance conditions.
---

# Focused change workflow

## Input contract

Extract and keep visible only:

1. Goal
2. Target paths or symbols
3. Constraints and non-goals
4. Observable acceptance conditions
5. Required checks

When one of these is missing, infer at most three low-risk assumptions from repository evidence. Do not expand the task to “improve the surrounding code.”

## FSL repository routing

In the FSL repository, read
[`references/fsl-repository-contract.md`](references/fsl-repository-contract.md)
completely before changing implementation, tests, verification, language
semantics, conformance, CLI/Worker/Kernel contracts, generated snapshots,
soundness evidence, CI/product gates, or release surfaces. The reference
preserves detailed repository obligations that are intentionally omitted from
the short root `AGENTS.md`.

For a small prose-only correction that cannot affect a contract or executable
surface, the root `AGENTS.md` is sufficient.

## Discovery funnel

1. Run `git status --short`.
2. Search named symbols, error strings, tests, or routes with `rg -n`.
3. Read only the relevant ranges and immediately adjacent contracts.
4. Identify the shortest test or reproducer before changing code.
5. Widen to callers/dependencies only when the current evidence cannot explain behavior.

Stop discovery when there is enough evidence to describe:

- the current behavior,
- the intended behavior,
- the smallest likely change,
- how the change will be falsified.

## Plan

For a small task, give a plan of at most four steps. For a trivial mechanical task, proceed without a separate plan. Do not create a long planning document unless requested.

## Implementation

- Make the smallest coherent edit.
- Follow an existing local pattern before inventing an abstraction.
- Avoid new dependencies unless acceptance requires them.
- Do not rewrite unaffected code.
- Add or update a test when it is the clearest executable expression of the changed contract.

## Command-output control

For noisy commands, use:

```bash
./scripts/cx-log <short-label> -- <command> [args...]
```

Read the complete log only if the summary is insufficient. Then search it for a specific pattern or read a bounded range; do not paste it whole into the conversation.

## Validation ladder

1. Focused test/reproducer
2. Target package/module checks
3. Broader impacted suite
4. Full suite only when change risk or repository policy requires it

After a failed check, do not rerun unchanged. First change the implementation/configuration, form a new hypothesis, or reduce the reproducer.

## Self-review

Inspect:

```bash
git diff --stat
git diff -- <relevant paths>
```

Check correctness, boundary conditions, failure handling, unintended files, public API changes, test coverage, and constraint violations.

## Final evidence

Report only:

- behavior changed,
- files changed,
- checks run and their result,
- any unresolved risk or skipped check.

Do not repeat code or logs already available in the repository.
