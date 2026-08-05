<!-- SPDX-License-Identifier: Apache-2.0 -->

# Optimistic merge and post-merge product validation

Status: Accepted

## Decision

Pull requests into `main` run the **complete Linux evidence** — `merge readiness` as a
short-latency fail-fast lane, plus `rust workspace` and `WASM`. Only the **cross-platform matrix**,
`native Z3 4.16` on macOS and Windows, is deferred to after the merge, together with the aggregate
`product gate` context; that matrix also runs on schedule, on manual dispatch, and before promotion
to `production`.

This changes `main` from "every supported platform was green before merge" to "every change was
fully tested on Linux before merge, and the remaining platforms are validated immediately after."
A post-merge failure can therefore still expose a temporarily broken `main`, but only for a
platform-specific defect. The failed check and its deduplicated issue are blocking evidence for
production/release promotion and must be repaired or reverted; they are not informational warnings.

### Why the split falls here

Measured on the batch that motivated this revision:

- **The tests live in the crates the bounded lane excludes.** 103 of 124 integration test files sit
  in `fslc` (77), `fsl-tools` (16), `fsl-verifier` (8) and `fsl-lsp` (2). `merge readiness` runs 21.
- **Its compile lane does not build test targets at all** — `cargo check` without `--all-targets` —
  so a type error or a failing assertion in any of those 103 files reached `main` and surfaced only
  post-merge. Only `cargo fmt --check` covered them, and only for syntactic breakage. Adding
  `--all-targets` there was tried and **reverted**: measured at 12m42s in CI, it destroys the lane's
  reason to exist, and it is redundant once `rust workspace` compiles and runs those targets
  pre-merge.
- **Platform failures have been Windows-specific**: a `python` executable absent from `PATH`, and
  `core.autocrlf` turning `depth = 2` into `depth = 2\r`. No macOS-specific failure has occurred.
- **Windows is the slowest job in the gate** (≈17 min, against ≈12 min for the Linux workspace on a
  warm cache and ≈18 min cold), so deferring the matrix is what buys the merge throughput this
  design is for.

The cost is stated plainly: a pull request into `main` now waits for `rust workspace`, measured at
18m17s on a cold cache and around twelve minutes warm, rather than forty seconds. Because a branch
that falls behind `main` must re-run its checks, a serial chain of rebases pays that repeatedly. Two
mitigations already exist and are worth using before the cost is treated as inherent: independent
changes can share one pull request with one commit per topic, and `merge-readiness.yml` already
handles `merge_group`, so a merge queue can validate several candidates as one batch once `ci.yml`
gains the same trigger.

## Merge readiness contract

`.github/workflows/merge-readiness.yml` runs for `pull_request` and `merge_group` events targeting
`main`. It is now the **fail-fast lane**: its slowest measured contract lane is 33s and the aggregator
4s, so obvious breakage is reported in well under a minute rather than after the Linux workspace job.
Keeping it that fast is the reason `--all-targets` was reverted below. It is no longer the only
pre-merge evidence, and it was never sufficient on its own. The aggregator fails unless all of these
independent lanes succeed:

1. `cargo check --workspace --exclude fsl-solver-z3 --exclude fslc-rust --no-default-features
   --locked` catches compile and dependency-integration drift across the authoritative
   native-Z3-free Rust surface without paying the vendored native-Z3 build cost before merge.
   Excluding the native CLI package as a workspace root avoids its Z3-only helper binaries; its
   library still compiles transitively through the LSP and WASM crates. **`--all-targets` is
   deliberately absent**: it was added, measured at 12m42s, and reverted, because this lane's whole
   value is sub-minute feedback and `rust workspace` already compiles and runs those targets on the
   same pull request.
2. Formatting, the `fsl-syntax`, `fsl-core`, `fsl-runtime`, and backend-neutral `fsl-solver` tests,
   plus the runtime/WASM dependency negative controls, protect the solver-independent semantic
   foundation.
3. The automation contract lane protects post-merge failure creation, duplicate suppression,
   recurrence updates, recovery closure, and workflow-level failure handling. It also runs the
   checked-in Codex/Claude task-harness contracts with the system Python: a discovered soundness
   finding must remain in the worktree ledger until fixed, linked to an issue, or explicitly marked
   as awaiting issue-creation authorization.

The lanes run in parallel through `tools/check-merge-readiness.sh`. Native-CLI/default-feature
compilation, all-target compilation, Clippy, native Z3 verification, the complete LSP/corpus suites,
the full workspace test/build, JavaScript solver probes, and browser Worker validation remain
product-gate evidence. A green readiness check must never be rendered or documented as a fully
verified product.

Superseded runs for the same pull request are cancelled. Merge-group runs are not cancelled and the
workflow handles GitHub's `merge_group` event directly, so a merge queue can validate the combined
candidate against current `main`.

