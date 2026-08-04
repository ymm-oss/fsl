---
name: pr-review
description: Independently review a pull request that resolves an issue. Reconstructs the issue contract from repository evidence, dispatches the specialized FSL reviewer agents in parallel, audits the diff for green-faking, reproduces verification claims, and delivers a severity-ranked verdict without merging.
---

# Review a pull request

Orchestrate an independent review of a pull request, typically one authored by an agent to
resolve an issue. This skill is the coordination layer; the per-dimension analysis belongs to
the specialized read-only reviewer agents under `.claude/agents/`. Diagnose and report — do not
push fixes onto the branch, and never merge (leave the PR open for human review).

Inputs: a PR number or branch, and the linked issue. If no issue is linked, ask for the intended
contract before reviewing.

Lenses that apply to every phase:

- Trust order per `AGENTS.md`: contracts and tests, then the Rust implementation, then frozen
  Python behavior, then prose. The PR description and any implementer conversation are claims,
  not evidence.
- Suspect green. A confidently green false negative is more dangerous than a crash, so "the
  checks pass" is never sufficient on its own.
- Findings outside the diff still carry a reporting duty: an accepted construct with absent,
  placeholder, or hollow semantics is a soundness defect wherever it is found.

## Phase 0 — Reconstruct the contract

1. Read the PR (`gh pr view`, `gh pr diff`), the linked issue, and any `docs/DESIGN-*.md` the
   change touches or should have touched.
2. Restate, from the issue and repository evidence only: the requested outcome, the affected
   authority surface, the invariants at risk, and the verification the change needs.
3. Note which CI lanes actually ran. `merge readiness` is a bounded fail-fast lane
   (`docs/DESIGN-ci.md`), not product verification — record what remains unverified.

## Phase 1 — Contract alignment

Map the issue's acceptance criteria to the diff in both directions:

- Required but missing: criteria with no corresponding change or test.
- Present but not required: scope creep, speculative fallbacks, compatibility work no contract
  calls for.

Check authority placement: new behavior lives under `rust/`, not the frozen `src/fslc/`; design
decisions are backed by an accepted design note; the change is the smallest contract-preserving
one.

## Phase 2 — Dispatch the specialized reviewers

Select every applicable reviewer from the table and launch the selected ones in parallel. Each
returns compact findings; do not re-run their file-level sweeps in the main thread.

The reviewer agents have tight turn budgets and routinely exhaust them mid-exploration. In each
dispatch prompt: point the agent at a checkout of the PR's tree (create one with
`git worktree add <scratchpad>/prNNN <head-ref> --detach`; remove it after the review), state
the base commit for diffing, enumerate the audit items, and instruct the agent to reserve budget
for the final report — evidence gathered but never reported is wasted. If an agent still stops
without a report, resume it with a "produce your final report now" message instead of
re-launching it.

| Diff touches | Reviewer agent |
| --- | --- |
| Grammar, lowering, CLI commands, public Kernel/JSON contracts, docs, corpus specs | `fsl-coupled-change-reviewer` |
| `fsl-core`, `fsl-runtime`, `fsl-verifier`, `fsl-solver*`, refinement or dialect semantics | `fsl-soundness-reviewer` |
| `.fsl` files under `specs/` or `examples/` | `fsl-vacuity-reviewer` |

A language-feature change dispatches the coupled-change reviewer even when the diff looks
complete — the coupled-change list (LSP index, `LANGUAGE.ja.md` alignment, dialect registry,
changelog) is exactly what implementers forget.

## Phase 3 — Green-faking audit

On the diff itself, in the main thread, check for changes that make checks pass without
honoring the contract:

- A `.fsl` spec, property, or invariant weakened relative to the base branch.
- Hand-edited generated snapshots, or snapshots regenerated without an intentional contract
  change to justify the diff.