## Product gate contract

`.github/workflows/ci.yml` is named `product gate`. A trusted `main` push runs all of these jobs in
parallel:

- the complete Rust-native integration phase from `tools/check-native-integration.sh rust`
  (`rust workspace`);
- the production WASM/browser phase from `tools/check-native-integration.sh wasm` (`WASM`);
- focused native-Z3 tests on macOS and Windows (`native Z3 4.16`);
- the M13 semantic mutation gate from `tools/check-native-integration.sh semantic-mutation`
  (`semantic mutation`), including curated implementation operators and pinned generic mutants.
- the deterministic finite-model agreement gate from `tools/check-native-integration.sh fsl-logic`
  (`FSL Logic Test`).

**The first two carry no event condition beyond scope evidence and therefore also run on every
pull request today**, which is what makes the Linux evidence pre-merge. Only `native Z3 4.16` and
the aggregate `product gate` context honour `FSL_OPTIMISTIC_CI` and skip on pull requests into
`main`.

All four heavy jobs (`rust workspace`, `WASM`, `semantic mutation`, `FSL Logic Test`) also trigger
on the `merge_group` event. As of this writing that trigger is dormant — no merge queue is
configured on `main`, so `merge_group` never fires — but once one exists, this is where these jobs
are meant to execute for real, batched per queue entry. On `pull_request` events they still run
in full today, because the `FSL_MERGE_QUEUE_CI` repository variable does not exist yet.
Once that variable and a `merge_queue` ruleset rule are added (see "Merge queue (planned, not yet
enabled)" below), pull-request pushes would report cheap queue-entry stubs from
`tools/check-product-gate-scope.sh` instead of running the jobs in full. Until that rollout
happens, treat this paragraph's first sentence — full evidence on every pull request — as the
live contract, and the `merge_group` sentence as forward-looking infrastructure only.

### Agent-configuration exemption

Each of the four heavy jobs runs `tools/check-product-gate-scope.sh` as its first step (named
"Decide required evidence scope"), reads its `run`/`reason` output, and gates every later step on
`steps.scope.outputs.run == 'true'`. This replaced a workflow-level `paths-ignore` list on
`ci.yml`'s `pull_request` trigger that named the same five surfaces: `.claude/**`, `.agents/**`,
`CLAUDE.md`, `AGENTS.md`, and `CHANGELOG.md` (exact root filenames, not prefixes —
`CLAUDE.md.d/x` does not match). `CHANGELOG.md` stays on the list for the exemption to fire at
all — the coupled-change convention gives nearly every notable agent-configuration change a
changelog entry.

The mechanism moved, not the accepted scope, for a concrete reason: a `paths-ignore`-skipped
workflow never emits its job's context at all. If that context were ever made a required status
check — issue #707 tracks exactly this gap — or ever needed to satisfy merge-queue entry, a
workflow-level skip would leave it permanently `Expected`, unfixable even by an admin merge. This
was independently observed while merging PR #715. An in-job early exit does not have that failure
mode: the job always starts, checks out, computes the diff, and reports success either from the
stub path or from real evidence. None of the four contexts are required status checks today —
`merge readiness` remains the only one — so this is a latent-risk fix, not a response to an
already-broken required check.

No **product-gate lane** reads those files. The paths that do read them keep their own
unfiltered pre-merge coverage: `merge readiness / automation contracts` executes the `.claude/`
environment contract test from `tests/test_claude_environment.py`, and `release.yml` extracts
release notes from `CHANGELOG.md` at tag time and fails loudly (`test -s`) on a malformed file.
A pull request whose every changed file matches the list therefore still gets `run=false` from the
scope script, each heavy job exits its remaining steps fast, and the pull request merges on
`merge readiness` (plus `site reference freshness`) alone — exactly as under the retired
`paths-ignore` list.

The exemption is fail-closed the same three ways as before, now enforced by the script rather than
by GitHub's path evaluation. A single changed file outside the list restores the full pre-merge
run. A missing base/head SHA, a failed `git diff`, or an event the script does not recognize all
resolve to `run=true` — never to a stub. The `main` push, schedule, and dispatch triggers carry no
exemption at all: `tools/check-product-gate-scope.sh` returns `run=true` unconditionally for
those events, so every merged `main` state still receives the complete product evidence, and a
wrongly exempted defect surfaces through the existing post-merge reporting contract below instead
of reaching `production`. And a pull request into `production` that matches the list is blocked
rather than waved through: the script's `production` branch always returns `run=true`, so the
production ruleset's required contexts (`rust workspace`, `WASM`, the `native Z3 4.16` matrix)
still report real evidence — such a pull request should not exist, and the gate does not invent a
stub for it.

The script's `selftest` subcommand is the executable accepting/rejecting control for its
classifier (all-exempt, all-product, mixed, a filename-prefix near-miss, and fail-closed-on-empty
cases), run by `merge readiness / automation contracts` on every pull request and merge-group
event.

Growing the exempt list is a contract change to this decision, not a script tweak, and each
candidate needs the same evidence sweep: name every path that reads it and show that path keeps
unfiltered pre-merge or fail-loud coverage. `skills/**` and `docs/**` must never join it:
`skills/fsl/reference.md` moves with language features under the coupled-change contract,
`docs/LANGUAGE*.md` feeds the site-reference freshness gate, and product-gate literate
doc-contract tests read documentation files directly.

`semantic mutation` is required on pull requests and every product-gate event. Ordinary pull
requests run all curated controls plus generic mutants intersecting the recorded base-to-head diff;
pushes, schedules, manual runs, and pull requests targeting `production` run the complete accepted
P2 pilot. The runner is pinned and
its raw machine-readable outcomes are retained as an artifact. A survivor, timeout, stale operator,
missing output, or incomplete job fails the lane; the aggregate gate never treats it as a deliberate
skip. [`DESIGN-semantic-mutation-gate.md`](DESIGN-semantic-mutation-gate.md) owns the detailed scope
and classification contract.

`FSL Logic Test` is likewise required everywhere. Ordinary pull requests run the bounded `pr` case
budget; other product events, including pull requests targeting `production`, run the larger
3-seed/3-depth `scheduled` matrix. Its artifact begins incomplete and is valid
only when expected, executed, and recorded case counts agree and every named semantic edge ran. A
missing or truncated report therefore cannot be mistaken for agreement. The detailed generation,
lineage, replay, shrink, and exclusion contract is in
[`DESIGN-fsl-logic-test.md`](DESIGN-fsl-logic-test.md).

Scheduled and manual runs use the same evidence. Pull requests into `production` also run the
complete product gate and emit the Linux native-Z3 compatibility context expected by the production
ruleset. Release jobs retain their independent four-target build, smoke, ABI, LSP, and packaging
checks. An always-running `product gate` aggregator fails unless every required lane emitted
successful evidence; an accidentally skipped lane cannot make the workflow confidently green.

Product-gate runs for merged commits are not cancelled. Each merged state therefore retains its own
portable evidence and failure attribution even when agents merge changes quickly.

## Merge queue (ruleset live, per-push cost not yet reduced)

**As of this writing, the `main` ruleset (id `19090811`) carries an active `merge_queue` rule and
five required status checks — `merge readiness`, `rust workspace`, `WASM`,
`semantic mutation (changed)`, and `FSL Logic Test (pr)` — applied 2026-08-05. The
`FSL_MERGE_QUEUE_CI` repository variable does not exist yet.** Concretely, this means: a pull
request into `main` can now be added to the queue once all five contexts pass on the pull request
itself, and the queue will re-validate a `merge_group` run before merging — but because
`FSL_MERGE_QUEUE_CI` is unset, `tools/check-product-gate-scope.sh` still returns `run=true` for
ordinary pull requests, so the four heavy jobs still run in full on every push, exactly as before
this section's rollout began. Nothing here changes today's per-push cost yet; it changes what
happens once a pull request author clicks "Merge when ready" — that action now enqueues into a
real, ruleset-configured queue rather than merging immediately once required checks pass on the
branch. Do not cite this ruleset state as evidence that per-push evidence has become cheap; that
is what step 2 below still needs to do.

The target mechanism, once step 2 completes: `ci.yml`'s four heavy jobs would execute for real on
the `merge_group` event, validating each batched queue entry against current `main` instead of
each pull request individually. A `pull_request` push would then report a cheap queue-entry stub
for those same context names — `tools/check-product-gate-scope.sh` returns
`run=false, reason=queue-entry-stub` for that case — deferring the real evidence to queue-entry
time, the same way `native Z3 4.16` already defers to post-merge under `FSL_OPTIMISTIC_CI`.

`ci.yml` already carries the `merge_group` trigger and the scope-check plumbing this needs, and
the ruleset now enforces the target required-context set — but until step 2 completes, every
pull-request run is real evidence, not a stub, so nothing merges on unvalidated stand-ins. Issue
#707 tracks the underlying required-status-check/ruleset gap this mechanism is meant to close;
this repository has already seen a design document assert ruleset enforcement that the live
ruleset did not actually implement, and this section is written to not repeat that mistake by
naming exactly which half is live.