- Growth in an allowlist, exclusion, or known-divergence list (`tests/dialect_registry.py`
  entries, `KNOWN_DIVERGENT_*` fixtures, skip markers) presented as a fix.
- Errors suppressed, downgraded, or swallowed instead of resolved.
- Test assertions loosened, deleted, or rewritten to follow the new behavior without evidence
  the new behavior is the contract.
- New positive-path tests with no negative control — a test suite that cannot reject a known
  contract-violating variant proves nothing about drift detection.
- Weak assertions: each new test must verify through the strongest available oracle (re-parse,
  type-check, `build_model`, exact message/location/exit code), not substring or line-presence
  matching. Issue #691 survived characterization tests precisely because they matched substrings
  without re-parsing the rendered output. Ask of every new test: would it have caught the bug it
  claims to prevent?

## Phase 4 — Reproduce verification claims

Re-run the narrowest relevant tests and `fslc` commands yourself in the PR's worktree state and
compare observed output with the PR body's claims. Do not accept a pass that was only planned,
only claimed, or only observed in an earlier tree state. Delegate verbose or ambiguous failure
output to `fsl-test-diagnostician` and keep exact commands, exit codes, and failing test names.

## Phase 4b — Adversarial verification (soundness-critical PRs)

When the central change touches verifier, lowering, or dialect semantics, reproduced green is
still the implementer's chosen evidence. Add evidence the implementer did not choose:

1. **Novel inputs.** Author 2–3 new `.fsl` fixtures that exercise edge shapes the PR's own tests
   avoid (adjacent type constructors, generated names, boundary arities, nesting the fix does
   not demonstrate). Run them through the strongest oracle on the PR tree — for two-path
   surfaces, execute both paths and compare verdicts, not text. Prompt the authoring agent to
   refute the PR's claim, not to confirm it. Keep these fixtures in the review packet; promote
   any that find a divergence into the PR's regression suite as a requested change.
2. **Live mutation of the negative control.** For the PR's central fix, revert or corrupt the
   fixed arm in the scratch checkout and observe the claimed guard test actually fail. A
   negative control verified only by reading is uncalibrated; one compile cycle buys executed
   proof that the gate can detect the regression it pins. Restore the checkout afterwards.

Skip this phase for doc, fixture-only, or tooling changes; say so explicitly in the verdict.

## Phase 5 — Synthesize the verdict

Build a claim ledger before writing the verdict: enumerate every verification claim made by the
PR body and every claim the verdict itself is about to make, and classify each as verified by
execution (command + observed output), verified by reading (file:line evidence), or unverified.
The verdict must name its unverified residue explicitly — a claim the review neither executed
nor read is an assumption, and certifying it is the reviewer's own green-faking. Rank the
remainder by severity:

1. Soundness defect (false-negative risk, hollow accepted semantics, broken
   symbolic/concrete/BFS agreement).
2. Contract violation or drift (public envelope, exit codes, accepted design notes).
3. Coupled-change gap (missing docs, LSP index, registry, changelog, skill reference).
4. Test-evidence gap (missing negative control or boundary case, unreproduced claim).
5. Style and convention.

Separately, record local-optimum escalation triggers without acting on them: the fix works by
adding a compatibility layer or special-case branch; one semantic change fans out across many
files; the same shape of fix recurs across recent PRs; an allowlist or exclusion list grows
monotonically. Two or more recorded triggers on the same surface warrant a standalone
local-optima audit outside any single PR review — note it, do not run it here.

## Phase 6 — Deliver and persist

1. Post the review with `gh pr review` (comment or request-changes; approve only when every
   severity-1/2 finding is resolved or refuted). State what was reproduced, what was not, and
   what remains unverified. Never merge.
2. Every out-of-scope soundness finding must land as an issue URL or an explicit unresolved
   follow-up in the task packet — never only in the review text or chat.
3. Promote durable lessons (a recurring finding class, a missed coupled-change edge) to the
   smallest authoritative surface: a test, a design note, or an agent rule.