The ruleset's `merge_queue` rule enforcement has one operator escape hatch: `bypass_actors`
carries a single `User` entry (repository admin, `bypass_mode: "pull_request"`), added because the
ruleset's prior `bypass_actors` list was empty and `current_user_can_bypass` was `"never"` for
every account — including admins — which was independently confirmed while merging #715 (`--admin`
could not override an `Expected` required context). Without this entry, a queue malfunction (an
undocumented `merge_group` branch-filter behavior, a `check_response_timeout_minutes` too low for
a slow batch, or any other queue-side failure) would have no direct-merge recovery path short of
reverting this ruleset change via the API. `OrganizationAdmin` was tried first and rejected: this
repository's admin operates at the repository role, not the organization-owner role, so that actor
type resolved to nobody, confirmed by `current_user_can_bypass` staying `"never"` after applying
it. `bypass_mode` is scoped to `"pull_request"` rather than `"always"`, so this bypass cannot be
used to push directly to `main` outside a pull request.

A future PR or operational change would still need to:

1. Create the repository variable `FSL_MERGE_QUEUE_CI` (e.g. set to `enabled`).

Completing this step is what activates the queue-entry-stub path described above. Until then, the
ruleset changes recorded here are enforcement of the *target* required-context set without yet
changing per-push cost — the queue exists and gates merges, but every pull request still earns its
five green checks the expensive way.

## Failure reporting contract

`.github/workflows/post-merge-ci-reporter.yml` listens for completed `product gate` runs. It acts
only when the triggering run was a push to the repository's default branch. The workflow receives
read access to Actions, contents, and pull requests, and write access only to issues.

For each failed, timed-out, cancelled, stale, startup-failed, or action-required job, the reporter:

- creates the `ci/post-merge` label when necessary;
- creates one canonical issue keyed by the product-gate workflow ID and stable job name, reopening
  the same issue after a later recurrence;
- records the commit, associated merged pull request, run, job, conclusion, and failed step names;
- copies no build log text or secret-bearing output into the issue;
- adds one occurrence comment for a later failing run instead of opening a duplicate;
- closes the matching issue only after that job, or the complete product gate, succeeds on a later
  `main` run.

An unsuccessful workflow with no failed-job metadata gets a workflow-level issue, so startup and
orchestration failures do not disappear. Re-running the reporter for the same product-gate run is
idempotent. Before mutating issues, the reporter queries the latest completed trusted push run for
the same workflow. When an older `run_number`/`run_attempt` event arrives, it reconciles that latest
run instead of the stale trigger; out-of-order platform completion cannot overwrite newer
default-branch health or cancel the only reporter for a persistent newer failure.

Reporter jobs are also serialized to avoid duplicate-creation races. When product gates finish
faster than reporting, GitHub may coalesce pending reporter runs toward the newest default-branch
health state; the underlying product-gate result remains attached to every merged commit, and any
failure that persists at the current `main` head remains an open issue.
When a concrete lane and the aggregate `product gate` job fail together, only the concrete lane is
reported; the aggregate is reported on its own only when it is the sole available failure.

## Safe rollout and rollback

The workflow change must land before the ruleset changes; otherwise existing pull requests cannot
produce the new required context. Roll out in this order:

1. Merge the workflow change. `rust workspace` and `WASM` begin running on pull requests
   immediately, as visible but not-yet-required evidence.
2. In the `main` repository ruleset, add `rust workspace` and `WASM` alongside `merge readiness`.
   `native Z3 4.16 (macos-15)` and `native Z3 4.16 (windows-latest)` stay out of the required set —
   they are the deferred cross-platform matrix.
3. Leave `FSL_OPTIMISTIC_CI=enabled`. It now governs only the cross-platform matrix and the
   aggregate `product gate` context; main pushes, schedules, manual runs, and production promotions
   ignore it as before.

Rollback is fail-safe and has two independent levers. To return the Linux evidence to post-merge,
restore the `if:` conditions on `rust workspace` and `WASM` and drop them from the ruleset. To return
to a fully pre-merge gate, delete or change `FSL_OPTIMISTIC_CI` and restore all four product contexts
in the main ruleset. No source or product artifact migration is involved either way.

### Why the ruleset step is not optional

A job that runs on a pull request but is not a required context is *advisory*: it can fail while the
merge still succeeds. Until step 2 lands, this change buys visibility, not enforcement — and the
gap it exists to close is precisely one that a disciplined developer already covers by running
`./tools/check-native-integration.sh` locally. The point of the change is to stop depending on that
discipline, which step 1 alone does not achieve.

## Non-goals

- Treating issue creation as a substitute for repair, revert, or release blocking.
- Hiding platform failures with `continue-on-error`.
- Trusting agent-authored verification claims in place of independent GitHub-hosted execution.
- Changing FSL language, Kernel, CLI/JSON, LSP, Worker, or frozen Python compatibility behavior.
- Enabling the GitHub merge queue or changing any live branch-protection ruleset — this PR lands
  only the inert workflow/script mechanism; see the new "Merge queue (planned, not yet enabled)"
  section.
